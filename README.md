# bitty-platform-services

Bitty L1 Rust Core Extension: the platform notification bridge.

This repository DEFINES the API that Bitty Core consumes to deliver desktop
notifications. It does not parse terminal output: Core parses OSC 777 and
Kitty notification sequences and enforces the RC-8 rate budget
(`bitty#1763`, OQ-076) before anything here runs. This crate receives an
already-gated `{title, body}` pair, applies a defensive second rate cap, and
dispatches through a per-platform backend behind a trait. Task management
lives in CarryCtx.

## Boundary

- Core owns: OSC 777 / Kitty parsing, consent and permission gating,
  sanitization policy input, and the RC-8 limiter (10 events/s coalesced,
  bounded queue). That work is a separate task in `bitty`; this repository
  never touches `bitty`.
- This repo owns: the `DesktopNotification` / `NotificationBackend` /
  `NotificationBridge` API, per-platform backend adapters, and the defensive
  second cap. The adapter only executes what Core authorizes (ADR-0016
  Boundary 5, focused contract W-136).

## API

```rust
use bitty_platform_services::{DesktopNotification, NotificationBridge, NoopBackend};

// Input: parsed notification {title, body}, already rate-limited by Core.
let notification = DesktopNotification::new("Build", "finished");

// Bridge: defensive second cap + backend dispatch (fail-closed).
let mut bridge = NotificationBridge::new(Box::new(NoopBackend));
let outcome = bridge.notify_parsed(&notification);
assert!(outcome.is_skipped()); // NoopBackend never touches the OS.
```

Signature overview (see crate rustdoc for the normative form):

- `DesktopNotification::new(title: &str, body: &str) -> Self` — sanitizes
  (strips control characters, collapses whitespace) and bounds length
  (title 128 chars, body 256 chars). Either side may be empty; a fully
  empty notification is well-formed but backends skip it.
- `trait NotificationBackend` — `name()`, `is_available()`,
  `deliver(&DesktopNotification) -> DeliveryOutcome`. Backends never spawn a
  shell; delivery is best-effort and fail-closed.
- `enum DeliveryOutcome` — `Delivered`, `Skipped(SkipReason)`,
  `Failed(String)`. `enum SkipReason` — `BackendMissing`, `Disabled`,
  `RateLimited`.
- `struct NotificationBridge` — owns a `Box<dyn NotificationBackend>` plus a
  fixed-window defensive limiter (default 10 admissions per 1 s, mirroring
  RC-8). `notify(title, body)` and `notify_parsed(&notification)` return
  `Skipped(RateLimited)` over the ceiling instead of queueing without bound.
- Backends: `LinuxDbusBackend` (`org.freedesktop.Notifications`),
  `MacosBackend` (notification center), `WindowsToastBackend` (Toast),
  `NoopBackend` (always skips). `platform_backend()` returns the matching
  backend for the compilation target. Native D-Bus / notification-center /
  WinRT wiring awaits a scoped dependency decision and stays fail-closed
  until then: `is_available()` is false and `deliver()` returns
  `Skipped(BackendMissing)` without touching the OS.

## Status

Initial bridge API at 0.0.1. Backends are fail-closed stubs behind the
trait; no OS notification is emitted yet. Core consumes the bridge for the
defensive second cap (`bitty-runtime` holds a `NotificationBridge` with
`NoopBackend`, CTX-1008 under `bitty#1763`): Core parses OSC 777 and Kitty
notification sequences and enforces the RC-8 rate budget before anything here
runs. Consent wiring, banner composition, and OS delivery stay follow-up work
in `bitty`.

## Gates

`just check` (fmt + clippy `-D warnings` + test + metadata/hygiene/paths).
Rust channel pinned in `rust-toolchain.toml`; MSRV 1.85 (`rust-version` in
the workspace root).
