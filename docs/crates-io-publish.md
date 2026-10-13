# crates.io publish runbook: bitty-platform-services

Priority: P2 | Area: area:packaging | Labels: feat,P2,area:packaging
| Milestone: v0.1.0 | Issue: bitty-platform-services#14
| Upstream: bitty-terminal/bitty#1512

Scope is this repository only. This note records the first publish of
`bitty-platform-services` at version `0.0.1`, so the owner can execute it
after the registry token rotation lands (see the Blockers section).

## Readiness (verified)

- The crate name returns "does not exist" on the crates.io API: free.
- `cargo publish --dry-run -p bitty-platform-services` is green.
- The crate has zero runtime dependencies (fail-closed stubs, no FFI wired),
  so the package surface is minimal and no manifest change is needed.

## Prerequisites

- The registry token rotation recorded upstream is complete. Never publish
  with a token that may have been exposed; each publish is permanent
  (yank is the only undo).
- The working tree is clean and on `main`; the crate is unchanged since the
  dry-run verification above.

## Publish command

Run from the repository root:

```sh
cargo publish -p bitty-platform-services
```

## Verification

- `cargo search bitty-platform-services --limit 1` shows the version once
  the index picks it up.
- The crate page on crates.io shows "Builds on docs.rs" once the docs build
  finishes.
