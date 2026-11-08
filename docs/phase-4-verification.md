# Phase 4 verification: bridge security and Core composition

Priority: P2 | Area: area:platform | Labels: feat,P2,area:platform
| Milestone: v0.1.0 | RFC: W-136 | Task: CTX-0003

Phase 4 independently verifies bridge security and Core composition
across the real Linux/macOS notification backends landed in PR #13
(CTX-0002) and consumed by Bitty Core (`bitty#1998`, CTX-1178, pin
`7c6b84b`).

## Negative-path evidence (all passing on this head)

`cargo test` passes 24 unit tests plus 1 doctest. Negative paths:

- Unavailable or stub backends stay `BackendMissing`:
  `noop_backend_never_touches_os` and `windows_backend_stays_fail_closed`
  assert `is_available() == false` and `deliver()` returns
  `Skipped(BackendMissing)`; `missing_program_is_fail_closed_skip` and
  `linux_and_macos_backends_fail_closed_without_helper` verify that
  missing `/usr/bin/notify-send` or `/usr/bin/osascript` binaries fail
  closed without spawning; `unexecutable_file_reports_failed` verifies
  spawn failure returns `DeliveryOutcome::Failed(_)`.
- Empty notifications never spawn and never consume rate budget:
  `empty_notification_never_spawns` and
  `empty_notification_does_not_consume_rate_budget` verify that
  notifications whose title and body are empty (or sanitize to empty)
  return `Skipped(BackendMissing)` before touching the rate limiter or OS.
- Hostile payloads stay sanitized and cannot inject options or shell commands:
  `sanitize_strips_controls_and_bounds_length` and
  `sanitize_strips_bidi_overrides_and_collapses_before_truncation` assert
  `Cc` control and Unicode BiDi override stripping, whitespace collapse
  prior to truncation, and `128`/`256` character bounds;
  `linux_argv_is_fixed_and_absolute` and
  `hostile_payload_reaches_fake_backend_only_as_literal_argv` verify
  `LINUX_NOTIFY_END_OPTIONS` (`"--"`) precedes positional summary/body
  arguments so leading-dash payloads (`--wait`, `--icon=...`) and shell
  metacharacters (`$(id)`, `` `id` ``) arrive only as literal positional
  strings; `osascript_quoting_never_breaks_out` verifies backslash and
  quote escaping inside `osascript -e`.
- Defensive cap drops over-ceiling:
  `defensive_cap_admits_burst_then_drops_within_window` admits 10 then
  drops the 11th as `Skipped(RateLimited)` with `rate_dropped == 1`
  and re-admits next window;
  `defensive_cap_backwards_clock_never_reopens_window` asserts a
  backwards clock cannot re-open the window early;
  `defensive_cap_zero_limit_is_fail_closed` asserts a zero limit
  admits nothing.

## Security properties verified by inspection plus gates

- `#![forbid(unsafe_code)]` in the crate. No `unwrap`, `expect`, or
  `panic` in non-test code. Typed errors and fail-closed outcomes only.
- No shell construction or interpolation anywhere in the delivery path:
  `spawn_notifier` invokes `/usr/bin/notify-send` or
  `/usr/bin/osascript` directly via `std::process::Command::new` with a
  fixed argv (`--` end-of-options on Linux; quoted string literals on
  macOS), `Stdio::null()` for stdin/stdout/stderr, and a bounded 2 s
  background reaper (`NOTIFIER_REAP_WAIT`).
- Payloads are untrusted observation data: stripped of control and BiDi
  formatting characters, whitespace-collapsed, and bounded on
  construction; never expanded, executed, or logged.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
  passes with zero warnings. `cargo fmt --all -- --check` passes.

## Core composition (no weakening of Core invariants)

- Core retains permission, consent, redaction, and RC-8 rate bounds
  (ADR-0016 Boundary 5). This crate executes only what Core
  authorizes: input is an already-gated `{title, body}` pair and the
  bridge applies a defensive second cap anyway.
- `bitty#1763` closed via merged `bitty#1773` (CTX-1008) and `bitty#1998`
  (CTX-1178) retired Core's duplicate `OsNotificationSink` onto
  `NotificationBridge::install_platform_notification_backend()`. Core
  never grants permission here, never bypasses consent, and never
  replaces Core redaction or rate bounds.
- Windows Toast remains a fail-closed stub (`WindowsToastBackend`) until a
  scoped task authorizes a native WinRT dependency in an isolated `ffi`
  module with audit rows.

## Canonical docs synchronization (W-136)

- W-136 (`specifications/platform-services-contract.md` in
  `bitty-terminal-docs`) is accepted and covers the notification
  intake, redaction, rate bounds, permission and consent gate,
  display handoff, and failure behavior this bridge implements.
- This repository serves the notification slice only; URL opening and
  blur stay with their owning contracts. No `bitty-terminal-docs`
  edits are made from this repository (scope is this repository
  only); synchronization here means the slice matches the accepted
  contract and any docs-repo follow-ups belong to that repository's
  owning task.

## Narrow file scope

This change adds only this verification note. No product code changes.
No `bitty` or `bitty-terminal-docs` edits.

## Verification performed on this head

- `just check` green: fmt, clippy `-D warnings`, 10 unit tests plus
  doctest, metadata, hygiene, portable-path gate.
- Green CI required on the verification head before acceptance. A
  different reviewer must approve; the author does not self-approve
  and does not merge.

## Open points

- Native backend wiring awaits a scoped dependency decision.
- Consent wiring, banner composition, and OS delivery refinements in
  Core stay in `bitty`.
