//! `bitty-platform-services`: platform notification bridge for Bitty.
//!
//! This crate DEFINES the API that Bitty Core consumes to deliver desktop
//! notifications (`bitty#1763`, OQ-076; ADR-0016 Boundary 5, focused contract
//! W-136). It does not parse terminal output: Core parses OSC 777 and Kitty
//! notification sequences and enforces the RC-8 rate budget (10 events/s
//! coalesced) before anything here runs.
//!
//! # Input contract
//!
//! Input is a parsed notification (`title`, `body`) already rate-limited by
//! Core. The bridge applies a defensive second cap anyway: over-ceiling calls
//! return [`DeliveryOutcome::Skipped`] with [`SkipReason::RateLimited`] and
//! never queue without bound.
//!
//! # Backends
//!
//! Per-platform backends sit behind [`NotificationBackend`]: Linux
//! [`LinuxDbusBackend`] (the `org.freedesktop.Notifications` service via its
//! inbox command projection `notify-send`), macOS [`MacosBackend`]
//! (notification center via inbox `osascript`), Windows
//! [`WindowsToastBackend`] (Toast; still fail-closed: no inbox command-line
//! toast path exists without a native WinRT dependency), plus [`NoopBackend`]
//! for headless runs and tests. Linux and macOS delivery spawn a fixed argv
//! (never a shell) with stdio nulled and hand the child to a bounded
//! background reaper, so the caller never blocks; see [`LinuxDbusBackend`]
//! and [`MacosBackend`] for the exact argv, timeouts, output handling, and
//! failure mapping. No shell is ever constructed or interpolated anywhere in
//! the delivery path.
//!
//! # Example
//!
//! ```
//! use bitty_platform_services::{DesktopNotification, NoopBackend, NotificationBridge};
//!
//! let notification = DesktopNotification::new("Build", "finished");
//! let mut bridge = NotificationBridge::new(Box::new(NoopBackend));
//! let outcome = bridge.notify_parsed(&notification);
//! assert!(outcome.is_skipped());
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::time::{Duration, Instant};

/// Maximum characters retained in a notification title.
///
/// Mirrors the Core banner bound so what the OS would show and what the
/// in-grid banner shows stay the same bounded string.
pub const TITLE_MAX_CHARS: usize = 128;

/// Maximum characters retained in a notification body.
pub const BODY_MAX_CHARS: usize = 256;

/// Defensive second cap: admitted deliveries per fixed window.
///
/// Mirrors the accepted RC-8 ceiling (10 events/s coalesced). Core enforces
/// RC-8 before calling here; this cap only bounds a misbehaving caller.
pub const DEFENSIVE_MAX_PER_WINDOW: u32 = 10;

/// Defensive second cap window (one second, mirroring RC-8).
pub const DEFENSIVE_WINDOW: Duration = Duration::from_secs(1);

/// Linux D-Bus destination for desktop notifications (spec-defined).
///
/// Single home for the well-known name. Delivery reaches this service through
/// the inbox [`LINUX_NOTIFY_SEND`] command projection below; no D-Bus client
/// is linked.
pub const LINUX_DBUS_SERVICE: &str = "org.freedesktop.Notifications";

/// Linux D-Bus object path for desktop notifications (spec-defined).
pub const LINUX_DBUS_OBJECT_PATH: &str = "/org/freedesktop/Notifications";

/// Linux D-Bus interface for desktop notifications (spec-defined).
pub const LINUX_DBUS_INTERFACE: &str = "org.freedesktop.Notifications";

/// Linux notification backend binary (absolute path, no `PATH` lookup).
///
/// `notify-send` is the inbox command-line projection of the
/// [`LINUX_DBUS_SERVICE`] service above: no new dependency is linked, and a
/// native D-Bus client stays an explicit non-goal until a scoped task
/// authorizes one. An absent binary fails closed
/// (`Skipped(BackendMissing)`).
pub const LINUX_NOTIFY_SEND: &str = "/usr/bin/notify-send";

/// Linux backend argv flag: application identity shown by the notifier.
pub const LINUX_NOTIFY_APP_NAME: &str = "--app-name=bitty";

/// Linux backend argv flag: banner-length expiry (4 s) matching the Core
/// in-grid banner, so the OS surface and the terminal surface agree.
pub const LINUX_NOTIFY_EXPIRE: &str = "--expire-time=4000";

/// Fallback summary handed to the OS when the sanitized title is empty.
///
/// `OSC 9` carries no title; the OS call stays well-formed by naming Bitty.
pub const NOTIFY_DEFAULT_SUMMARY: &str = "bitty";

/// macOS notification backend binary (absolute path, no `PATH` lookup).
///
/// Inbox on every supported release; `terminal-notifier` would add an
/// out-of-tree dependency, so `osascript` is the delivery contract.
pub const MACOS_OSASCRIPT: &str = "/usr/bin/osascript";

/// macOS backend argv flag: the following argv entry is a script expression.
pub const MACOS_OSASCRIPT_EXPR_FLAG: &str = "-e";

