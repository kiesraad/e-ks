//! Per-stream rate limits, part of [`Config`](crate::Config).
//!
//! Counted from the stream's own event log in
//! [`PgStore::update`](crate::PgStore::update), so there is no extra state and
//! the counts survive restarts.

use chrono::{DateTime, TimeDelta, Utc};

/// Downloads per window; one download renders a PDF per candidate.
const DEFAULT_MAX_DOWNLOADS: usize = 60;

/// Events per window; far above manual data entry.
const DEFAULT_MAX_EVENTS: usize = 3_000;

/// Absolute cap on the number of events in one stream.
const DEFAULT_MAX_EVENTS_TOTAL: usize = 30_000;

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

    /// This limit with the configured `max` and window laid over it where set.
    pub(super) fn with(self, max: Option<usize>, window_secs: Option<u64>) -> Self {
        Self {
            max: max.unwrap_or(self.max),
            window: window_secs.map_or(self.window, window_from_secs),
        }
    }
}

/// The rate limits applied to a political group's own event stream.
#[derive(Debug, Clone, Copy)]
pub struct RateLimits {
    /// Document downloads per window.
    pub downloads: RateLimit,
    /// Events per window.
    pub events: RateLimit,
    /// Absolute cap on the number of events in one stream; `0` disables it.
    pub events_total: usize,
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
        }
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// `with` keeps whatever is not set.
    #[test]
    fn with_lays_settings_over_the_limit() {
        let configured = limit(5).with(Some(9), None);
        assert_eq!(configured.max, 9);
        assert_eq!(configured.window, TimeDelta::minutes(1));

        let configured = limit(5).with(None, Some(90));
        assert_eq!(configured.max, 5);
        assert_eq!(configured.window, TimeDelta::seconds(90));
    }
}
