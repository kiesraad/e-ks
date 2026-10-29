//! Shared constants used across the app.

use std::{num::NonZeroU64, time::Duration};

use chrono_tz::{Europe, Tz};

/// Default date format
pub const DEFAULT_DATE_FORMAT: &str = "%d-%m-%Y";

/// Default time format
pub const DEFAULT_TIME_FORMAT: &str = "%H:%M";

/// Default datetime format
pub const DEFAULT_DATE_TIME_FORMAT: &str = "%d-%m-%Y %H:%M";

/// Default datetime format with seconds
pub const DATE_TIME_SECONDS_FORMAT: &str = "%d-%m-%Y %H:%M:%S";

pub const DEFAULT_TIMEZONE: &Tz = &Europe::Amsterdam;

/// A committee user active within `CSB_ALERT_HOURS` is reported at most once
/// per peer address in this interval, not on every request.
pub const CSB_ALERT_HOURS_REPEAT_INTERVAL: Duration = Duration::from_mins(30);

pub const MAX_CANDIDATES: usize = 80;

/// Election the dev login lands on when `DEFAULT_ELECTION` is unset.
#[cfg(feature = "dev-features")]
pub(crate) const DEV_LOGIN_ELECTION: crate::ElectionConfig = crate::ElectionConfig::EK27;

/// Hard cap on the persons one political group's stream may hold. Persons
/// are held in memory, cloned by the list and audit-log pages and copied into
/// the CSB import, so the count is bounded independently of the event caps.
pub const MAX_PERSONS: usize = 1_000;

/// Path of the Haal Centraal "personen" resource as the test stub serves it,
/// mirroring the `personen-mock` container; `BRP_URL` names it in full.
#[cfg(test)]
pub(crate) const BRP_PERSONS_ENDPOINT: &str = "haalcentraal/api/brp/personen";

/// Default request timeout (in seconds) for BRP lookups.
pub(crate) const BRP_TIMEOUT: u64 = 30;

/// Default minimum pause (in milliseconds) between the starts of two BRP
/// requests.
pub(crate) const BRP_REQUEST_INTERVAL_MS: u64 = 100;

/// Default idle time (in minutes) before a cached store projection is evicted.
pub(crate) const STORE_CACHE_IDLE_MINUTES: NonZeroU64 =
    NonZeroU64::new(24 * 60).expect("24 * 60 is non-zero");