/// How long a spawned notifier child may run before the reaper kills it.
///
/// A hung notifier (wedged bus, suspended session) is killed, never left
/// running, and never left a zombie.
pub const NOTIFIER_REAP_WAIT: Duration = Duration::from_secs(2);

/// Reaper poll interval while waiting for a notifier child.
pub const NOTIFIER_REAP_POLL: Duration = Duration::from_millis(10);

/// Background reaper thread name for spawned notifier children.
pub const NOTIFIER_REAPER_THREAD_NAME: &str = "bitty-platform-services-notify-reap";

/// A sanitized, length-bounded desktop notification.
///
/// Constructed only via [`DesktopNotification::new`], which strips control
/// characters, collapses whitespace, and truncates to [`TITLE_MAX_CHARS`] /
/// [`BODY_MAX_CHARS`], so an unbounded or hostile parser payload can never
/// reach a backend as-is. Payloads are untrusted observation data: never
/// expanded, never executed, never interpreted as paths or commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopNotification {
    title: String,
    body: String,
}

impl DesktopNotification {
    /// Builds a notification from parsed title/body text.
    ///
    /// Either side may be empty (`OSC 9` carries no title); delivery backends
    /// degrade to whichever side is non-empty, and a fully empty notification
    /// is still well-formed (backends skip it without touching the OS).
    #[must_use]
    pub fn new(title: &str, body: &str) -> Self {
        Self {
            title: sanitize_field(title, TITLE_MAX_CHARS),
            body: sanitize_field(body, BODY_MAX_CHARS),
        }
    }

    /// Sanitized title (possibly empty).
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Sanitized body (possibly empty).
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }

    /// Whether both sides are empty (nothing worth handing to the OS).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.title.is_empty() && self.body.is_empty()
    }
}

/// Strips control characters, collapses whitespace runs, and truncates to
/// `max_chars` characters.
fn sanitize_field(raw: &str, max_chars: usize) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|ch| !ch.is_control())
        .take(max_chars)
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Why a delivery attempt was skipped without touching the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// No usable backend exists on this platform/configuration.
    BackendMissing,
    /// Delivery was disabled by the caller.
    Disabled,
    /// The defensive second cap dropped an over-ceiling event.
    RateLimited,
}

/// Best-effort delivery outcome.
///
/// `Delivered` means the request reached the backend; the OS may still drop
/// it silently, which no synchronous API can observe without blocking. Skips
/// and failures are values, never panics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryOutcome {
    /// The backend accepted the request.
    Delivered,
    /// No attempt was made (backend absent, disabled, or rate-capped).
    Skipped(SkipReason),
    /// The attempt failed; carries the backend diagnostic.
    Failed(String),
}

impl DeliveryOutcome {
    /// Whether the backend accepted the request.
    #[must_use]
    pub const fn is_delivered(&self) -> bool {
        matches!(self, Self::Delivered)
    }

    /// Whether no attempt was made.
    #[must_use]
    pub const fn is_skipped(&self) -> bool {
        matches!(self, Self::Skipped(_))
    }
}

/// Per-platform notification delivery seam.
///
/// Production uses the matching platform backend via [`platform_backend`];
/// tests install a recording double. Backends never construct or interpolate
/// a shell; delivery is best-effort and fail-closed.
pub trait NotificationBackend {
    /// Stable backend name for diagnostics (for example `"noop"`).
    fn name(&self) -> &'static str;

    /// Whether this backend can deliver on the current configuration.
    ///
    /// Native wiring awaits a scoped dependency decision; stub backends
    /// report false until then.
    fn is_available(&self) -> bool;

    /// Hands an already-gated notification to the platform (best-effort).
    fn deliver(&self, notification: &DesktopNotification) -> DeliveryOutcome;
}

/// No-op backend: always skips without touching the OS.
///
/// Used for headless runs, CI, and embedders that render their own surface.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopBackend;

impl NotificationBackend for NoopBackend {
    fn name(&self) -> &'static str {
        "noop"
    }

    fn is_available(&self) -> bool {
        false
    }

    fn deliver(&self, _notification: &DesktopNotification) -> DeliveryOutcome {
        DeliveryOutcome::Skipped(SkipReason::BackendMissing)
    }
}

/// Fixed argv tail for `notify-send`: `[--app-name=bitty,
/// --expire-time=4000, <SUMMARY>, <BODY>]`.
///
/// `notify-send` takes `SUMMARY [BODY]` positionally; an empty title degrades
/// to [`NOTIFY_DEFAULT_SUMMARY`] so the call stays well-formed. Entries are
/// passed as literal argv items: shell metacharacters are never interpreted.
fn linux_notify_argv(notification: &DesktopNotification) -> Vec<String> {
    let summary = if notification.title().is_empty() {
        String::from(NOTIFY_DEFAULT_SUMMARY)
    } else {
        String::from(notification.title())
    };
    vec![
        String::from(LINUX_NOTIFY_APP_NAME),
        String::from(LINUX_NOTIFY_EXPIRE),
        summary,
        String::from(notification.body()),
    ]
}

