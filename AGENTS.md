# bitty-platform-services AGENTS.md

L1 Rust Core Extension: platform notification bridge (`bitty-platform-services`).
Default-off; the `bitty` binary stays notification-free until Core wires this
crate behind its consent gate. This repository DEFINES the API that Bitty Core
consumes; Core OSC 777 / Kitty parsing plus the RC-8 limiter is a separate
task in `bitty` — do not touch `bitty` from here.

## Rules

- English only for all content (code comments, docs, commits, Issues, PRs).
- Rust edition 2024, MSRV 1.85 (no let-chains; desugar to nested `if let`).
  Channel pinned in `rust-toolchain.toml`; never bump pins in unrelated tasks.
- Quality gates only via the justfile: `just check`
  (fmt + `clippy -D warnings` + tests + metadata/hygiene/paths).
  Never invoke formatters/linters bare.
- `#![forbid(unsafe_code)]` in every crate. No `unwrap`/`expect`/`panic` in
  non-test code; typed errors and fail-closed outcomes only. Platform FFI is
  not wired: backends stay fail-closed stubs until a scoped task authorizes a
  native dependency, isolated in an `ffi` module with audit rows if it lands.
- Never hardcode host/environment values (paths, usernames, URLs, ports).
  Constants for policy/bound/timeout/default values, never magic literals.
  Spec-defined identifiers (D-Bus service names) live in exactly one place.
- Core retains permission, consent, redaction, and RC-8 rate bounds (ADR-0016
  Boundary 5). This crate executes only what Core authorizes: input is a
  parsed notification (`title`, `body`) already rate-limited by Core, and the
  bridge applies a defensive second cap anyway (excess returns
  `Skipped(RateLimited)`, never queues without bound).
- No shell construction or interpolation anywhere in the delivery path
  (`P0-AC-009`): fixed argv or native API calls only, never a shell string.
  Backends are fail-closed (`Skipped(BackendMissing)`) when unavailable.
- Notification payloads are untrusted observation data: control characters
  are stripped, text is length-bounded, and payloads are never expanded,
  executed, or interpreted as paths or commands.
- Ephemeral scratch under `/tmp/bitty/`; durable material under repo-local
  gitignored `recording/`. Disk hygiene: remove task target dirs on close.
- Implementation goes to scoped subagents with independent review; no
  self-review. No commit/push/merge without explicit task authorization.
