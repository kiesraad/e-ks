//! Session storage with pluggable in-memory or database backends.
//!
//! The backend is selected by `STORAGE_URL`:
//! - `memory://...` → in-memory (default).
//! - `local://...` → in-memory (disk is **not** a valid session backend; we
//!   treat the filesystem as less trusted than application memory).
//! - `postgres://...` / `postgresql://` → Postgres.

use std::{collections::HashMap, sync::Arc};

use parking_lot::RwLock;
#[cfg(feature = "database")]
use tracing::error;
use tracing::info;

use crate::{AppError, Session, auth::session::hash_token, utils::StorageScheme};

#[cfg(feature = "database")]
use crate::auth::session_db;

/// Session storage backend.
#[derive(Clone)]
pub enum SessionStore {
    /// Process-local, in-memory storage. Cleared on restart.
    InMemory(Arc<RwLock<HashMap<String, Session>>>),
    /// Postgres-backed storage, survives restarts.
    #[cfg(feature = "database")]
    Database(sqlx::PgPool),
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::InMemory(Arc::new(RwLock::new(HashMap::new())))
    }
}

impl SessionStore {
    /// Construct a session store from `STORAGE_URL`.
    ///
    /// Disk-backed storage (`local://`) is not a valid session backend and
    /// falls back to an in-memory store (with a log line noting the fallback).
    pub fn from_storage_url(storage_url: &str) -> Result<Self, AppError> {
        match StorageScheme::parse(storage_url)? {
            StorageScheme::Memory => Ok(Self::default()),
            StorageScheme::Local => {
                info!(
                    "sessions: STORAGE_URL is local://; falling back to in-memory \
                     (disk-backed sessions are not supported)"
                );
                Ok(Self::default())
            }
            StorageScheme::Postgres => {
                #[cfg(feature = "database")]
                {
                    Ok(Self::Database(sqlx::PgPool::connect_lazy(storage_url)?))
                }
                #[cfg(not(feature = "database"))]
                {
                    Err(crate::utils::database_disabled_error())
                }
            }
        }
    }

    /// Returns the session for `token` if it exists and is still valid.
    ///
    /// A storage failure is surfaced as `Err` rather than mapped to "no
    /// session", so a database outage trips the maintenance gate instead of
    /// looking like a signed-out user. A valid-but-expired session is dropped
    /// and reported as `Ok(None)`.
    pub async fn get(&self, token: &str) -> Result<Option<Session>, AppError> {
        // The cookie carries the raw token; the store is keyed by its hash.
        let token_hash = hash_token(token);
        let Some(session) = self.load(&token_hash).await? else {
            return Ok(None);
        };
        if session.is_expired() {
            self.drop_token(&token_hash).await;
            return Ok(None);
        }
        Ok(Some(session))
    }

    /// Like [`Self::get`], but accepts an optional token; `None` yields
    /// `Ok(None)` without touching the store.
    pub async fn get_existing(&self, token: Option<&str>) -> Result<Option<Session>, AppError> {
        match token {
            Some(token) => self.get(token).await,
            None => Ok(None),
        }
    }

    /// Inserts or updates a session in the store.
    pub async fn insert(&self, session: Session) {
        match self {
            SessionStore::InMemory(inner) => {
                inner
                    .write()
                    .insert(session.token_hash().to_string(), session);
            }
            #[cfg(feature = "database")]
            SessionStore::Database(pool) => {
                if let Err(err) = session_db::upsert(pool, &session).await {
                    error!("failed to persist session: {err}");
                }
            }
        }
    }

    /// Persists changes to a stored session. Unlike [`Self::insert`] this never
    /// re-creates the entry, so an in-flight request holding a clone cannot undo
    /// a concurrent logout. Use [`Self::insert`] only for a new session.
    ///
    /// The identity is written as one value (within its role: an update whose
    /// identity changes role is refused, mirroring the database backend —
    /// role changes go through session establishment, never through mutation);
    /// `created_at` and `user_agent_hash` stay fixed at login.
    pub async fn update(&self, session: &Session) {
        match self {
            SessionStore::InMemory(inner) => {
                if let Some(existing) = inner.write().get_mut(session.token_hash()) {
                    if existing.user.scope() != session.user.scope() {
                        tracing::error!("refusing session update that changes the session's role");
                        return;
                    }
                    existing.csrf_token = session.csrf_token.clone();
                    existing.last_activity = session.last_activity;
                    existing.user = session.user.clone();
                    existing.locale = session.locale;
                }
            }
            #[cfg(feature = "database")]
            SessionStore::Database(pool) => {
                if let Err(err) = session_db::update(pool, session).await {
                    error!("failed to update session: {err}");
                }
            }
        }
    }

