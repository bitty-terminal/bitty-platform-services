# Backend delivery contract: real Linux/macOS delivery, Windows gap

Priority: P2 | Area: area:platform | Labels: chore,P2,area:platform
| Milestone: v0.1.0 | Task: CTX-0002 | Issue: platform-services#12

Scope is this repository only. No `bitty` edits. This note publishes the
delivery contract the Linux and macOS backends implement, so Core can retire
its second implementation (`OsNotificationSink` in
`crates/bitty-platform/src/notification.rs`) with a mechanical swap and end
the split-brain rate/dispatch paths (audit findings F6/F9).

## Contract

Input is unchanged: an already-gated `{title, body}` pair sanitized on
`DesktopNotification` construction (control characters stripped, whitespace
collapsed, title 128 chars, body 256 chars). The bridge defensive second cap
(10 admissions per 1 s, `Skipped(RateLimited)` over the ceiling) still wraps
every backend.

| Platform                                                   | Helper (absolute path, no `PATH` lookup) | Fixed argv                                                                                                                                                                                                                     |
| ---------------------------------------------------------- | ---------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Linux (`LinuxDbusBackend`, name `"linux-dbus"`)            | `/usr/bin/notify-send`                   | `--app-name=bitty --expire-time=4000 <SUMMARY> <BODY>`; empty title degrades to summary `bitty`                                                                                                                                |
| macOS (`MacosBackend`, name `"macos-notification-center"`) | `/usr/bin/osascript`                     | `-e <script>` with one `display notification` AppleScript expression; backslashes doubled then quotes escaped, so hostile text stays inside string literals; empty body degrades to the title, empty title degrades to `bitty` |
| Windows (`WindowsToastBackend`)                            | none (explicit gap, see below)           | n/a                                                                                                                                                                                                                            |

Timeouts: the spawn never blocks the caller. The child (stdio nulled, never
a shell) is handed to a bounded background reaper thread
(`bitty-platform-services-notify-reap`): 2 s wait (`NOTIFIER_REAP_WAIT`) at
10 ms polls (`NOTIFIER_REAP_POLL`); a hung notifier is killed and reaped, never
left running and never left a zombie. If the reaper thread itself fails to
spawn, the child is killed and reaped inline.

Output handling: child stdout/stderr are nulled, never read, never logged.
Untrusted notification text cannot leak into logs through the delivery path.

Failure mapping:

- Fully empty notification, wrong compilation platform, or absent helper
  binary: `Skipped(BackendMissing)`, no spawn. Availability
  (`is_available`) is a present-tense file probe, never a guarantee:
  `deliver` re-probes immediately before spawning.
- Spawn or reaper-thread failure: `Failed` with the I/O diagnostic.
- Successful reaper handoff: `Delivered` (the backend accepted the spawn;
  the OS may still drop the banner silently, and the exit status is
  unobserved by design).

## Backend choices

- Linux `notify-send`, not a linked D-Bus client: `notify-send` is the inbox
  command-line projection of `org.freedesktop.Notifications`, so real delivery
  needed no new dependency, no `ffi` module, and no audit rows. A native D-Bus
  client stays an explicit non-goal until a scoped task authorizes one. The
  spec constants (`LINUX_DBUS_SERVICE`, object path, interface) remain the
  single home for the well-known names.
- macOS `osascript`, not `terminal-notifier`: inbox on every supported
  release; `terminal-notifier` would add an out-of-tree dependency for no
  contract gain.
- Windows stays `Skipped(BackendMissing)`: no inbox command-line toast path
  exists without a native WinRT dependency, which awaits a scoped dependency
  decision in an isolated `ffi` module with audit rows. Core-side retirement
  keeps the Windows-missing behavior on both sides, so this gap changes
  nothing there.

## Conformance (no real desktop needed in CI)

`cargo test` passes 22 unit tests plus 1 doctest. Delivery is covered against
executable fake helpers (unix): the backend spawns a recording shell script
from a unique temp dir and the suite asserts `Delivered` plus the exact
recorded argv (`linux_delivers_through_fake_backend`,
`macos_delivers_through_fake_backend`,
`hostile_payload_reaches_fake_backend_only_as_literal_argv`), the empty
notification never spawning (`empty_notification_never_spawns`), a missing
program failing closed (`missing_program_is_fail_closed_skip`), and a
non-executable file reporting `Failed` (`unexecutable_file_reports_failed`).
Pure argv/quoting tests run on every platform
(`linux_argv_is_fixed_and_absolute`,
`linux_untitled_notification_names_bitty`,
`osascript_quoting_never_breaks_out`,
`osascript_empty_sides_stay_wellformed`,
`macos_argv_is_fixed_and_absolute`). The suite never spawns the real helpers,
so CI with no session bus stays silent.

## Core retirement follow-up (Core-owned, referenced only)

Retiring Core's `OsNotificationSink` is a Core-side change tracked from #1629:
`bitty-runtime` swaps the installed `notification_sink` arms for these
backends behind the existing `NotificationBridge` (which it already holds),
keeping consent, RC-8, and the defensive second cap untouched. Single
rate/dispatch path afterwards: Core RC-8 plus the bridge cap, one backend
per platform. This repository makes no `bitty` edits.

## Verification

- `just check` green on this head (fmt, clippy `-D warnings`, 22 unit tests
  plus doctest, metadata, hygiene, portable-path gate).
- Independent review required. This change does not merge itself and does not
  approve its own work.