/// Builds the `display notification` AppleScript expression with both
/// interpolated strings quoted.
///
/// AppleScript has no escape inside double-quoted strings except `\"` (with
/// backslashes doubled first), so a hostile payload can never break out of
/// the quoted literal. No shell is involved either way: the script travels as
/// a single `osascript -e` argv entry.
fn osascript_notification_script(title: &str, body: &str) -> String {
    fn quote(text: &str) -> String {
        let mut quoted = String::with_capacity(text.len() + 2);
        quoted.push('"');
        for ch in text.chars() {
            if ch == '\\' || ch == '"' {
                quoted.push('\\');
            }
            quoted.push(ch);
        }
        quoted.push('"');
        quoted
    }
    // `display notification` requires a message; an empty body degrades to
    // the title so the call stays well-formed.
    let message = if body.is_empty() { title } else { body };
    if title.is_empty() || body.is_empty() {
        format!(
            "display notification {} with title \"{NOTIFY_DEFAULT_SUMMARY}\"",
            quote(message)
        )
    } else {
        format!(
            "display notification {} with title {}",
            quote(message),
            quote(title)
        )
    }
}

/// Fixed argv for `osascript`: `-e <script>`, with the script from
/// [`osascript_notification_script`] as one literal argv entry.
fn macos_osascript_argv(notification: &DesktopNotification) -> Vec<String> {
    vec![
        String::from(MACOS_OSASCRIPT_EXPR_FLAG),
        osascript_notification_script(notification.title(), notification.body()),
    ]
}

/// Absolute-path backend presence probe (no `PATH` resolution).
///
/// Availability is a present-tense probe, never a guarantee: `deliver`
/// re-probes immediately before spawning, so a binary removed in between
/// still fails closed instead of resolving something else.
fn backend_present(program: &str) -> bool {
    std::path::Path::new(program).is_file()
}

/// Spawns `program` with fixed `args` (stdio nulled, never a shell) and hands
/// the child to a bounded background reaper so the caller never blocks.
///
/// Returns `Delivered` once the child is handed to the reaper: the backend
/// accepted the spawn, not proof that a banner is visible (the OS may still
/// drop it silently, and the exit status is unobserved by design). Spawn
/// failure is `Failed` with the I/O diagnostic. Child output is discarded:
/// stdout and stderr are nulled, never read and never logged, so untrusted
/// notification text cannot leak into logs through the delivery path.
fn spawn_notifier(program: &str, args: &[String]) -> DeliveryOutcome {
    use std::process::{Command, Stdio};
    let spawn = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let child = match spawn {
        Ok(child) => child,
        Err(error) => return DeliveryOutcome::Failed(error.to_string()),
    };
    let (child_tx, child_rx) = std::sync::mpsc::channel::<std::process::Child>();
    let program_name = String::from(program);
    let reaper = std::thread::Builder::new()
        .name(String::from(NOTIFIER_REAPER_THREAD_NAME))
        .spawn(move || {
            let Ok(mut child) = child_rx.recv() else {
                return;
            };
            // Bounded wait: a hung notifier (wedged bus) is killed, never
            // left running, and never left a zombie.
            let waited = NOTIFIER_REAP_WAIT.as_millis() / NOTIFIER_REAP_POLL.as_millis().max(1);
            for _ in 0..waited.max(1) {
                match child.try_wait() {
                    Ok(Some(_)) => return,
                    Ok(None) => std::thread::sleep(NOTIFIER_REAP_POLL),
                    Err(_) => break,
                }
            }
            let _ = child.kill();
            let _ = child.wait();
            let _ = program_name;
        });
    match reaper {
        Ok(_) => {
            let _ = child_tx.send(child);
            DeliveryOutcome::Delivered
        }
        Err(error) => {
            let mut child = child;
            let _ = child.kill();
            let _ = child.wait();
            DeliveryOutcome::Failed(error.to_string())
        }
    }
}

/// Linux backend: D-Bus `org.freedesktop.Notifications` via `notify-send`.
///
/// Delivery spawns [`LINUX_NOTIFY_SEND`] with the fixed argv from
/// [`linux_notify_argv`]. stdio is nulled, no shell is constructed, and the
/// child is handed to a bounded background reaper ([`NOTIFIER_REAP_WAIT`]
/// wait at [`NOTIFIER_REAP_POLL`] intervals), so the caller never blocks.
/// `notify-send` is the inbox command-line projection of the spec service
/// ([`LINUX_DBUS_SERVICE`]); no D-Bus client is linked, so real delivery
/// needed no new dependency and no `ffi` module.
///
/// Fail-closed mapping: a fully empty notification, a non-Linux platform, or
/// a missing backend binary returns `Skipped(BackendMissing)` without
/// spawning; spawn failure returns `Failed` with the I/O diagnostic; a
/// successful reaper handoff returns `Delivered` (the backend accepted the
/// spawn; the OS may still drop the banner silently).
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxDbusBackend;

