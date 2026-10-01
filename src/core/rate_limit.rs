//! Rate limits, loaded from environment variables into
//! [`Config`](crate::Config).
//!
//! The per-stream limits are counted from the stream's own event log in
//! [`PgStore::update`](crate::PgStore::update), so there is no extra state and
//! the counts survive restarts. The blocked-notification limit is counted in
//! memory (see [`crate::app::blocked_notification`]).

use std::env;

use chrono::{DateTime, TimeDelta, Utc};

use crate::AppError;

/// Downloads per window; one download renders a PDF per candidate.
const DEFAULT_MAX_DOWNLOADS: usize = 60;

/// Events per window; far above manual data entry.
const DEFAULT_MAX_EVENTS: usize = 3_000;

/// Absolute cap on the number of events in one stream.
const DEFAULT_MAX_EVENTS_TOTAL: usize = 30_000;

/// CDN block notifications logged per user per window.
const DEFAULT_MAX_BLOCKED_NOTIFICATIONS: usize = 10;

/// Default sliding window: one hour.
const DEFAULT_WINDOW_SECS: u64 = 3_600;

/// The sliding window as a duration, saturating rather than panicking on an
/// absurdly large number of seconds.
fn window_from_secs(secs: u64) -> TimeDelta {
    i64::try_from(secs)
        .ok()
        .and_then(TimeDelta::try_seconds)
        .unwrap_or(TimeDelta::MAX)
}

/// A "no more than `max` within `window`" limit over a sliding window.
#[derive(Debug, Clone, Copy)]
pub struct RateLimit {
    /// Maximum number of occurrences within the window. `0` disables the limit.
    pub max: usize,
    /// Length of the sliding window.
    pub window: TimeDelta,
}

impl RateLimit {
    /// Whether `count` occurrences within the window already fill this limit.
    /// A disabled limit (`max == 0`) is never reached.
    pub fn is_reached(&self, count: usize) -> bool {
        self.max > 0 && count >= self.max
    }

    /// Start of the window ending at `now`. An absurdly long window saturates
    /// to "since forever" rather than panicking.
    pub fn window_start(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        now.checked_sub_signed(self.window)
            .unwrap_or(DateTime::<Utc>::MIN_UTC)
    }
}

/// The configured rate limits: per political-group stream, plus the per-user
/// cap on CDN block notifications.
#[derive(Debug, Clone, Copy)]
pub struct RateLimits {
    /// Document downloads per window.
    pub downloads: RateLimit,
    /// Events per window.
    pub events: RateLimit,
    /// Absolute cap on the number of events in one stream; `0` disables it.
    pub events_total: usize,
    /// CDN block notifications logged per user per window.
    pub blocked_notifications: RateLimit,
}

impl Default for RateLimits {
    fn default() -> Self {
        Self {
            downloads: RateLimit {
                max: DEFAULT_MAX_DOWNLOADS,
                window: window_from_secs(DEFAULT_WINDOW_SECS),
            },
            events: RateLimit {
                max: DEFAULT_MAX_EVENTS,
                window: window_from_secs(DEFAULT_WINDOW_SECS),
            },
            events_total: DEFAULT_MAX_EVENTS_TOTAL,
            blocked_notifications: RateLimit {
                max: DEFAULT_MAX_BLOCKED_NOTIFICATIONS,
                window: window_from_secs(DEFAULT_WINDOW_SECS),
            },
        }
    }
}

impl RateLimits {
    /// Read the limits from the environment, defaulting every unset variable.
    pub(super) fn from_env_with<F>(lookup: &mut F) -> Result<Self, AppError>
    where
        F: FnMut(&'static str) -> Result<String, env::VarError>,
    {
        let defaults = Self::default();

        Ok(Self {
            downloads: RateLimit {
                max: number("RATE_LIMIT_DOWNLOADS", defaults.downloads.max, lookup)?,
                window: window_from_env("RATE_LIMIT_DOWNLOADS_WINDOW_SECS", lookup)?,
            },
            events: RateLimit {
                max: number("RATE_LIMIT_EVENTS", defaults.events.max, lookup)?,
                window: window_from_env("RATE_LIMIT_EVENTS_WINDOW_SECS", lookup)?,
            },
            events_total: number("RATE_LIMIT_EVENTS_TOTAL", defaults.events_total, lookup)?,
            blocked_notifications: RateLimit {
                max: number(
                    "RATE_LIMIT_BLOCKED_NOTIFICATIONS",
                    defaults.blocked_notifications.max,
                    lookup,
                )?,
                window: window_from_env("RATE_LIMIT_BLOCKED_NOTIFICATIONS_WINDOW_SECS", lookup)?,
            },
        })
    }
}

#[cfg(test)]
impl RateLimits {
    /// Limits with an explicit maximum per kind, counted over `window`.
    pub fn new_for_test(
        max_downloads: usize,
        max_events: usize,
        events_total: usize,
        window: TimeDelta,
    ) -> Self {
        Self {
            downloads: RateLimit {
                max: max_downloads,
                window,
            },
            events: RateLimit {
                max: max_events,
                window,
            },
            events_total,
            blocked_notifications: RateLimit {
                max: DEFAULT_MAX_BLOCKED_NOTIFICATIONS,
                window,
            },
        }
    }