    /// Refreshes `last_activity` of an existing session. Unlike [`Self::insert`]
    /// this never re-creates the row, so a concurrent logout stays terminal.
    pub async fn touch(&self, session: &Session) {
        match self {
            SessionStore::InMemory(inner) => {
                if let Some(existing) = inner.write().get_mut(session.token_hash()) {
                    existing.last_activity = session.last_activity;
                }
            }
            #[cfg(feature = "database")]
            SessionStore::Database(pool) => {
                if let Err(err) =
                    session_db::touch(pool, session.token_hash(), session.last_activity).await
                {
                    error!("failed to touch session: {err}");
                }
            }
        }
    }

    /// Removes a session by its token. Returns the removed session, if any.
    /// A failed delete is logged: a session that outlives the request that ended
    /// it must be visible somewhere.
    pub async fn remove(&self, token: &str) -> Option<Session> {
        let token_hash = hash_token(token);
        match self {
            SessionStore::InMemory(inner) => inner.write().remove(&token_hash),
            #[cfg(feature = "database")]
            SessionStore::Database(pool) => {
                let session = session_db::load(pool, &token_hash).await.ok().flatten();
                if let Err(err) = session_db::delete(pool, &token_hash).await {
                    error!("failed to remove session, it stays valid until it expires: {err}");
                }
                session
            }
        }
    }

    /// Removes all expired sessions from the store.
    pub async fn cleanup_expired(&self) {
        match self {
            SessionStore::InMemory(inner) => {
                let mut sessions = inner.write();
                sessions.retain(|_, session| !session.is_expired());
            }
            #[cfg(feature = "database")]
            SessionStore::Database(pool) => {
                if let Err(err) = session_db::cleanup_expired(pool).await {
                    error!("failed to cleanup expired sessions: {err}");
                }
            }
        }
    }

    async fn load(&self, token_hash: &str) -> Result<Option<Session>, AppError> {
        match self {
            SessionStore::InMemory(inner) => Ok(inner.read().get(token_hash).cloned()),
            #[cfg(feature = "database")]
            SessionStore::Database(pool) => session_db::load(pool, token_hash).await,
        }
    }

    async fn drop_token(&self, token_hash: &str) {
        match self {
            SessionStore::InMemory(inner) => {
                inner.write().remove(token_hash);
            }
            #[cfg(feature = "database")]
            SessionStore::Database(pool) => {
                if let Err(err) = session_db::delete(pool, token_hash).await {
                    error!("failed to drop expired session: {err}");
                }
            }
        }
    }
}

