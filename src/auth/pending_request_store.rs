//! Storage for outstanding AuthnRequest IDs (`InResponseTo` replay check,
//! eID §9.7). Mirrors [`crate::SessionStore`]: in-memory by default, Postgres
//! when `STORAGE_URL` is `postgres://` (required for multi-instance deploys).

use auth_service::PendingRequests;

use crate::{AppError, utils::StorageScheme};

#[cfg(feature = "database")]
use crate::auth::pending_request_db;
#[cfg(feature = "database")]
use tracing::error;

/// Pending-request storage backend.
#[derive(Clone)]
pub enum PendingRequestStore {
    /// Process-local, in-memory storage. Cleared on restart.
    InMemory(PendingRequests),
    /// Postgres-backed storage, shared across instances.
    #[cfg(feature = "database")]
    Database(sqlx::PgPool),
}

impl Default for PendingRequestStore {
    fn default() -> Self {
        Self::InMemory(PendingRequests::default())
    }
}

impl PendingRequestStore {
    /// Construct from `STORAGE_URL`. Same scheme rules as [`crate::SessionStore`].
    pub fn from_storage_url(storage_url: &str) -> Result<Self, AppError> {
        match StorageScheme::parse(storage_url)? {
            StorageScheme::Memory => Ok(Self::default()),
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

    /// Record an outgoing AuthnRequest ID for the later `InResponseTo` check.
    pub async fn register(&self, id: String) {
        match self {
            PendingRequestStore::InMemory(inner) => inner.register(id),
            #[cfg(feature = "database")]
            PendingRequestStore::Database(pool) => {
                if let Err(err) = pending_request_db::register(pool, &id).await {
                    error!("failed to persist pending AuthnRequest: {err}");
                }
            }
        }
    }

    /// Consume a pending AuthnRequest ID (eID §7.6.3.5 / §9.7). Returns `true`
    /// once for a valid pending ID. Storage errors fail closed (`false`).
    pub async fn consume_if_pending(&self, id: &str) -> bool {
        match self {
            PendingRequestStore::InMemory(inner) => inner.consume_if_pending(id),
            #[cfg(feature = "database")]
            PendingRequestStore::Database(pool) => {
                match pending_request_db::consume_if_pending(pool, id).await {
                    Ok(matched) => matched,
                    Err(err) => {
                        error!("failed to consume pending AuthnRequest: {err}");
                        false
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn in_memory_register_and_consume_roundtrip() {
        let store = PendingRequestStore::default();
        store.register("req-1".to_string()).await;
        store.register("req-2".to_string()).await;

        assert!(store.consume_if_pending("req-1").await);
        assert!(!store.consume_if_pending("req-1").await); // replayed ID rejected
        assert!(!store.consume_if_pending("never").await); // unknown ID rejected
        assert!(store.consume_if_pending("req-2").await); // other ID remains unaffected
    }

    #[test]
    fn from_storage_url_memory_is_in_memory() {
        let store = PendingRequestStore::from_storage_url("memory://").unwrap();
        assert!(matches!(store, PendingRequestStore::InMemory(_)));
    }

    #[test]
    fn from_storage_url_rejects_unsupported_scheme() {
        match PendingRequestStore::from_storage_url("s3://bucket") {
            Err(AppError::ConfigLoadError(_)) => {}
            Ok(_) => panic!("expected an error for unsupported scheme"),
            Err(err) => panic!("unexpected error variant: {err:?}"),
        }
    }
}