    /// The limits with the blocked-notification cap replaced.
    pub fn with_blocked_notifications(mut self, limit: RateLimit) -> Self {
        self.blocked_notifications = limit;
        self
    }
}

/// Read a window length from a `*_WINDOW_SECS` environment variable, keeping
/// [`DEFAULT_WINDOW_SECS`] when it is unset or blank.
fn window_from_env<F>(name: &'static str, lookup: &mut F) -> Result<TimeDelta, AppError>
where
    F: FnMut(&'static str) -> Result<String, env::VarError>,
{
    Ok(window_from_secs(number(name, DEFAULT_WINDOW_SECS, lookup)?))
}

/// Parse a numeric environment variable: unset or blank keeps `default`, a
/// non-numeric value stops startup.
fn number<T, F>(name: &'static str, default: T, lookup: &mut F) -> Result<T, AppError>
where
    T: std::str::FromStr,
    F: FnMut(&'static str) -> Result<String, env::VarError>,
{
    let Ok(value) = lookup(name) else {
        return Ok(default);
    };
    let value = value.trim();

    if value.is_empty() {
        return Ok(default);
    }

    value.parse().map_err(|_| {
        AppError::ConfigLoadError(format!(
            "{name} must be a non-negative whole number, got: {value}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn lookup_from(
        map: HashMap<&'static str, &'static str>,
    ) -> impl FnMut(&'static str) -> Result<String, env::VarError> {
        move |key| {
            map.get(key)
                .map(|value| (*value).to_string())
                .ok_or(env::VarError::NotPresent)
        }
    }

    fn limit(max: usize) -> RateLimit {
        RateLimit {
            max,
            window: TimeDelta::minutes(1),
        }
    }

    /// The count fills the limit at `max`; a zero maximum disables it.
    #[test]
    fn is_reached_compares_count_to_max() {
        assert!(limit(2).is_reached(2));
        assert!(!limit(3).is_reached(2));
        assert!(!limit(0).is_reached(1_000));
    }

    /// The window start sits one window before `now`; an out-of-range window
    /// saturates instead of panicking.
    #[test]
    fn window_start_saturates() {
        let now = Utc::now();

        assert_eq!(limit(1).window_start(now), now - TimeDelta::seconds(60));

        let absurd = RateLimit {
            max: 1,
            window: window_from_secs(u64::MAX),
        };
        assert_eq!(absurd.window_start(now), DateTime::<Utc>::MIN_UTC);
    }

    #[test]
    fn from_env_uses_defaults_when_unset() {
        let mut lookup = lookup_from(HashMap::new());
        let defaults = RateLimits::default();

        let limits = RateLimits::from_env_with(&mut lookup).expect("limits");

        assert_eq!(limits.downloads.max, defaults.downloads.max);
        assert_eq!(limits.events.max, defaults.events.max);
        assert_eq!(limits.events_total, defaults.events_total);
        assert_eq!(limits.events.window, defaults.events.window);
        assert_eq!(
            limits.blocked_notifications.max,
            defaults.blocked_notifications.max
        );
    }

    #[test]
    fn from_env_reads_configured_values() {
        let mut lookup = lookup_from(HashMap::from([
            ("RATE_LIMIT_DOWNLOADS", "3"),
            ("RATE_LIMIT_DOWNLOADS_WINDOW_SECS", "60"),
            ("RATE_LIMIT_EVENTS", "7"),
            ("RATE_LIMIT_EVENTS_WINDOW_SECS", "120"),
            ("RATE_LIMIT_EVENTS_TOTAL", "9"),
            ("RATE_LIMIT_BLOCKED_NOTIFICATIONS", "2"),
            ("RATE_LIMIT_BLOCKED_NOTIFICATIONS_WINDOW_SECS", "30"),
        ]));

        let limits = RateLimits::from_env_with(&mut lookup).expect("limits");

        assert_eq!(limits.downloads.max, 3);
        assert_eq!(limits.downloads.window, TimeDelta::seconds(60));
        assert_eq!(limits.events.max, 7);
        assert_eq!(limits.events.window, TimeDelta::seconds(120));
        assert_eq!(limits.events_total, 9);
        assert_eq!(limits.blocked_notifications.max, 2);
        assert_eq!(limits.blocked_notifications.window, TimeDelta::seconds(30));
    }

    /// A blank value is treated as unset, a garbage value stops startup.
    #[test]
    fn from_env_rejects_a_non_numeric_value() {
        let mut blank = lookup_from(HashMap::from([("RATE_LIMIT_EVENTS", "  ")]));
        assert_eq!(
            RateLimits::from_env_with(&mut blank)
                .expect("limits")
                .events
                .max,
            RateLimits::default().events.max
        );

        let mut garbage = lookup_from(HashMap::from([("RATE_LIMIT_EVENTS", "many")]));
        let err = RateLimits::from_env_with(&mut garbage).expect_err("must be rejected");

        assert!(
            matches!(err, AppError::ConfigLoadError(ref message)
                if message.contains("RATE_LIMIT_EVENTS")),
            "got {err:?}"
        );
    }
}
