//! Storage for ACME http-01 challenge tokens. Mirrors
//! [`crate::PendingRequestStore`]: in-memory by default, Postgres when
//! `STORAGE_URL` is `postgres://`. Challenge tokens are public by protocol.

use std::{collections::HashMap, sync::Arc};

use parking_lot::RwLock;
use tracing::error;

use crate::{AppError, acme::acme_db, utils::StorageScheme};

/// ACME challenge-token storage backend.
#[derive(Clone)]
pub enum AcmeStore {
    /// Single instance only; cleared on restart.
    InMemory(Arc<RwLock<HashMap<String, String>>>),
    /// Shared across instances.
    Database(sqlx::PgPool),
}

impl Default for AcmeStore {
    fn default() -> Self {
        Self::InMemory(Arc::default())
    }
}

impl AcmeStore {
    /// Construct from `STORAGE_URL`; same scheme rules as
    /// [`crate::SessionStore`], with disk falling back to in-memory.
    pub fn from_storage_url(storage_url: &str) -> Result<Self, AppError> {
        match StorageScheme::parse(storage_url)? {
            StorageScheme::Memory | StorageScheme::Local => Ok(Self::default()),
            StorageScheme::Postgres => Ok(Self::Database(sqlx::PgPool::connect_lazy(storage_url)?)),
        }
    }

    pub async fn put_challenge(
        &self,
        token: &str,
        key_authorization: &str,
    ) -> Result<(), AppError> {
        match self {
            AcmeStore::InMemory(inner) => {
                inner
                    .write()
                    .insert(token.to_string(), key_authorization.to_string());
                Ok(())
            }
            AcmeStore::Database(pool) => {
                acme_db::put_challenge(pool, token, key_authorization).await
            }
        }
    }

    /// Storage errors are logged and fail closed.
    pub async fn find_challenge(&self, token: &str) -> Option<String> {
        match self {
            AcmeStore::InMemory(inner) => inner.read().get(token).cloned(),
            AcmeStore::Database(pool) => match acme_db::find_challenge(pool, token).await {
                Ok(key_authorization) => key_authorization,
                Err(err) => {
                    error!("failed to look up ACME challenge: {err}");
                    None
                }
            },
        }
    }

    /// Best effort: a leftover row is swept on the next renewal.
    pub async fn delete_challenge(&self, token: &str) {
        match self {
            AcmeStore::InMemory(inner) => {
                inner.write().remove(token);
            }
            AcmeStore::Database(pool) => {
                if let Err(err) = acme_db::delete_challenge(pool, token).await {
                    error!("failed to delete ACME challenge: {err}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn in_memory_challenge_roundtrip() {
        let store = AcmeStore::default();
        store.put_challenge("tok", "tok.thumbprint").await.unwrap();

        assert_eq!(
            store.find_challenge("tok").await.as_deref(),
            Some("tok.thumbprint")
        );
        assert_eq!(store.find_challenge("other").await, None);

        store.delete_challenge("tok").await;
        assert_eq!(store.find_challenge("tok").await, None);
    }

    #[test]
    fn from_storage_url_memory_is_in_memory() {
        let store = AcmeStore::from_storage_url("memory://").unwrap();
        assert!(matches!(store, AcmeStore::InMemory(_)));
    }

    #[test]
    fn from_storage_url_local_falls_back_to_memory() {
        let store = AcmeStore::from_storage_url("local:///whatever").unwrap();
        assert!(matches!(store, AcmeStore::InMemory(_)));
    }

    #[test]
    fn from_storage_url_rejects_unsupported_scheme() {
        assert!(matches!(
            AcmeStore::from_storage_url("s3://bucket"),
            Err(AppError::ConfigLoadError(_))
        ));
    }

    #[cfg_attr(not(feature = "db-tests"), ignore = "requires database")]
    #[sqlx::test(migrations = false)]
    async fn database_challenge_roundtrip(pool: sqlx::PgPool) {
        sqlx::raw_sql(include_str!("../../deploy/schema.sql"))
            .execute(&pool)
            .await
            .expect("apply deploy/schema.sql");
        let store = AcmeStore::Database(pool);

        store.put_challenge("tok", "tok.thumbprint").await.unwrap();
        assert_eq!(
            store.find_challenge("tok").await.as_deref(),
            Some("tok.thumbprint")
        );

        store.delete_challenge("tok").await;
        assert_eq!(store.find_challenge("tok").await, None);
    }

    #[tokio::test]
    async fn database_errors_fail_closed() {
        // Nothing listens on port 1, so every query errors. The connection is
        // refused at once; the timeout only bounds sqlx's retry loop.
        let pool = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_millis(50))
            .connect_lazy("postgres://nobody@127.0.0.1:1/nothing")
            .unwrap();
        let store = AcmeStore::Database(pool);

        assert!(store.put_challenge("tok", "tok.thumbprint").await.is_err());
        assert_eq!(store.find_challenge("tok").await, None);
        store.delete_challenge("tok").await;
    }
}
