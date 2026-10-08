# Phase 2 bridge readiness: W-136 notification slice

Priority: P2 | Area: area:platform | Labels: feat,P2,area:platform
| Milestone: v0.1.0 | RFC: W-136 | Task: CTX-0002

Phase 2 accepts the W-136 notification slice covering this bridge API
and records public bridge contract readiness. Scope is this repository
only. Core parsing plus RC-8 stays in `bitty` (do not touch `bitty`).

## Phase 1 verification (bootstrap plus bridge API)

Phase 1 is complete on `main` at `944d065`:

- `just check` passes: `fmt-check`, `clippy` with `-D warnings`,
  `test`, `metadata`, `hygiene`, `paths`.
- Unit tests: 10 passed in
  `crates/bitty-platform-services/src/lib.rs`
  (`sanitize_strips_controls_and_bounds_length`,
  `empty_sides_degrade`,
  `hostile_payload_reaches_backend_only_sanitized`,
  `noop_backend_never_touches_os`,
  `stub_backends_are_fail_closed`,
  `platform_backend_matches_target_and_fails_closed`,
  `bridge_delivers_through_mock_backend`,
  `defensive_cap_admits_burst_then_drops_within_window`,
  `defensive_cap_backwards_clock_never_reopens_window`,
  `defensive_cap_zero_limit_is_fail_closed`).
- Doctest: 1 passed (bridge example with `NoopBackend`).
- MSRV 1.85: `rust-version = "1.85"` in workspace root and crate,
  edition 2024, channel pinned in `rust-toolchain.toml`.
- Safety: `#![forbid(unsafe_code)]`, no `unwrap` or `expect` or
  `panic` in non-test code, typed errors and fail-closed outcomes only.
- Branch protection recorded on `main`.

## W-136 notification slice accepted here

The accepted terminal-side contract is W-136 in
`bitty-terminal-docs` (`specifications/platform-services-contract.md`,
status accepted). This repository serves the notification slice only:

- `DesktopNotification::new(title, body)` sanitizes (strips control
  characters, collapses whitespace) and bounds length (title 128 chars,
  body 256 chars). Either side may be empty; fully empty is well-formed
  but backends skip it.
- `trait NotificationBackend` with `name`, `is_available`, and
  `deliver`. Backends never spawn a shell; delivery is best-effort and
  fail-closed.
- `enum DeliveryOutcome` (`Delivered`, `Skipped(SkipReason)`,
  `Failed(String)`) and `enum SkipReason` (`BackendMissing`,
  `Disabled`, `RateLimited`).
- `struct NotificationBridge` with a fixed-window defensive limiter
  (default 10 admissions per 1 s, mirroring RC-8). Over-ceiling calls
  return `Skipped(RateLimited)` and are counted; nothing queues without
  bound.
- Backends: `LinuxDbusBackend` (`org.freedesktop.Notifications`),
  `MacosBackend`, `WindowsToastBackend`, `NoopBackend`. Native wiring
  stays deferred until a scoped dependency decision; stubs report
  `is_available() == false` and return `Skipped(BackendMissing)`
  without touching the OS.

## Public bridge contract readiness

The bridge API is the public contract Core consumes:

- Input is a parsed notification (`title`, `body`) already
  rate-limited by Core. The bridge applies a defensive second cap
  anyway.
- Core retains permission, consent, redaction, and RC-8 rate bounds
  (ADR-0016 Boundary 5). This crate executes only what Core
  authorizes.
- No shell construction or interpolation anywhere in the delivery path.
- Notification payloads are untrusted observation data: control
  characters stripped, text length-bounded, never expanded, executed,
  or interpreted as paths or commands.

## Cross-repo prerequisites

- ADR-0016 Boundary 5: accepted (platform-service adapter boundary;
  Core retains permission gate, validated args, redaction and rate
  bounds).
- W-136: accepted in `bitty-terminal-docs`.
- `bitty#1763` (OQ-076): Core OSC 777 and Kitty parsing plus RC-8
  limiter lives in `bitty`. Phase 2 does not require Core integration
  to land; tracking belongs to Phase 3.

## Narrow file scope

This change adds only this readiness note. No product code changes.
No `bitty` edits. Native D-Bus, notification-center, and WinRT wiring
stay deferred until a scoped dependency decision.

## Verification

- `just check` green on this head (fmt, clippy `-D warnings`, 10 unit
  tests plus doctest, metadata, hygiene, portable-path gate).
- Independent review required before Phase 2 acceptance is recorded.
  This PR does not merge itself and does not approve its own work.

## Open points

- Native backend wiring (D-Bus client, notification-center, WinRT)
  awaits a scoped dependency decision with an isolated `ffi` module
  and audit rows if it lands.
- Consent wiring, banner composition, and OS delivery in Core stay
  follow-up work in `bitty`.
