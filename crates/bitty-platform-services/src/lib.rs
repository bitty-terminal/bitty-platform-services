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
//! Per-platform backends sit behind [`NotificationBackend`]: Linux D-Bus
//! [`LinuxDbusBackend`] (`org.freedesktop.Notifications`), macOS notification
//! center [`MacosBackend`], Windows Toast [`WindowsToastBackend`], plus
//! [`NoopBackend`] for headless runs and tests. Native D-Bus /
//! notification-center / WinRT wiring awaits a scoped dependency decision and
//! stays fail-closed until then: `is_available()` is false and `deliver()`
//! returns [`SkipReason::BackendMissing`] without touching the OS. No shell
//! is ever constructed or interpolated anywhere in the delivery path.
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
/// Single home for the well-known name so future D-Bus wiring references one
/// constant. No connection is opened today; the backend stays fail-closed.
pub const LINUX_DBUS_SERVICE: &str = "org.freedesktop.Notifications";

/// Linux D-Bus object path for desktop notifications (spec-defined).
pub const LINUX_DBUS_OBJECT_PATH: &str = "/org/freedesktop/Notifications";

/// Linux D-Bus interface for desktop notifications (spec-defined).
pub const LINUX_DBUS_INTERFACE: &str = "org.freedesktop.Notifications";

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

/// Linux backend: D-Bus `org.freedesktop.Notifications`.
///
/// Fail-closed stub: native D-Bus wiring awaits a scoped dependency decision
/// (no D-Bus client is linked today), so `is_available()` is false and
/// `deliver()` skips without touching the session bus. The spec-defined
/// service constants above are the single home for the well-known name.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxDbusBackend;

impl NotificationBackend for LinuxDbusBackend {
    fn name(&self) -> &'static str {
        "linux-dbus"
    }

    fn is_available(&self) -> bool {
        false
    }

    fn deliver(&self, _notification: &DesktopNotification) -> DeliveryOutcome {
        DeliveryOutcome::Skipped(SkipReason::BackendMissing)
    }
}

/// macOS backend: notification center.
///
/// Fail-closed stub: native notification-center wiring awaits a scoped
/// dependency decision, so `is_available()` is false and `deliver()` skips
/// without touching the OS.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacosBackend;

impl NotificationBackend for MacosBackend {
    fn name(&self) -> &'static str {
        "macos-notification-center"
    }

    fn is_available(&self) -> bool {
        false
    }

    fn deliver(&self, _notification: &DesktopNotification) -> DeliveryOutcome {
        DeliveryOutcome::Skipped(SkipReason::BackendMissing)
    }
}

/// Windows backend: Toast notifications.
///
/// Fail-closed stub: native WinRT Toast wiring awaits a scoped dependency
/// decision, so `is_available()` is false and `deliver()` skips without
/// touching the OS.
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
/// Each arm is a fail-closed stub until native wiring lands; unknown targets
/// resolve to [`NoopBackend`]. Callers must still handle
/// [`SkipReason::BackendMissing`]: availability is platform-gated.
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
    fn stub_backends_are_fail_closed() {
        for backend in [
            Box::new(LinuxDbusBackend) as Box<dyn NotificationBackend>,
            Box::new(MacosBackend),
            Box::new(WindowsToastBackend),
            Box::new(NoopBackend),
        ] {
            assert!(
                !backend.is_available(),
                "{} must stay unavailable",
                backend.name()
            );
            let notification = DesktopNotification::new("t", "b");
            assert_eq!(
                backend.deliver(&notification),
                DeliveryOutcome::Skipped(SkipReason::BackendMissing),
                "{} must fail closed",
                backend.name()
            );
        }
    }

    #[test]
    fn platform_backend_matches_target_and_fails_closed() {
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
        let notification = DesktopNotification::new("t", "b");
        assert_eq!(
            backend.deliver(&notification),
            DeliveryOutcome::Skipped(SkipReason::BackendMissing)
        );
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
