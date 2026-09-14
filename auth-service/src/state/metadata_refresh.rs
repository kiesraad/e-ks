//! Keeping the verified RD (IdP) descriptor fresh: the startup fetch and the
//! background refresh task (eID §8.5).
//!
//! Split out of `state/mod.rs` so the module the embedding application depends
//! on carries only the state container and the `AuthState` contract; none of
//! this is reachable from outside the crate.

use std::{
    path::Path,
    sync::{Arc, Weak},
    time::Duration,
};

use tracing::{debug, error, warn};

use super::Inner;
use crate::saml::idp_metadata::{
    IdpMetadata, RdTrust, fetch_and_cache_idp_metadata, load_cached_idp_metadata,
};

/// Start the background refresh task for `inner`.
///
/// The task observes the state through a `Weak` handle and exits once the state
/// is dropped, so it does not keep an otherwise unused state alive.
pub(super) fn spawn(inner: &Arc<Inner>) {
    tokio::spawn(metadata_refresh_loop(Arc::downgrade(inner)));
}

/// Upper bound on how often the background task re-fetches the IdP metadata once
/// a descriptor is loaded: the RD metadata is re-fetched at least every 24h, and
/// sooner when its `cacheDuration` (eID §8.5) indicates a shorter window (see
/// [`next_refresh_interval`]). The hard `validUntil` expiry is enforced
/// separately at parse time (`saml::idp_metadata`).
const MAX_METADATA_REFRESH_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Floor on the computed refresh interval, so a very small (or buggy)
/// `cacheDuration` cannot make the task re-fetch in a tight loop.
const MIN_METADATA_REFRESH_INTERVAL: Duration = Duration::from_secs(15 * 60);

/// How often the background task retries while *no* descriptor is loaded (the RD
/// was unreachable at startup and there was no disk cache). Short enough that the
/// SAML login flow recovers within minutes of the RD coming back rather than
/// waiting a full refresh interval.
const METADATA_RECOVERY_RETRY_INTERVAL: Duration = Duration::from_secs(60);

/// The next refresh delay for a loaded descriptor: its `cacheDuration` clamped to
/// `[MIN_METADATA_REFRESH_INTERVAL, MAX_METADATA_REFRESH_INTERVAL]`, defaulting to
/// the 24h cap when the metadata carries no (parseable) `cacheDuration`.
fn next_refresh_interval(metadata: &IdpMetadata) -> Duration {
    metadata
        .cache_duration
        .unwrap_or(MAX_METADATA_REFRESH_INTERVAL)
        .clamp(MIN_METADATA_REFRESH_INTERVAL, MAX_METADATA_REFRESH_INTERVAL)
}

/// Number of times startup tries to fetch the RD metadata before falling back to
/// the on-disk cache (and, failing that, booting without a descriptor).
const STARTUP_FETCH_ATTEMPTS: u32 = 3;

/// Delay between the startup fetch attempts.
const STARTUP_FETCH_RETRY_DELAY: Duration = Duration::from_secs(2);

/// Keep the IdP metadata fresh until the state is dropped.
///
/// Waits first, then refreshes: startup already made its attempts, so there is
/// nothing to do on the immediate first pass. The delay is short while no
/// descriptor is loaded yet; otherwise it is driven by the loaded descriptor's
/// `cacheDuration`, capped at 24h. The `Weak` handle ends the task once the
/// state is dropped.
async fn metadata_refresh_loop(weak: Weak<Inner>) {
    loop {
        let delay = match weak.upgrade() {
            Some(inner) => match inner.rd_metadata.read().as_deref() {
                Some(metadata) => next_refresh_interval(metadata),
                None => METADATA_RECOVERY_RETRY_INTERVAL,
            },
            None => {
                debug!("[metadata] Auth state dropped, stopping refresh task");
                break;
            }
        };
        tokio::time::sleep(delay).await;

        let Some(inner) = weak.upgrade() else {
            debug!("[metadata] Auth state dropped, stopping refresh task");
            break;
        };
        refresh_rd_metadata(&inner).await;
    }
}

/// Obtain the RD metadata at startup, returning `None` rather than failing so the
/// application can always boot.
///
/// Tries the live fetch up to [`STARTUP_FETCH_ATTEMPTS`] times, then falls back
/// to the on-disk cache from a previous run. If neither yields a descriptor the
/// process still boots with `None`; the SAML login flow then reports itself
/// unavailable (the handlers check for a descriptor) until the background
/// refresh task, which retries quickly while none is loaded, manages to fetch
/// one.
pub(super) async fn load_at_startup(
    url: &str,
    certs_dir: &Path,
    trust: RdTrust,
) -> Option<IdpMetadata> {
    if let Some(fetched) = fetch_rd_metadata_with_retries(url, certs_dir, &trust).await {
        return Some(fetched);
    }
    match load_cached_idp_metadata(certs_dir, &trust).await {
        Some(cached) => {
            warn!(
                "[metadata] RD metadata fetch exhausted {STARTUP_FETCH_ATTEMPTS} attempts; falling back to on-disk cache"
            );
            Some(cached)
        }
        None => {
            error!(
                "[metadata] No RD metadata after {STARTUP_FETCH_ATTEMPTS} attempts and no disk cache; booting with SAML login unavailable, the background task will keep retrying"
            );
            None
        }
    }
}

/// Fetch the RD metadata, retrying up to [`STARTUP_FETCH_ATTEMPTS`] times with
/// [`STARTUP_FETCH_RETRY_DELAY`] between attempts to ride out a brief RD outage.
async fn fetch_rd_metadata_with_retries(
    url: &str,
    certs_dir: &Path,
    trust: &RdTrust,
) -> Option<IdpMetadata> {
    for attempt in 1..=STARTUP_FETCH_ATTEMPTS {
        match fetch_and_cache_idp_metadata(url, certs_dir, trust).await {
            Ok(m) => return Some(m),
            Err(e) => {
                warn!(
                    "[metadata] RD metadata fetch attempt {attempt}/{STARTUP_FETCH_ATTEMPTS} failed: {e}"
                );
                if attempt < STARTUP_FETCH_ATTEMPTS {
                    tokio::time::sleep(STARTUP_FETCH_RETRY_DELAY).await;
                }
            }
        }
    }
    None
}

/// Re-fetch the RD metadata for the background refresh task, swapping in the new
/// descriptor on success and keeping the previous one (which may be `None`) on
/// failure.
async fn refresh_rd_metadata(inner: &Inner) {
    let cfg = &inner.auth_config;
    let trust = RdTrust::for_environment(cfg.environment);
    match fetch_and_cache_idp_metadata(&cfg.rd.metadata_url, &cfg.certs_dir, &trust).await {
        Ok(metadata) => {
            debug!(
                "[metadata] Refreshed RD metadata: entity_id={}, signing_keys={}",
                metadata.entity_id,
                metadata.signing_keys.len()
            );
            *inner.rd_metadata.write() = Some(Arc::new(metadata));
        }
        Err(e) => {
            warn!("[metadata] Scheduled RD metadata refresh failed: {e}; keeping previous metadata")
        }
    }
}
