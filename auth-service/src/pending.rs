//! In-memory store of outstanding AuthnRequest IDs for the `InResponseTo`
//! replay check (eID §7.6.3.5 rule 4 / §9.7).
//!
//! Process-local implementation of the storage behind
//! [`AuthState::consume_if_pending`](crate::AuthState) and
//! [`register_pending_request`](crate::AuthState::register_pending_request),
//! for test setups only: it is lost on restart and not shared between
//! instances, while a login started on one server may have its ACS callback
//! handled by another. Production runs on the embedding application's
//! Postgres-backed store (`STORAGE_URL=postgres://`), which never touches this
//! module.

use parking_lot::Mutex;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

/// Retention window for an outstanding AuthnRequest ID used in the
/// `InResponseTo` replay check (eID §9.7). The correlated artifact is valid for
/// at most 15 minutes (eID §7.1/§7.5), so a legitimate flow always completes
/// within this window. Older entries are swept, bounding memory and the
/// replay-acceptance window for abandoned flows (cancellations/errors leave no
/// assertion to correlate, so they can only be reclaimed by age).
pub const PENDING_REQUEST_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);

const PENDING_REQUEST_TTL_MS: u64 = PENDING_REQUEST_TTL.as_secs() * 1000;

/// Map size below which no housekeeping sweep runs. Chosen so a normal working
/// set (outstanding logins within the 15-minute TTL) never triggers one, while
/// abandoned flows still cannot grow the map without bound.
const SWEEP_THRESHOLD: usize = 1024;

/// The outstanding IDs and the map size at which the next sweep runs: twice
/// what the last sweep left, so a sweep over n entries is paid for by at least
/// n registrations and the per-request cost stays O(1), however many fresh
/// entries abandoned logins keep in the map.
#[derive(Default)]
struct Pending {
    map: HashMap<String, u64>,
    sweep_at: usize,
}

impl Pending {
    fn sweep_if_large(&mut self, now: u64) {
        if self.map.len() >= self.sweep_at.max(SWEEP_THRESHOLD) {
            sweep(&mut self.map, now);
            self.sweep_at = self.map.len() * 2;
        }
    }
}

/// Sweep entries that have outlived [`PENDING_REQUEST_TTL`] so abandoned flows
/// cannot accumulate.
///
/// Housekeeping only: expiry itself is enforced per-entry at lookup time (see
/// [`PendingRequests::consume_if_pending`]), so *when* this runs never affects
/// whether an expired ID can match. That is what lets it be skipped on most
/// calls.
fn sweep(pending: &mut HashMap<String, u64>, now: u64) {
    pending.retain(|_, &mut created| !is_expired(created, now));
}

/// Whether an entry registered at `created` has outlived the TTL as of `now`.
fn is_expired(created: u64, now: u64) -> bool {
    now.saturating_sub(created) >= PENDING_REQUEST_TTL_MS
}

/// Process-local, in-memory set of outstanding AuthnRequest IDs with creation
/// timestamps, for test setups (see the module docs). Cheap to clone
/// (internally reference-counted), so it can be stored on an application's
/// shared state.
#[derive(Clone, Default)]
pub struct PendingRequests {
    inner: Arc<Mutex<Pending>>,
}

impl PendingRequests {
    /// Record an outgoing AuthnRequest ID.
    pub fn register(&self, id: String) {
        let now = unix_ms();
        let mut pending = self.inner.lock();
        pending.sweep_if_large(now);
        pending.map.insert(id, now);
    }

    /// Atomically check whether `id` is a still-valid outstanding request and,
    /// if so, consume it so it can never be matched again (eID §7.6.3.5 rule 4 /
    /// §9.7). Returns `false` for an unknown, expired, or already-consumed ID.
    ///
    /// An ID older than [`PENDING_REQUEST_TTL`] is rejected on its own timestamp,
    /// so expiry does not depend on a sweep having run. The entry is removed
    /// either way: an expired ID is consumed as it is refused, so a replay of it
    /// cannot even reach the age check twice.
    pub fn consume_if_pending(&self, id: &str) -> bool {
        let now = unix_ms();
        let mut pending = self.inner.lock();
        pending.sweep_if_large(now);
        match pending.map.remove(id) {
            Some(created) => !is_expired(created, now),
            None => false,
        }
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consume_if_pending_matches_once_then_rejects_replay() {
        let pending = PendingRequests::default();
        pending.register("abc".into());
        pending.register("def".into());

        // A registered ID matches exactly once...
        assert!(pending.consume_if_pending("abc"));
        // ...and a replay of the same ID is rejected.
        assert!(!pending.consume_if_pending("abc"));
        // Other registered IDs are unaffected.
        assert!(pending.consume_if_pending("def"));
    }

    #[test]
    fn consume_if_pending_rejects_unknown_id() {
        let pending = PendingRequests::default();
        assert!(!pending.consume_if_pending("never-registered"));
    }

    #[test]
    fn expired_pending_requests_are_rejected_and_swept() {
        let pending = PendingRequests::default();
        // Inject a stale entry whose timestamp is well past the TTL.
        pending
            .inner
            .lock()
            .map
            .insert("stale".into(), unix_ms() - PENDING_REQUEST_TTL_MS - 1);

        // An expired ID no longer matches (so a stale InResponseTo is rejected,
        // eID §9.7)...
        assert!(!pending.consume_if_pending("stale"));
        // ...and the stale entry has been physically swept in the same call.
        assert!(pending.inner.lock().map.is_empty());
    }

    /// Fresh entries survive a sweep, so one at the threshold must not repeat
    /// on every call while abandoned flows keep the map large.
    #[test]
    fn sweeps_are_amortised_over_registrations() {
        let pending = PendingRequests::default();
        for i in 0..SWEEP_THRESHOLD {
            pending.register(i.to_string());
        }
        assert_eq!(pending.inner.lock().sweep_at, 0);

        // The registration at the threshold sweeps, finds everything fresh, and
        // defers the next sweep to twice the surviving size.
        pending.register("fresh".into());
        assert_eq!(pending.inner.lock().sweep_at, 2 * SWEEP_THRESHOLD);
        assert!(!pending.consume_if_pending("unknown"));
        assert_eq!(pending.inner.lock().sweep_at, 2 * SWEEP_THRESHOLD);
    }
}
