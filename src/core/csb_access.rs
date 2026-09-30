//! Access guards for the CSB section, loaded from environment variables into
//! [`Config`](crate::Config): the peer-address allow list and the hours in
//! which committee activity raises an alert, plus the throttle that keeps
//! that alert from repeating on every request.

use std::{
    collections::{HashMap, hash_map::Entry},
    hash::Hash,
    net::IpAddr,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::NaiveTime;
use ipnet::IpNet;
use parking_lot::Mutex;

use crate::AppError;

/// Peer addresses allowed to reach the CSB section (`CSB_IP_ALLOW_LIST`):
/// single addresses and CIDR ranges, IPv4 or IPv6.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsbIpAllowList(Vec<IpNet>);

impl CsbIpAllowList {
    /// Whether `ip` falls in one of the listed ranges. An IPv4 peer on a
    /// dual-stack listener arrives as a v4-mapped IPv6 address, so it is also
    /// matched as plain IPv4.
    pub fn allows(&self, ip: IpAddr) -> bool {
        let unmapped = match ip {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4),
            IpAddr::V4(_) => None,
        };
        self.0
            .iter()
            .any(|net| net.contains(&ip) || unmapped.is_some_and(|ip| net.contains(&ip)))
    }

    /// Parses the comma-separated list. Strict: one malformed entry rejects the
    /// configuration rather than silently shrinking the allow list.
    pub fn parse(raw: &str) -> Result<Self, AppError> {
        let nets = raw
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(parse_entry)
            .collect::<Result<Vec<_>, _>>()?;
        if nets.is_empty() {
            return Err(AppError::ConfigLoadError(
                "CSB_IP_ALLOW_LIST must contain at least one IP address or CIDR range".to_string(),
            ));
        }
        Ok(Self(nets))
    }
}

/// One allow-list entry: a bare address or a CIDR range.
fn parse_entry(entry: &str) -> Result<IpNet, AppError> {
    if let Ok(ip) = IpAddr::from_str(entry) {
        return Ok(IpNet::from(ip));
    }
    IpNet::from_str(entry).map_err(|_| {
        AppError::ConfigLoadError(format!(
            "CSB_IP_ALLOW_LIST entry {entry:?} is neither an IP address nor a CIDR range \
             (e.g. 203.0.113.7, 203.0.113.0/24, 2001:db8::/32)"
        ))
    })
}

/// Local (Europe/Amsterdam) hours in which committee activity is unexpected
/// (`CSB_ALERT_HOURS`, e.g. `22:00-06:00`). Activity in this window is
/// reported, never blocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CsbAlertHours {
    /// Start of the window, inclusive.
    pub start: NaiveTime,
    /// End of the window, exclusive; before `start` when it wraps midnight.
    pub end: NaiveTime,
}

impl CsbAlertHours {
    /// Whether `time` falls in the window, wrapping past midnight when the
    /// end lies before the start.
    pub fn contains(&self, time: NaiveTime) -> bool {
        if self.start < self.end {
            self.start <= time && time < self.end
        } else {
            time >= self.start || time < self.end
        }
    }

    /// Parses `HH:MM-HH:MM`. Equal start and end are rejected as ambiguous.
    pub fn parse(raw: &str) -> Result<Self, AppError> {
        let malformed = || {
            AppError::ConfigLoadError(format!(
                "CSB_ALERT_HOURS {raw:?} is not a time range of the form HH:MM-HH:MM \
                 (e.g. 22:00-06:00)"
            ))
        };
        let (start, end) = raw.trim().split_once('-').ok_or_else(malformed)?;
        let parse = |s: &str| NaiveTime::parse_from_str(s.trim(), "%H:%M").map_err(|_| malformed());
        let (start, end) = (parse(start)?, parse(end)?);
        if start == end {
            return Err(AppError::ConfigLoadError(format!(
                "CSB_ALERT_HOURS {raw:?} starts and ends at the same time"
            )));
        }
        Ok(Self { start, end })
    }
}

/// Suppresses repeats of an alert for the same key within an interval, so a
/// working session raises it once rather than on every request. Shared by
/// the clones of one `AppState`, so both listeners count as one.
#[derive(Debug, Clone)]
pub struct AlertThrottle<K> {
    interval: Duration,
    last_raised: Arc<Mutex<HashMap<K, Instant>>>,
}

