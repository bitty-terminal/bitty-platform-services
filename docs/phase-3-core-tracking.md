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

- Core parses OSC 777 notify plus Kitty OSC 99 sequences into
  title and body (chunk parser plus bounded assembler, base64 `e=1`;
  queries and close stay inert).
- RC-8 rate limiter (bounded frequency 10 per s plus queue depth 8,
  fail-closed on flood) shared across OSC 9, 777, and 99 plus the
  bridge defensive second cap.
- Bell presentation (visual flash plus `terminal.bell`
  audible and visual via `BellSink`; no sink means counted-only).
- Core consumes `bitty-platform-services` as a dependency at exact-rev
  pin `2fc794a` (same pattern as `bitty-network-wire`), recorded in
  `crates/bitty-runtime/Cargo.toml` and `Cargo.lock`.
- No shell interpolation anywhere (fixed argv only).

The pinned revision `2fc794a` is the bootstrap commit of this
repository. The bridge API is unchanged since that pin: later commits
here are docs-only (`22e13c4`) and toolchain (`944d065`). The pin
therefore still resolves to the accepted bridge surface.

## How Core calls the bridge (read-only evidence)

Read from the `bitty` checkout (no edits here):

- `crates/bitty-runtime/src/runtime.rs`: `notification_bridge` held as
  `bitty_platform_services::NotificationBridge` behind a cap-only
  `NoopBackend`; admission builds
  `DesktopNotification::new(title, body)` and over-ceiling maps to
  `Skipped(RateLimited)`.
- `crates/bitty-runtime/Cargo.toml`: `bitty-platform-services` at
  `rev = "2fc794a"`.
- `crates/bitty-runtime/tests/m1_kitty_notification.rs` (257 lines):
  consent, assembly, base64, rapid RC-8, mixed budget, hostile
  sanitization, denied no-buffer. Existing `m1_bell_notification` and
  `m1_bell_os_delivery` cover the bell path.
- Unit coverage for the parser lives in `bitty-vt` (OSC 99
  title, body, chunked, base64, inert, ST handling; assembler single,
  chunked, empty, evict, bound).

## What this repository owns

This repository defines the API Core consumes and applies the
defensive second cap. It does not parse terminal output and does not
enforce the primary RC-8 budget; Core does that before anything here
runs. Backends stay fail-closed stubs until a scoped task authorizes a
native dependency.

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
