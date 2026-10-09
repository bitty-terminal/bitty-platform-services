# Phase 4 verification: bridge security and Core composition

Priority: P2 | Area: area:platform | Labels: feat,P2,area:platform
| Milestone: v0.1.0 | RFC: W-136 | Task: CTX-0004

Phase 4 independently verifies bridge security and Core composition.
Scope is this repository only. Verification requires a different
reviewer (no self-acceptance), negative-path evidence, canonical docs
synchronization for W-136, no weakening of Core invariants, and green
CI on the verification head. This PR is the review vehicle; it does
not approve itself and does not merge itself.

## Negative-path evidence (all passing on this head)

`cargo test` passes 10 unit tests plus 1 doctest. Negative paths:

- Unavailable backends stay `BackendMissing`:
  `stub_backends_are_fail_closed` asserts all four stubs report
  `is_available() == false` and `deliver()` returns
  `Skipped(BackendMissing)`; `platform_backend_matches_target_and_fails_closed`
  asserts the target-selected backend fails closed;
  `noop_backend_never_touches_os` asserts the no-op path.
- Hostile payloads stay sanitized:
  `sanitize_strips_controls_and_bounds_length` asserts control
  stripping, whitespace collapse, and title 128 and body 256 bounds;
  `hostile_payload_reaches_backend_only_sanitized` delivers shell
  metacharacters, command substitution, and escape sequences through a
  recording double and asserts only the sanitized form arrives.
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
  `panic` in non-test code (the single `unwrap_or` in the file is
  inside the test module). Typed errors and fail-closed outcomes only.
- No shell construction or interpolation anywhere in the delivery path:
  no `std::process`, no `Command`, no shell string in `src`. Backends
  are fixed-trait dispatches returning typed outcomes.
- Payloads are untrusted observation data: stripped, collapsed, and
  bounded on construction; never expanded, executed, or interpreted as
  paths or commands.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
  passes with zero warnings. `cargo fmt --all -- --check` passes.

## Core composition (no weakening of Core invariants)

- Core retains permission, consent, redaction, and RC-8 rate bounds
  (ADR-0016 Boundary 5). This crate executes only what Core
  authorizes: input is an already-gated `{title, body}` pair and the
  bridge applies a defensive second cap anyway.
- `bitty#1763` closed via merged `bitty#1773` (CTX-1008): Core parses
  OSC 777 and Kitty OSC 99, enforces RC-8 (10 per s, queue depth 8),
  and holds a `NotificationBridge` with `NoopBackend` as a cap-only
  gate. This crate never grants permission, never bypasses consent,
  and never replaces Core redaction or rate bounds.
- Platform FFI is not wired: backends stay fail-closed stubs until a
  scoped task authorizes a native dependency in an isolated `ffi`
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