impl<K: Hash + Eq> AlertThrottle<K> {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            last_raised: Arc::default(),
        }
    }

    /// Whether the alert for `key` may be raised at `now`: once per interval,
    /// recording the time when it may. Expired entries are dropped on the way,
    /// so the map holds only the keys alerted on within the last interval.
    pub fn allows(&self, key: K, now: Instant) -> bool {
        let mut last_raised = self.last_raised.lock();
        last_raised.retain(|_, raised| now.duration_since(*raised) < self.interval);
        match last_raised.entry(key) {
            Entry::Occupied(_) => false,
            Entry::Vacant(entry) => {
                entry.insert(now);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().expect("ip")
    }

    fn time(s: &str) -> NaiveTime {
        NaiveTime::parse_from_str(s, "%H:%M").expect("time")
    }

    #[test]
    fn allow_list_accepts_addresses_and_ranges() {
        let list =
            CsbIpAllowList::parse(" 203.0.113.7, 10.0.0.0/8 ,2001:db8::/32, ::1 ").expect("list");

        for allowed in ["203.0.113.7", "10.1.2.3", "2001:db8::1", "::1"] {
            assert!(list.allows(ip(allowed)), "{allowed}");
        }
        for denied in ["203.0.113.8", "11.0.0.1", "2001:db9::1", "::2"] {
            assert!(!list.allows(ip(denied)), "{denied}");
        }
    }

    /// An IPv4 peer on a dual-stack listener shows up as `::ffff:a.b.c.d`.
    #[test]
    fn allow_list_matches_v4_mapped_peers_against_v4_entries() {
        let list = CsbIpAllowList::parse("10.0.0.0/8").expect("list");

        assert!(list.allows(ip("::ffff:10.1.2.3")));
        assert!(!list.allows(ip("::ffff:11.1.2.3")));
    }

    #[test]
    fn allow_list_rejects_malformed_or_empty_input() {
        for raw in [
            "",
            " , ",
            "office",
            "10.0.0.0/33",
            "10.0.0.1,nope",
            "10.0.0",
        ] {
            assert!(
                matches!(
                    CsbIpAllowList::parse(raw),
                    Err(AppError::ConfigLoadError(_))
                ),
                "{raw:?} must be rejected"
            );
        }
    }

    #[test]
    fn alert_hours_parse_and_wrap_midnight() {
        let hours = CsbAlertHours::parse(" 22:00 - 06:00 ").expect("hours");

        for inside in ["22:00", "23:59", "00:00", "03:30", "05:59"] {
            assert!(hours.contains(time(inside)), "{inside}");
        }
        for outside in ["06:00", "12:00", "21:59"] {
            assert!(!hours.contains(time(outside)), "{outside}");
        }
    }

    #[test]
    fn alert_hours_within_one_day() {
        let hours = CsbAlertHours::parse("09:00-17:00").expect("hours");

        assert!(hours.contains(time("09:00")));
        assert!(hours.contains(time("16:59")));
        assert!(!hours.contains(time("17:00")));
        assert!(!hours.contains(time("08:59")));
    }

    #[test]
    fn throttle_allows_once_per_key_and_interval() {
        let throttle = AlertThrottle::new(Duration::from_secs(60));
        let start = Instant::now();

        assert!(throttle.allows("a", start));
        assert!(!throttle.allows("a", start + Duration::from_secs(59)));
        assert!(throttle.allows("b", start));
        assert!(throttle.allows("a", start + Duration::from_secs(60)));
        assert!(!throttle.allows("a", start + Duration::from_secs(61)));
    }

    #[test]
    fn alert_hours_reject_malformed_input() {
        for raw in [
            "",
            "22:00",
            "22:00-",
            "22-06",
            "25:00-06:00",
            "22:00-06:60",
            "22:00-22:00",
            "night",
        ] {
            assert!(
                matches!(CsbAlertHours::parse(raw), Err(AppError::ConfigLoadError(_))),
                "{raw:?} must be rejected"
            );
        }
    }
}
