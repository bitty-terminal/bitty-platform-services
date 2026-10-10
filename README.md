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
- Backends: `LinuxDbusBackend` (`org.freedesktop.Notifications` via inbox
  `notify-send`), `MacosBackend` (notification center via inbox `osascript`),
  `WindowsToastBackend` (Toast; still fail-closed: no inbox command-line toast
  path without a native WinRT dependency), `NoopBackend` (always skips).
  `platform_backend()` returns the matching backend for the compilation
  target. Linux and macOS delivery spawns a fixed argv (never a shell) with
  stdio nulled and hands the child to a bounded background reaper (2 s wait at
  10 ms polls), so the caller never blocks; the full argv/timeout/output and
  failure contract lives in `docs/backend-delivery-contract.md`. Absent
  helpers stay fail-closed (`Skipped(BackendMissing)`).

## Status

Bridge API plus real Linux/macOS delivery at 0.0.1. Linux delivers through
inbox `notify-send` and macOS through inbox `osascript` (fixed argv, bounded
reaper, no new dependencies); Windows stays fail-closed until a scoped
dependency decision wires WinRT. Core consumes the bridge for the defensive
second cap (`bitty-runtime` holds a `NotificationBridge` with `NoopBackend`,
CTX-1008 under `bitty#1763`): Core parses OSC 777 and Kitty notification
sequences and enforces the RC-8 rate budget before anything here runs.
Retiring Core's second implementation (`OsNotificationSink`) is a Core-side
follow-up tracked from platform-services#12; URL opening stays Core-owned
permanently (`docs/url-open-ownership.md`). Consent wiring and banner
composition stay follow-up work in `bitty`.

## Gates

`just check` (fmt + clippy `-D warnings` + test + metadata/hygiene/paths).
Rust channel pinned in `rust-toolchain.toml`; MSRV 1.85 (`rust-version` in
the workspace root).
