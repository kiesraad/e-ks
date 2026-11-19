//! Shared database-availability state and the background prober that keeps it
//! current.

use std::{sync::Arc, time::Duration};

use jiff::Timestamp;
use parking_lot::RwLock;
use tokio::sync::Notify;

use super::StorePersistence;

/// Current view of database availability.
#[derive(Clone, Debug)]
pub enum HealthState {
    /// The database is reachable and the schema is present.
    Healthy,
    /// The database is unreachable or its schema is broken/incomplete.
    Unavailable {
        /// When the outage was first observed.
        since: Timestamp,
        /// The most recent error message, for diagnostics.
        last_error: String,
    },
}

#[derive(Clone)]
pub struct DbHealth {
    state: Arc<RwLock<HealthState>>,
    wake: Arc<Notify>,
}

impl Default for DbHealth {
    fn default() -> Self {
        Self {
            state: Arc::new(RwLock::new(HealthState::Healthy)),
            wake: Arc::new(Notify::new()),
        }
    }
}

impl DbHealth {
    /// Whether the database is currently considered healthy.
    pub fn is_healthy(&self) -> bool {
        matches!(&*self.state.read(), HealthState::Healthy)
    }

    /// A snapshot of the current state (for diagnostics / the health endpoint).
    pub fn snapshot(&self) -> HealthState {
        self.state.read().clone()
    }

    /// Mark the database healthy. Logs only on an actual recovery.
    pub fn mark_healthy(&self) {
        let mut state = self.state.write();
        if !matches!(&*state, HealthState::Healthy) {
            tracing::info!("database recovered: serving normally again");
            *state = HealthState::Healthy;
        }
    }

    /// Mark the database unavailable and wake the prober. Preserves the original
    /// `since` across repeated reports, updating only the latest error.
    pub fn mark_unavailable(&self, error: impl std::fmt::Display) {
        let mut state = self.state.write();
        match &mut *state {
            HealthState::Healthy => {
                tracing::warn!("database unavailable: {error}; serving maintenance page");
                *state = HealthState::Unavailable {
                    since: Timestamp::now(),
                    last_error: error.to_string(),
                };
            }
            HealthState::Unavailable { last_error, .. } => {
                *last_error = error.to_string();
            }
        }
        self.wake.notify_one();
    }

    /// Wait until a request asks for a re-check (or a prior request already did).
    async fn wait_for_check_request(&self) {
        self.wake.notified().await;
    }
}

/// Background task: keep `health` in sync with the backend's real availability.
pub async fn run_db_prober(persistence: StorePersistence, health: DbHealth) {
    /// Routine re-check cadence while healthy.
    const HEALTHY_INTERVAL: Duration = Duration::from_secs(30);
    /// Backoff schedule (seconds) while unavailable; the last value repeats.
    const BACKOFF_SECS: [u64; 4] = [1, 5, 15, 30];

    let mut consecutive_failures: usize = 0;

    let mut migrated = false;

    loop {
        let ready = async {
            if !migrated {
                persistence.migrate().await?;
                migrated = true;
            }
            persistence.verify_ready().await
        }
        .await;

        match ready {
            Ok(()) => {
                health.mark_healthy();
                consecutive_failures = 0;
                tokio::select! {
                    _ = tokio::time::sleep(HEALTHY_INTERVAL) => {}
                    _ = health.wait_for_check_request() => {}
                }
            }
            Err(err) => {
                health.mark_unavailable(&err);
                let idx = consecutive_failures.min(BACKOFF_SECS.len() - 1);
                consecutive_failures = consecutive_failures.saturating_add(1);
                tokio::time::sleep(Duration::from_secs(BACKOFF_SECS[idx])).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_healthy() {
        let health = DbHealth::default();
        assert!(health.is_healthy());
    }

    #[test]
    fn mark_unavailable_then_healthy_round_trips() {
        let health = DbHealth::default();

        health.mark_unavailable("connection refused");
        assert!(!health.is_healthy());
        match health.snapshot() {
            HealthState::Unavailable { last_error, .. } => {
                assert_eq!(last_error, "connection refused");
            }
            HealthState::Healthy => panic!("expected unavailable"),
        }

        health.mark_healthy();
        assert!(health.is_healthy());
    }

    #[test]
    fn repeated_unavailable_updates_error_but_keeps_since() {
        let health = DbHealth::default();
        health.mark_unavailable("first");
        let since_first = match health.snapshot() {
            HealthState::Unavailable { since, .. } => since,
            HealthState::Healthy => panic!("expected unavailable"),
        };

        health.mark_unavailable("second");
        match health.snapshot() {
            HealthState::Unavailable { since, last_error } => {
                assert_eq!(since, since_first, "outage start must be preserved");
                assert_eq!(last_error, "second");
            }
            HealthState::Healthy => panic!("expected unavailable"),
        }
    }
}
