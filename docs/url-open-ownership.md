# URL-open ownership decision: Core keeps it permanently

Priority: P2 | Area: area:platform | Labels: chore,P2,area:platform
| Milestone: v0.1.0 | Task: CTX-0002 | Issue: platform-services#12

Decision (recorded, no code either way): URL opening stays owned by Core
(`bitty`) permanently. This repository does not gain URL-open code, and no
URL-open implementation was added here.

## Reasoning

1. Validation and dispatch are co-located in Core today and must stay that
   way. `validate_url` / `validate_file_url` (scheme allowlist, traversal and
   authority rejection) live in `crates/bitty-platform/src/url.rs`, and the
   spawn site `Runtime::spawn_validated_url` plus the `UrlOpener` /
   `SystemUrlOpener` seam live in `bitty-runtime` (`runtime/plugin.rs`).
   Moving only the spawn half here would split validation from dispatch and
   duplicate the `ValidatedUrl` / `PlatformError` vocabulary across repos.
2. This repository serves the W-136 notification slice only (accepted
   contract; see `docs/phase-2-bridge-readiness.md`). URL opening belongs to
   a different owning contract (Core documents the W-145 reduction in
   `crates/bitty-platform/src/url.rs`); giving this repo a second contract
   owner conflates two failure modes.
3. Activation authority lives in Core. Detection lives in
   `bitty-url-detector`, but the gesture-plus-scheme gate and the
   `UrlActivation` / `FileUrlActivation` capabilities that authorize a launch
   are issued by `bitty-runtime` (CTX-0577). This repo has no activation or
   consent vocabulary and must not gain one: Core retains the permission gate
   (ADR-0016 Boundary 5).
4. The threat models differ. Notifications are untrusted display text:
   sanitize, bound, and show. URL opening launches an external handler for a
   validated URI: allowlist, then spawn. One repo owning both would merge a
   display-text pipeline with a handler-launch pipeline under one rate/limit
   story that does not fit both.
5. No duplication exists to fix. Unlike notifications (two rate/dispatch
   paths, split-brain), URL opening has exactly one path: detection in
   `bitty-url-detector`, validation plus dispatch in Core. There is nothing
   to consolidate.

## Consequences

- The canonical URL-open sites stay in Core (read-only references, no edits
  here): `bitty_platform::validate_url` / `validate_file_url`,
  `Runtime::open_url` / `open_file_url`, `Runtime::spawn_validated_url`,
  `SystemUrlOpener`.
- This repository stays notification-slice only: `DesktopNotification` /
  `NotificationBackend` / `NotificationBridge` plus the per-platform delivery
  backends in `docs/backend-delivery-contract.md`.
- Any future URL-open work (handler changes, scheme policy) belongs to Core
  issues, never to this repository.

## Verification

- `just check` green on this head. No product-code URL surface was added:
  `grep` for `UrlOpener` / `spawn_validated_url` / `validate_url` finds no
  hits in `crates/`.
- Independent review required. This change does not merge itself and does not
  approve its own work.