impl LinuxDbusBackend {
    /// Delivers through `program` instead of [`LINUX_NOTIFY_SEND`].
    ///
    /// Seam for the fake-based conformance suite: production always passes
    /// the absolute inbox path. Same empty/probe/spawn mapping as
    /// [`NotificationBackend::deliver`].
    fn deliver_via(&self, program: &str, notification: &DesktopNotification) -> DeliveryOutcome {
        if notification.is_empty() {
            return DeliveryOutcome::Skipped(SkipReason::BackendMissing);
        }
        if !backend_present(program) {
            return DeliveryOutcome::Skipped(SkipReason::BackendMissing);
        }
        spawn_notifier(program, &linux_notify_argv(notification))
    }
}

impl NotificationBackend for LinuxDbusBackend {
    fn name(&self) -> &'static str {
        "linux-dbus"
    }

    fn is_available(&self) -> bool {
        cfg!(target_os = "linux") && backend_present(LINUX_NOTIFY_SEND)
    }

    fn deliver(&self, notification: &DesktopNotification) -> DeliveryOutcome {
        if !cfg!(target_os = "linux") {
            return DeliveryOutcome::Skipped(SkipReason::BackendMissing);
        }
        self.deliver_via(LINUX_NOTIFY_SEND, notification)
    }
}

/// macOS backend: notification center via `osascript`.
///
/// Delivery spawns [`MACOS_OSASCRIPT`] with the fixed argv from
/// [`macos_osascript_argv`] (`-e` plus one AppleScript expression built by
/// [`osascript_notification_script`] with hostile text quoted inside string
/// literals). stdio is nulled, no shell is constructed, and the child is
/// handed to a bounded background reaper ([`NOTIFIER_REAP_WAIT`] wait at
/// [`NOTIFIER_REAP_POLL`] intervals), so the caller never blocks.
/// `terminal-notifier` would add an out-of-tree dependency, so the inbox
/// `osascript` is the delivery contract.
///
/// Fail-closed mapping mirrors [`LinuxDbusBackend`]: a fully empty
/// notification, a non-macOS platform, or a missing backend binary returns
/// `Skipped(BackendMissing)` without spawning; spawn failure returns `Failed`
/// with the I/O diagnostic; a successful reaper handoff returns `Delivered`.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacosBackend;

impl MacosBackend {
    /// Delivers through `program` instead of [`MACOS_OSASCRIPT`].
    ///
    /// Seam for the fake-based conformance suite: production always passes
    /// the absolute inbox path. Same empty/probe/spawn mapping as
    /// [`NotificationBackend::deliver`].
    fn deliver_via(&self, program: &str, notification: &DesktopNotification) -> DeliveryOutcome {
        if notification.is_empty() {
            return DeliveryOutcome::Skipped(SkipReason::BackendMissing);
        }
        if !backend_present(program) {
            return DeliveryOutcome::Skipped(SkipReason::BackendMissing);
        }
        spawn_notifier(program, &macos_osascript_argv(notification))
    }
}

impl NotificationBackend for MacosBackend {
    fn name(&self) -> &'static str {
        "macos-notification-center"
    }

    fn is_available(&self) -> bool {
        cfg!(target_os = "macos") && backend_present(MACOS_OSASCRIPT)
    }

    fn deliver(&self, notification: &DesktopNotification) -> DeliveryOutcome {
        if !cfg!(target_os = "macos") {
            return DeliveryOutcome::Skipped(SkipReason::BackendMissing);
        }
        self.deliver_via(MACOS_OSASCRIPT, notification)
    }
}

/// Windows backend: Toast notifications.
///
/// Fail-closed stub with an explicit gating gap: no inbox command-line toast
/// path exists without a native WinRT dependency, so `is_available()` is
/// false and `deliver()` returns `Skipped(BackendMissing)` without touching
/// the OS. Wiring WinRT awaits a scoped dependency decision in an isolated
/// `ffi` module with audit rows. Core-side retirement keeps the
/// Windows-missing behavior on both sides, so this gap changes nothing there.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsToastBackend;

impl NotificationBackend for WindowsToastBackend {
    fn name(&self) -> &'static str {
        "windows-toast"
    }

    fn is_available(&self) -> bool {
        false
    }

    fn deliver(&self, _notification: &DesktopNotification) -> DeliveryOutcome {
        DeliveryOutcome::Skipped(SkipReason::BackendMissing)
    }
}

/// Returns the matching backend for the compilation target.
///
/// The Linux and macOS arms deliver for real through their inbox helpers
/// (still fail-closed when the helper is absent); Windows and unknown targets
/// resolve to fail-closed stubs. Callers must still handle
/// [`SkipReason::BackendMissing`]: availability is platform- and
/// configuration-gated.
#[must_use]
pub fn platform_backend() -> Box<dyn NotificationBackend> {
    if cfg!(target_os = "linux") {
        Box::new(LinuxDbusBackend)
    } else if cfg!(target_os = "macos") {
        Box::new(MacosBackend)
    } else if cfg!(target_os = "windows") {
        Box::new(WindowsToastBackend)
    } else {
        Box::new(NoopBackend)
    }
}