/// Periodically evict expired sessions, bounding how long expired rows linger
/// beyond the lazy per-lookup cleanup (matters for the Postgres backend).
pub async fn run_session_sweeper(sessions: SessionStore) {
    const SWEEP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5 * 60);

    let mut ticker = tokio::time::interval(SWEEP_INTERVAL);
    loop {
        ticker.tick().await;
        sessions.cleanup_expired().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::session::session_idle_timeout;
    use chrono::{Duration, Utc};

    /// Returns an existing session by token.
    #[tokio::test]
    async fn get_existing_returns_session() {
        let store = SessionStore::default();
        let session = Session::new_test();
        let token = session.token_string();
        store.insert(session.clone()).await;

        let loaded = store
            .get_existing(Some(&token))
            .await
            .expect("load session");

        assert_eq!(loaded, Some(session));
    }

    /// `touch` refreshes activity but never re-creates a removed session, so
    /// a logout that races an in-flight request stays terminal.
    #[tokio::test]
    async fn touch_does_not_resurrect_removed_session() {
        let store = SessionStore::default();
        let mut session = Session::new_test();
        let token = session.token_string();
        store.insert(session.clone()).await;

        session.last_activity = Utc::now() + Duration::minutes(5);
        store.touch(&session).await;
        let refreshed = store.get(&token).await.expect("load").expect("present");
        assert_eq!(refreshed.last_activity, session.last_activity);

        store.remove(&token).await;
        store.touch(&session).await;
        assert!(store.get(&token).await.expect("load").is_none());
    }

    /// `update` writes a session's mutable fields but, like `touch`, never
    /// re-creates a removed one: a request that still holds a clone of a
    /// logged-out session cannot bring it back.
    #[tokio::test]
    async fn update_persists_changes_without_resurrecting_a_removed_session() {
        let store = SessionStore::default();
        let mut session = Session::new_test();
        let token = session.token_string();
        store.insert(session.clone()).await;

        let old_csrf = session.csrf_token().to_string();
        session.rotate_csrf_token();
        session.set_test_election(crate::ElectionConfig::EK27);
        store.update(&session).await;

        let updated = store.get(&token).await.expect("load").expect("present");
        assert_eq!(
            updated.user.election(),
            Some(crate::ElectionConfig::EK27),
            "the identity must be written as a whole"
        );
        assert!(!updated.csrf_matches(&old_csrf));

        store.remove(&token).await;
        store.update(&session).await;
        assert!(store.get(&token).await.expect("load").is_none());
    }

    /// The login-time facts of a session are not writable after the fact, and
    /// an update can never change the session's role: role changes go through
    /// session establishment (a new session), not mutation.
    #[tokio::test]
    async fn update_leaves_login_time_fields_and_role_alone() {
        let store = SessionStore::default();
        let mut session = Session::new_test();
        session.set_user_agent_hash("ua-hash".to_string());
        let token = session.token_string();
        let created_at = session.created_at;
        let original_user = session.user.clone();
        store.insert(session.clone()).await;

        session.created_at = created_at + Duration::hours(1);
        session.set_user_agent_hash("other-ua".to_string());
        // Attempt a role escalation through the mutable-session path.
        session.user = crate::SessionUser::CentralElectoralCommittee {
            user: crate::CsbUser::new_test(),
            election: crate::ElectionConfig::EK27,
            paper_correction_stream_id: None,
        };
        store.update(&session).await;

        let stored = store.get(&token).await.expect("load").expect("present");
        assert_eq!(stored.created_at, created_at);
        assert_eq!(stored.user_agent_hash.as_deref(), Some("ua-hash"));
        assert_eq!(
            stored.user, original_user,
            "role escalation must be refused"
        );
    }

    /// Removes expired sessions on lookup.
    #[tokio::test]
    async fn get_removes_expired_sessions() {
        let store = SessionStore::default();
        let mut session = Session::new_test();
        session.last_activity = Utc::now() - session_idle_timeout() - Duration::seconds(1);
        let token = session.token_string();
        store.insert(session).await;

        let loaded = store.get(&token).await.expect("load session");

        assert!(loaded.is_none());
    }

    /// Removes expired sessions in bulk cleanup.
    #[tokio::test]
    async fn cleanup_expired_removes_stale_sessions() {
        let store = SessionStore::default();
        let mut expired = Session::new_test();
        expired.last_activity = Utc::now() - session_idle_timeout() - Duration::seconds(1);
        let active = Session::new_test();
        let expired_token = expired.token_string();
        let active_token = active.token_string();
        store.insert(expired).await;
        store.insert(active).await;

        store.cleanup_expired().await;

        assert!(store.get(&expired_token).await.expect("load").is_none());
        assert!(store.get(&active_token).await.expect("load").is_some());
    }

    #[test]
    fn from_storage_url_memory_is_in_memory() {
        let store = SessionStore::from_storage_url("memory://").unwrap();
        assert!(matches!(store, SessionStore::InMemory(_)));
    }

    #[test]
    fn from_storage_url_local_falls_back_to_memory() {
        // local:// with any path — disk is not a valid session backend, so we
        // expect an in-memory fallback regardless of directory presence.
        let store = SessionStore::from_storage_url("local:///does-not-matter").unwrap();
        assert!(matches!(store, SessionStore::InMemory(_)));
    }

    #[test]
    fn from_storage_url_rejects_unsupported_scheme() {
        match SessionStore::from_storage_url("s3://bucket") {
            Err(AppError::ConfigLoadError(_)) => {}
            Ok(_) => panic!("expected an error for unsupported scheme"),
            Err(err) => panic!("unexpected error variant: {err:?}"),
        }
    }

    /// Postgres round-trip: hash-at-rest, identity/UA persistence, `created_at`
    /// fixed across a touch, and `remove`.
    #[cfg(feature = "database")]
    #[cfg_attr(not(feature = "db-tests"), ignore = "requires database")]
    #[tokio::test]
    async fn database_backend_roundtrips_session() -> Result<(), AppError> {
        crate::test_db::with_pool(|pool| async move {
            crate::store::database::migrate(&pool).await?;
            let store = SessionStore::Database(pool);

            let stream_id = crate::StreamId::new();
            let mut session = crate::Session::for_political_group(
                stream_id,
                "nid-1".to_string(),
                None,
                crate::Locale::default(),
            );
            session.set_user_agent_hash("ua-hash-xyz".to_string());
            let token = session.token_string();
            let created_at = session.created_at;
            let original_user = session.user.clone();
            store.insert(session).await;

            // Look up by the raw cookie token (hashed internally).
            let loaded = store.get(&token).await?.expect("session present");
            assert_eq!(loaded.user, original_user);
            assert_eq!(loaded.user_agent_hash.as_deref(), Some("ua-hash-xyz"));
            assert_eq!(loaded.token_hash(), hash_token(&token));
            assert!(
                loaded.reveal_token().is_none(),
                "a reloaded session must not carry its raw token"
            );

            // A touch must not reset created_at, even carrying a bogus one.
            let mut touched = loaded;
            touched.last_activity = Utc::now();
            touched.created_at = created_at + Duration::hours(1);
            store.insert(touched).await;
            let reloaded = store.get(&token).await?.expect("still present");
            // Tolerance: Postgres TIMESTAMPTZ truncates the Rust DateTime's nanoseconds.
            assert!(
                (reloaded.created_at - created_at).abs() < Duration::milliseconds(1),
                "created_at must be fixed across touches (got {}, expected ~{created_at})",
                reloaded.created_at,
            );

            // `update` writes the identity within its role and the CSRF token.
            let mut changed = reloaded.clone();
            changed.set_test_election(crate::ElectionConfig::EK27);
            changed.rotate_csrf_token();
            store.update(&changed).await;
            let updated = store.get(&token).await?.expect("still present");
            assert_eq!(updated.user.election(), Some(crate::ElectionConfig::EK27));
            assert!(updated.csrf_matches(&changed.csrf_token().0));

            // An update that changes the session's role is refused.
            let mut escalated = updated.clone();
            escalated.user = crate::SessionUser::CentralElectoralCommittee {
                user: crate::CsbUser::new_test(),
                election: crate::ElectionConfig::EK27,
                paper_correction_stream_id: None,
            };
            store.update(&escalated).await;
            let unchanged = store.get(&token).await?.expect("still present");
            assert_eq!(
                unchanged.user, changed.user,
                "role escalation must be refused"
            );

            store.remove(&token).await;
            assert!(store.get(&token).await?.is_none());

            // A touch or an update after logout must not resurrect the session row.
            store.touch(&reloaded).await;
            store.update(&reloaded).await;
            assert!(store.get(&token).await?.is_none());
            Ok(())
        })
        .await
    }

    /// A stored row whose identity does not parse is dropped on load (fail
    /// closed), never mapped to a default identity.
    #[cfg(feature = "database")]
    #[cfg_attr(not(feature = "db-tests"), ignore = "requires database")]
    #[tokio::test]
    async fn database_backend_drops_unreadable_identity() -> Result<(), AppError> {
        crate::test_db::with_pool(|pool| async move {
            crate::store::database::migrate(&pool).await?;

            let session = Session::new_test();
            let token = session.token_string();
            let store = SessionStore::Database(pool.clone());
            store.insert(session).await;

            sqlx::query("UPDATE sessions SET identity = '{\"Unknown\":{}}'::jsonb")
                .execute(&pool)
                .await?;

            assert!(store.get(&token).await?.is_none());
            // The corrupt row itself is deleted, not retried forever.
            let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
                .fetch_one(&pool)
                .await?;
            assert_eq!(remaining, 0);
            Ok(())
        })
        .await
    }
}
