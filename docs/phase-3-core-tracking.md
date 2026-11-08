# Phase 3 Core tracking: consent plus RC-8 wired to bridge API

Priority: P2 | Area: area:platform | Labels: feat,P2,area:platform
| Milestone: v0.1.0 | RFC: W-136 | Task: CTX-0003

Phase 3 tracks Bitty Core integration against this bridge API. Scope is
tracking only. Implementation lives in `bitty` under `bitty#1763`; this
issue authorizes no Core edits from this repository. No `bitty` files
are touched here.

## Consumer-side acceptance criteria (from #3)

- Consented notifications admitted by Core RC-8 reach
  `NotificationBridge::notify_parsed`.
- Denied and over-ceiling events never call it.
- No shell construction anywhere in the path.

## Core integration landed (tracking evidence)

`bitty#1763` is CLOSED via merged `bitty#1773` (CTX-1008,
merge `1d6bae3`): Kitty OSC 99 parsing plus RC-8 plus bridge cap.
Subsequently, `bitty#1998` (CTX-1178) retired Core's duplicate
`OsNotificationSink` onto the real `LinuxDbusBackend` and `MacosBackend`
implementations shipped in `bitty-platform-services#13` (CTX-0002),
pinning `bitty-platform-services` at `7c6b84b` in
`crates/bitty-runtime/Cargo.toml`.

- Core parses OSC 777 notify plus Kitty OSC 99 sequences into
  title and body (chunk parser plus bounded assembler, base64 `e=1`;
  queries and close stay inert).
- RC-8 rate limiter (bounded frequency 10 per s plus queue depth 8,
  fail-closed on flood) shared across OSC 9, 777, and 99 plus the
  bridge defensive second cap.
- Bell presentation (visual flash plus `terminal.bell`
  audible and visual via `BellSink`; no sink means counted-only).
- Core consumes `bitty-platform-services` as a dependency at exact-rev
  pin `7c6b84b`, recorded in `crates/bitty-runtime/Cargo.toml` and
  `Cargo.lock`.
- Headed runs install `platform_backend()` via
  `Runtime::install_platform_notification_backend()`, while headless and
  test runs keep `NoopBackend` by default so CI never touches the OS.
- No shell interpolation anywhere (fixed argv with `--` option delimiter
  on Linux `notify-send` and quoted AppleScript literal on macOS
  `osascript`).

## How Core calls the bridge (read-only evidence)

Read from the `bitty` checkout (no edits here):

- `crates/bitty-runtime/src/runtime.rs`: `notification_bridge` held as
  `bitty_platform_services::NotificationBridge` (defaulting to
  `NoopBackend`, upgraded to `platform_backend()` in real headed runs via
  `install_platform_notification_backend`); admission builds
  `DesktopNotification::new(title, body)`, skips empty notifications, and
  calls `notify_parsed_at(&bridge_notification, now)`. Over-ceiling maps
  to `Skipped(RateLimited)`.
- `crates/bitty-runtime/Cargo.toml`: `bitty-platform-services` at
  `rev = "7c6b84b"`.
- `crates/bitty-runtime/tests/m1_kitty_notification.rs`:
  consent, assembly, base64, rapid RC-8, mixed budget, hostile
  sanitization, denied no-buffer. Existing `m1_bell_notification` and
  `m1_bell_os_delivery` cover the bell and OS delivery paths.
- Unit coverage for the parser lives in `bitty-vt` (OSC 99
  title, body, chunked, base64, inert, ST handling; assembler single,
  chunked, empty, evict, bound).

## What this repository owns

This repository defines the API Core consumes, applies the defensive
second cap, sanitizes `DesktopNotification` fields (stripping control and
Unicode BiDi override characters, collapsing whitespace before enforcing
`128`/`256` character bounds), and owns the per-platform delivery
backends (`LinuxDbusBackend` via `/usr/bin/notify-send` with `--`
end-of-options protection, `MacosBackend` via `/usr/bin/osascript` with
escaped string literals, `WindowsToastBackend` fail-closed stub, and
`NoopBackend`).

## Narrow file scope

This change adds only this tracking note. No product code changes. No
Core edits from this repository. No `bitty` checkout is modified.

## Verification

- `just check` green on this head.
- Pin and consumer call sites verified read-only against the `bitty`
  checkout and the merged Core PR. No cross-repo writes performed.
- Independent review required. This PR does not merge itself and does
  not approve its own work.

## Open points

- Consent wiring, banner composition refinements, and OS delivery in
  Core stay follow-up work in `bitty`.
- Native backend wiring here stays deferred until a scoped dependency
  decision.