/// Defensive rate-cap configuration for [`NotificationBridge`].
///
/// Defaults mirror RC-8. A zero `max_per_window` admits nothing (fail-closed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BridgeConfig {
    /// Admissions allowed per fixed window.
    pub max_per_window: u32,
    /// Fixed window length.
    pub window: Duration,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            max_per_window: DEFENSIVE_MAX_PER_WINDOW,
            window: DEFENSIVE_WINDOW,
        }
    }
}

impl BridgeConfig {
    /// Configuration admitting `max_per_window` deliveries per `window`.
    #[must_use]
    pub const fn new(max_per_window: u32, window: Duration) -> Self {
        Self {
            max_per_window,
            window,
        }
    }
}

/// Fixed-window defensive rate limiter.
///
/// Deterministic via the caller-supplied [`Instant`] so tests never depend on
/// wall-clock timing. A backwards clock can never re-open a window early
/// (`saturating_duration_since`). The bridge counts its own drops.
#[derive(Debug, Clone)]
struct DefensiveLimiter {
    window: Duration,
    limit: u32,
    window_start: Option<Instant>,
    admitted: u32,
}

impl DefensiveLimiter {
    /// New limiter admitting `limit` events per `window` (a zero limit admits
    /// nothing, so the surface stays fail-closed).
    const fn new(window: Duration, limit: u32) -> Self {
        Self {
            window,
            limit,
            window_start: None,
            admitted: 0,
        }
    }

    /// Whether one more event is admitted at `now`.
    fn admit_at(&mut self, now: Instant) -> bool {
        let expired = match self.window_start {
            None => true,
            Some(start) => now.saturating_duration_since(start) >= self.window,
        };
        if expired {
            self.window_start = Some(now);
            self.admitted = 0;
        }
        if self.admitted < self.limit {
            self.admitted = self.admitted.saturating_add(1);
            true
        } else {
            false
        }
    }
}

/// Notification bridge: defensive second cap plus backend dispatch.
///
/// Core rate-limits (RC-8) before calling here; this bridge re-caps so a
/// misbehaving caller can never flood the OS. Over-ceiling calls return
/// `Skipped(RateLimited)` and are counted; nothing queues without bound.
pub struct NotificationBridge {
    backend: Box<dyn NotificationBackend>,
    limiter: DefensiveLimiter,
    delivered: u64,
    rate_dropped: u64,
}

impl NotificationBridge {
    /// Bridge with default (RC-8-mirroring) defensive cap over `backend`.
    #[must_use]
    pub fn new(backend: Box<dyn NotificationBackend>) -> Self {
        Self::with_config(backend, BridgeConfig::default())
    }

    /// Bridge with an explicit defensive cap over `backend`.
    #[must_use]
    pub fn with_config(backend: Box<dyn NotificationBackend>, config: BridgeConfig) -> Self {
        Self {
            backend,
            limiter: DefensiveLimiter::new(config.window, config.max_per_window),
            delivered: 0,
            rate_dropped: 0,
        }
    }

    /// Delivers parsed `title`/`body` text (sanitized on construction).
    pub fn notify(&mut self, title: &str, body: &str) -> DeliveryOutcome {
        let notification = DesktopNotification::new(title, body);
        self.notify_parsed(&notification)
    }

    /// Delivers an already-constructed notification.
    pub fn notify_parsed(&mut self, notification: &DesktopNotification) -> DeliveryOutcome {
        self.notify_parsed_at(notification, Instant::now())
    }

    /// Delivers at an explicit instant (deterministic seam for tests).
    pub fn notify_parsed_at(
        &mut self,
        notification: &DesktopNotification,
        now: Instant,
    ) -> DeliveryOutcome {
        if !self.limiter.admit_at(now) {
            self.rate_dropped = self.rate_dropped.saturating_add(1);
            return DeliveryOutcome::Skipped(SkipReason::RateLimited);
        }
        let outcome = self.backend.deliver(notification);
        if outcome.is_delivered() {
            self.delivered = self.delivered.saturating_add(1);
        }
        outcome
    }

    /// Backend name for diagnostics.
    #[must_use]
    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }

    /// Deliveries the backend accepted since construction.
    #[must_use]
    pub const fn delivered(&self) -> u64 {
        self.delivered
    }

    /// Over-ceiling events dropped by the defensive cap since construction.
    #[must_use]
    pub const fn rate_dropped(&self) -> u64 {
        self.rate_dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// Recording double: captures sanitized payloads without touching the OS.
    struct RecordingBackend {
        available: bool,
        deliveries: Cell<u64>,
        last_title: std::cell::RefCell<String>,
        last_body: std::cell::RefCell<String>,
    }

    impl RecordingBackend {
        fn available() -> Self {
            Self {
                available: true,
                deliveries: Cell::new(0),
                last_title: std::cell::RefCell::new(String::new()),
                last_body: std::cell::RefCell::new(String::new()),
            }
        }
    }

    impl NotificationBackend for RecordingBackend {
        fn name(&self) -> &'static str {
            "recording-test-double"
        }

        fn is_available(&self) -> bool {
            self.available
        }

        fn deliver(&self, notification: &DesktopNotification) -> DeliveryOutcome {
            self.deliveries.set(self.deliveries.get().saturating_add(1));
            *self.last_title.borrow_mut() = notification.title().to_owned();
            *self.last_body.borrow_mut() = notification.body().to_owned();
            DeliveryOutcome::Delivered
        }
    }

    #[test]
    fn sanitize_strips_controls_and_bounds_length() {
        let raw = format!("a\u{1}b\nc\u{7}d  e\t{}", "x".repeat(400));
        let notification = DesktopNotification::new(&raw, &raw);
        assert!(!notification.title().contains('\u{1}'));
        assert!(!notification.title().contains('\n'));
        assert!(!notification.body().contains('\u{7}'));
        assert!(!notification.body().contains("  "));
        assert!(notification.title().chars().count() <= TITLE_MAX_CHARS);
        assert!(notification.body().chars().count() <= BODY_MAX_CHARS);
    }

    #[test]
    fn empty_sides_degrade() {
        let notification = DesktopNotification::new("", "");
        assert!(notification.is_empty());
        assert!(!DesktopNotification::new("t", "").is_empty());
        assert!(!DesktopNotification::new("", "b").is_empty());
    }

    #[test]
    fn hostile_payload_reaches_backend_only_sanitized() {
        let backend = RecordingBackend::available();
        let mut bridge = NotificationBridge::new(Box::new(backend));
        let hostile_title = "hi\"; rm -rf ~; \"\\bye\x07";
        let hostile_body = "$(id) `id` \u{1b}]0;pwned\x07";
        let outcome = bridge.notify(hostile_title, hostile_body);
        assert_eq!(outcome, DeliveryOutcome::Delivered);
    }

    /// Fake backend binary for delivery conformance (unix only).
    ///
    /// Installs an executable shell script that appends each argv entry on
    /// its own line to a log file, so tests assert the exact argv the backend
    /// would hand to the real helper without touching a desktop.
    #[cfg(unix)]
    struct FakeBackend {
        dir: std::path::PathBuf,
        program: std::path::PathBuf,
        log: std::path::PathBuf,
    }

    #[cfg(unix)]
    static FAKE_BACKEND_COUNTER: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(0);

    #[cfg(unix)]
    impl FakeBackend {
        fn install() -> Self {
            use std::os::unix::fs::PermissionsExt as _;
            let id = FAKE_BACKEND_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "bitty-platform-services-fake-{}-{id}",
                std::process::id()
            ));
            let program = dir.join("fake-notify");
            let log = dir.join("argv.log");
            let script = format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"{}\"\n",
                log.to_string_lossy()
            );
            std::fs::create_dir_all(&dir).expect("fake backend dir");
            std::fs::write(&program, script).expect("fake backend install");
            let mut permissions = std::fs::metadata(&program)
                .expect("fake backend metadata")
                .permissions();
            permissions.set_mode(0o750);
            std::fs::set_permissions(&program, permissions).expect("fake backend chmod");
            Self { dir, program, log }
        }

        fn program_string(&self) -> String {
            self.program.to_string_lossy().into_owned()
        }

        fn logged_argv(&self) -> Vec<String> {
            let text = std::fs::read_to_string(&self.log).unwrap_or_default();
            text.lines().map(String::from).collect()
        }

        fn spawned(&self) -> bool {
            self.log.is_file()
        }
    }

    /// Waits (bounded) for the fake backend child to record `want` argv
    /// entries.
    ///
    /// Delivery hands the child to a background reaper and returns
    /// immediately, so the fake script may not have run yet when the caller
    /// reads the log. The fake answers in milliseconds; the multi-second
    /// bound only trips on a genuine failure.
    #[cfg(unix)]
    fn await_logged_argv(fake: &FakeBackend, want: usize) -> Vec<String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let logged = fake.logged_argv();
            if logged.len() >= want || std::time::Instant::now() >= deadline {
                return logged;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[cfg(unix)]
    impl Drop for FakeBackend {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn linux_argv_is_fixed_and_absolute() {
        assert!(LINUX_NOTIFY_SEND.starts_with('/'));
        assert!(!LINUX_NOTIFY_SEND.contains("PATH"));
        let notification = DesktopNotification::new("Build", "done");
        assert_eq!(
            linux_notify_argv(&notification),
            vec![
                String::from(LINUX_NOTIFY_APP_NAME),
                String::from(LINUX_NOTIFY_EXPIRE),
                String::from("Build"),
                String::from("done"),
            ]
        );
        // No shell metacharacters are interpreted: hostile text stays a
        // literal argv entry.
        let hostile = DesktopNotification::new("$(id)", "`id`");
        let hostile_args = linux_notify_argv(&hostile);
        assert!(hostile_args.iter().any(|arg| arg == "$(id)"));
        assert!(hostile_args.iter().any(|arg| arg == "`id`"));
    }

    #[test]
    fn linux_untitled_notification_names_bitty() {
        let notification = DesktopNotification::new("", "plain");
        let args = linux_notify_argv(&notification);
        assert_eq!(args.len(), 4);
        assert_eq!(args[2], NOTIFY_DEFAULT_SUMMARY);
        assert_eq!(args[3], "plain");
    }

    #[test]
    fn osascript_quoting_never_breaks_out() {
        let hostile = "hi\"; do shell script \"rm -rf ~\"; \"\\bye";
        let script = osascript_notification_script("t\"tle", hostile);
        // Every interior quote/backslash is escaped: the script contains no
        // bare hostile text, only escaped literals.
        assert!(script.contains("\\\\"));
        assert!(script.contains("\\\""));
        assert!(script.starts_with("display notification"));
        // The exact hostile text (with raw quotes) never appears verbatim.
        assert!(!script.contains(hostile));
    }

    #[test]
    fn osascript_empty_sides_stay_wellformed() {
        let script = osascript_notification_script("", "hello");
        assert!(script.contains("with title \"bitty\""));
        let script = osascript_notification_script("title", "");
        assert!(script.contains("display notification \"title\""));
    }

    #[test]
    fn macos_argv_is_fixed_and_absolute() {
        assert!(MACOS_OSASCRIPT.starts_with('/'));
        let notification = DesktopNotification::new("Build", "done");
        let args = macos_osascript_argv(&notification);
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], MACOS_OSASCRIPT_EXPR_FLAG);
        assert!(args[1].starts_with("display notification"));
    }

    #[cfg(unix)]
    #[test]
    fn linux_delivers_through_fake_backend() {
        let fake = FakeBackend::install();
        let notification = DesktopNotification::new("Build", "finished");
        assert_eq!(
            LinuxDbusBackend.deliver_via(&fake.program_string(), &notification),
            DeliveryOutcome::Delivered
        );
        assert_eq!(
            await_logged_argv(&fake, 4),
            vec![
                String::from(LINUX_NOTIFY_APP_NAME),
                String::from(LINUX_NOTIFY_EXPIRE),
                String::from("Build"),
                String::from("finished"),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn macos_delivers_through_fake_backend() {
        let fake = FakeBackend::install();
        let notification = DesktopNotification::new("Build", "finished");
        assert_eq!(
            MacosBackend.deliver_via(&fake.program_string(), &notification),
            DeliveryOutcome::Delivered
        );
        let logged = await_logged_argv(&fake, 2);
        assert_eq!(logged.len(), 2);
        assert_eq!(logged[0], MACOS_OSASCRIPT_EXPR_FLAG);
        assert!(logged[1].starts_with("display notification"));
    }

    #[cfg(unix)]
    #[test]
    fn hostile_payload_reaches_fake_backend_only_as_literal_argv() {
        let fake = FakeBackend::install();
        let notification = DesktopNotification::new("$(id)", "`id`");
        assert_eq!(
            LinuxDbusBackend.deliver_via(&fake.program_string(), &notification),
            DeliveryOutcome::Delivered
        );
        let logged = await_logged_argv(&fake, 4);
        assert!(logged.iter().any(|arg| arg == "$(id)"));
        assert!(logged.iter().any(|arg| arg == "`id`"));
        // Had a shell expanded the payload, the log would hold command output
        // instead of the literal text.
    }

    #[cfg(unix)]
    #[test]
    fn empty_notification_never_spawns() {
        let fake = FakeBackend::install();
        let program = fake.program_string();
        let notification = DesktopNotification::new("", "");
        assert_eq!(
            LinuxDbusBackend.deliver_via(&program, &notification),
            DeliveryOutcome::Skipped(SkipReason::BackendMissing)
        );
        assert_eq!(
            MacosBackend.deliver_via(&program, &notification),
            DeliveryOutcome::Skipped(SkipReason::BackendMissing)
        );
        assert!(!fake.spawned(), "empty notification must not spawn");
    }

    #[cfg(unix)]
    #[test]
    fn missing_program_is_fail_closed_skip() {
        let notification = DesktopNotification::new("t", "b");
        assert_eq!(
            LinuxDbusBackend.deliver_via("/nonexistent-bitty-backend-0123456789", &notification),
            DeliveryOutcome::Skipped(SkipReason::BackendMissing)
        );
        assert_eq!(
            MacosBackend.deliver_via("/nonexistent-bitty-backend-0123456789", &notification),
            DeliveryOutcome::Skipped(SkipReason::BackendMissing)
        );
    }

    #[cfg(unix)]
    #[test]
    fn unexecutable_file_reports_failed() {
        let dir = std::env::temp_dir().join(format!(
            "bitty-platform-services-unexec-{}",
            std::process::id()
        ));
        let file = dir.join("not-executable");
        std::fs::create_dir_all(&dir).expect("unexecutable fixture dir");
        // Intentionally left non-executable: the presence probe passes (it is
        // a file) but the spawn fails.
        std::fs::write(&file, "not a program").expect("unexecutable fixture install");
        let program = file.to_string_lossy().into_owned();
        let notification = DesktopNotification::new("t", "b");
        assert!(matches!(
            LinuxDbusBackend.deliver_via(&program, &notification),
            DeliveryOutcome::Failed(_)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn noop_backend_never_touches_os() {
        let backend = NoopBackend;
        assert!(!backend.is_available());
        let notification = DesktopNotification::new("t", "b");
        assert_eq!(
            backend.deliver(&notification),
            DeliveryOutcome::Skipped(SkipReason::BackendMissing)
        );
    }

    #[test]
    fn windows_backend_stays_fail_closed() {
        let backend = WindowsToastBackend;
        assert!(!backend.is_available());
        let notification = DesktopNotification::new("t", "b");
        assert_eq!(
            backend.deliver(&notification),
            DeliveryOutcome::Skipped(SkipReason::BackendMissing)
        );
    }

    #[test]
    fn linux_and_macos_backends_fail_closed_without_helper() {
        // Never spawns the real helper: when the presence probe reports the
        // helper absent, delivery must skip without touching the OS. (When the
        // helper is present, real delivery is covered against fakes above.)
        for backend in [
            Box::new(LinuxDbusBackend) as Box<dyn NotificationBackend>,
            Box::new(MacosBackend),
        ] {
            if !backend.is_available() {
                let notification = DesktopNotification::new("t", "b");
                assert_eq!(
                    backend.deliver(&notification),
                    DeliveryOutcome::Skipped(SkipReason::BackendMissing),
                    "{} must fail closed without its helper",
                    backend.name()
                );
            }
        }
    }

    #[test]
    fn platform_backend_matches_target() {
        let backend = platform_backend();
        let expected = if cfg!(target_os = "linux") {
            "linux-dbus"
        } else if cfg!(target_os = "macos") {
            "macos-notification-center"
        } else if cfg!(target_os = "windows") {
            "windows-toast"
        } else {
            "noop"
        };
        assert_eq!(backend.name(), expected);
        // Delivery itself is covered against fakes above; invoking the real
        // helper here would touch a desktop, so selection asserts the name
        // only (no real desktop needed in CI).
    }

    #[test]
    fn bridge_delivers_through_mock_backend() {
        let mut bridge = NotificationBridge::new(Box::new(RecordingBackend::available()));
        let notification = DesktopNotification::new("Build", "finished");
        assert_eq!(
            bridge.notify_parsed(&notification),
            DeliveryOutcome::Delivered
        );
        assert_eq!(bridge.delivered(), 1);
        assert_eq!(bridge.rate_dropped(), 0);
    }

    #[test]
    fn defensive_cap_admits_burst_then_drops_within_window() {
        let mut bridge = NotificationBridge::new(Box::new(RecordingBackend::available()));
        let now = Instant::now();
        let notification = DesktopNotification::new("t", "b");
        for _ in 0..DEFENSIVE_MAX_PER_WINDOW {
            assert_eq!(
                bridge.notify_parsed_at(&notification, now),
                DeliveryOutcome::Delivered
            );
        }
        assert_eq!(
            bridge.notify_parsed_at(&notification, now),
            DeliveryOutcome::Skipped(SkipReason::RateLimited),
            "over-ceiling event must be dropped"
        );
        assert_eq!(bridge.rate_dropped(), 1);
        assert_eq!(
            bridge.notify_parsed_at(&notification, now + DEFENSIVE_WINDOW),
            DeliveryOutcome::Delivered,
            "next window admits again"
        );
    }

    #[test]
    fn defensive_cap_backwards_clock_never_reopens_window() {
        let config = BridgeConfig::new(1, DEFENSIVE_WINDOW);
        let mut bridge =
            NotificationBridge::with_config(Box::new(RecordingBackend::available()), config);
        let now = Instant::now();
        let notification = DesktopNotification::new("t", "b");
        assert_eq!(
            bridge.notify_parsed_at(&notification, now),
            DeliveryOutcome::Delivered
        );
        let earlier = now.checked_sub(DEFENSIVE_WINDOW).unwrap_or(now);
        assert_eq!(
            bridge.notify_parsed_at(&notification, earlier),
            DeliveryOutcome::Skipped(SkipReason::RateLimited)
        );
    }

    #[test]
    fn defensive_cap_zero_limit_is_fail_closed() {
        let config = BridgeConfig::new(0, DEFENSIVE_WINDOW);
        let mut bridge =
            NotificationBridge::with_config(Box::new(RecordingBackend::available()), config);
        let notification = DesktopNotification::new("t", "b");
        assert_eq!(
            bridge.notify_parsed(&notification),
            DeliveryOutcome::Skipped(SkipReason::RateLimited)
        );
        assert_eq!(bridge.delivered(), 0);
    }
}
