//! Storage for passkey accounts and their registered passkeys. Mirrors
//! [`crate::PendingRequestStore`]: in-memory by default, Postgres when
//! `STORAGE_URL` is `postgres://`. Public keys only; nothing here is secret,
//! but the rows decide who may log in, so writes are audited by the callers.

use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use webauthn_rs::prelude::{CredentialID, Passkey};

use crate::{
    AppError, CsbUser, PasskeyAccountId, PasskeyAccountName, PasskeyId, PasskeyLabel,
    utils::StorageScheme,
};

#[cfg(feature = "database")]
use super::db;

/// A passkey account: the WebAuthn user handle, its login name, and who
/// created it (recorded on the account, not just in the audit log, so the
/// management page can show it).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasskeyAccount {
    pub id: PasskeyAccountId,
    pub name: PasskeyAccountName,
    pub created_at: DateTime<Utc>,
    pub created_by: CsbUser,
}

impl PasskeyAccount {
    pub fn new(id: PasskeyAccountId, name: PasskeyAccountName, created_by: CsbUser) -> Self {
        Self {
            id,
            name,
            created_at: Utc::now(),
            created_by,
        }
    }
}

/// A registered passkey: the webauthn-rs credential (public key, counter,
/// backup flags) under a member-chosen label.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPasskey {
    pub id: PasskeyId,
    pub account_id: PasskeyAccountId,
    pub label: PasskeyLabel,
    pub passkey: Passkey,
    pub created_at: DateTime<Utc>,
}

impl StoredPasskey {
    pub fn new(account_id: PasskeyAccountId, label: PasskeyLabel, passkey: Passkey) -> Self {
        Self {
            id: PasskeyId::new(),
            account_id,
            label,
            passkey,
            created_at: Utc::now(),
        }
    }

    pub fn credential_id(&self) -> &CredentialID {
        self.passkey.cred_id()
    }
}

/// Process-local storage behind [`CsbPasskeyStore::InMemory`].
#[derive(Default)]
pub struct InMemoryPasskeys {
    accounts: HashMap<PasskeyAccountId, PasskeyAccount>,
    passkeys: HashMap<PasskeyId, StoredPasskey>,
}

/// Passkey storage backend.
#[derive(Clone)]
pub enum CsbPasskeyStore {
    /// Process-local, in-memory storage. Cleared on restart.
    InMemory(Arc<RwLock<InMemoryPasskeys>>),
    /// Postgres-backed storage, shared across instances.
    #[cfg(feature = "database")]
    Database(sqlx::PgPool),
}

impl Default for CsbPasskeyStore {
    fn default() -> Self {
        Self::InMemory(Arc::default())
    }
}

impl CsbPasskeyStore {
    /// Construct from `STORAGE_URL`; same scheme rules as
    /// [`crate::SessionStore`], with disk falling back to in-memory.
    pub fn from_storage_url(storage_url: &str) -> Result<Self, AppError> {
        match StorageScheme::parse(storage_url)? {
            StorageScheme::Memory | StorageScheme::Local => Ok(Self::default()),
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

    /// The account with this name, compared case-insensitively.
    pub async fn find_account_by_name(
        &self,
        name: &PasskeyAccountName,
    ) -> Result<Option<PasskeyAccount>, AppError> {
        match self {
            Self::InMemory(inner) => {
                let normalized = name.normalized();
                Ok(inner
                    .read()
                    .accounts
                    .values()
                    .find(|account| account.name.normalized() == normalized)
                    .cloned())
            }
            #[cfg(feature = "database")]
            Self::Database(pool) => db::find_account_by_name(pool, name).await,
        }
    }

    pub async fn find_account(
        &self,
        id: PasskeyAccountId,
    ) -> Result<Option<PasskeyAccount>, AppError> {
        match self {
            Self::InMemory(inner) => Ok(inner.read().accounts.get(&id).cloned()),
            #[cfg(feature = "database")]
            Self::Database(pool) => db::find_account(pool, id).await,
        }
    }

    /// Every account with its passkeys, by name; passkeys oldest first.
    pub async fn list_accounts(
        &self,
    ) -> Result<Vec<(PasskeyAccount, Vec<StoredPasskey>)>, AppError> {
        match self {
            Self::InMemory(inner) => {
                let inner = inner.read();
                let mut accounts: Vec<_> = inner
                    .accounts
                    .values()
                    .map(|account| {
                        let mut passkeys: Vec<_> = inner
                            .passkeys
                            .values()
                            .filter(|passkey| passkey.account_id == account.id)
                            .cloned()
                            .collect();
                        passkeys.sort_by_key(|passkey| passkey.created_at);
                        (account.clone(), passkeys)
                    })
                    .collect();
                accounts.sort_by_key(|(account, _)| account.name.normalized());
                Ok(accounts)
            }
            #[cfg(feature = "database")]
            Self::Database(pool) => db::list_accounts(pool).await,
        }
    }

    /// The account's passkeys, oldest first.
    pub async fn passkeys_for_account(
        &self,
        account_id: PasskeyAccountId,
    ) -> Result<Vec<StoredPasskey>, AppError> {
        match self {
            Self::InMemory(inner) => {
                let mut passkeys: Vec<_> = inner
                    .read()
                    .passkeys
                    .values()
                    .filter(|passkey| passkey.account_id == account_id)
                    .cloned()
                    .collect();
                passkeys.sort_by_key(|passkey| passkey.created_at);
                Ok(passkeys)
            }
            #[cfg(feature = "database")]
            Self::Database(pool) => db::passkeys_for_account(pool, account_id).await,
        }
    }

    /// Fails with [`AppError::Conflict`] when the name (case-insensitively)
    /// or the id is already taken.
    pub async fn create_account(&self, account: &PasskeyAccount) -> Result<(), AppError> {
        match self {
            Self::InMemory(inner) => {
                let mut inner = inner.write();
                let normalized = account.name.normalized();
                let taken = inner.accounts.contains_key(&account.id)
                    || inner
                        .accounts
                        .values()
                        .any(|existing| existing.name.normalized() == normalized);
                if taken {
                    return Err(AppError::Conflict);
                }
                inner.accounts.insert(account.id, account.clone());
                Ok(())
            }
            #[cfg(feature = "database")]
            Self::Database(pool) => db::create_account(pool, account).await,
        }
    }

    /// Fails with [`AppError::Conflict`] when the credential id is already
    /// registered, and with [`AppError::GenericNotFound`] when the account
    /// does not exist.
    pub async fn insert_passkey(&self, passkey: &StoredPasskey) -> Result<(), AppError> {
        match self {
            Self::InMemory(inner) => {
                let mut inner = inner.write();
                if !inner.accounts.contains_key(&passkey.account_id) {
                    return Err(AppError::GenericNotFound);
                }
                let duplicate = inner.passkeys.values().any(|existing| {
                    existing.id == passkey.id || existing.credential_id() == passkey.credential_id()
                });
                if duplicate {
                    return Err(AppError::Conflict);
                }
                inner.passkeys.insert(passkey.id, passkey.clone());
                Ok(())
            }
            #[cfg(feature = "database")]
            Self::Database(pool) => db::insert_passkey(pool, passkey).await,
        }
    }

    /// Persist the credential after a login updated its counter or flags.
    pub async fn update_passkey(&self, id: PasskeyId, passkey: &Passkey) -> Result<(), AppError> {
        match self {
            Self::InMemory(inner) => {
                if let Some(stored) = inner.write().passkeys.get_mut(&id) {
                    stored.passkey = passkey.clone();
                }
                Ok(())
            }
            #[cfg(feature = "database")]
            Self::Database(pool) => db::update_passkey(pool, id, passkey).await,
        }
    }

    pub async fn find_passkey(&self, id: PasskeyId) -> Result<Option<StoredPasskey>, AppError> {
        match self {
            Self::InMemory(inner) => Ok(inner.read().passkeys.get(&id).cloned()),
            #[cfg(feature = "database")]
            Self::Database(pool) => db::find_passkey(pool, id).await,
        }
    }

    /// Removes the passkey and returns it, or `None` when it was already gone.
    pub async fn delete_passkey(&self, id: PasskeyId) -> Result<Option<StoredPasskey>, AppError> {
        match self {
            Self::InMemory(inner) => Ok(inner.write().passkeys.remove(&id)),
            #[cfg(feature = "database")]
            Self::Database(pool) => db::delete_passkey(pool, id).await,
        }
    }

    /// Removes the account with all its passkeys and returns it, or `None`
    /// when it was already gone.
    pub async fn delete_account(
        &self,
        id: PasskeyAccountId,
    ) -> Result<Option<PasskeyAccount>, AppError> {
        match self {
            Self::InMemory(inner) => {
                let mut inner = inner.write();
                let account = inner.accounts.remove(&id);
                if account.is_some() {
                    inner.passkeys.retain(|_, passkey| passkey.account_id != id);
                }
                Ok(account)
            }
            #[cfg(feature = "database")]
            Self::Database(pool) => db::delete_account(pool, id).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::passkey::test_support::{test_account, test_passkey};

    #[tokio::test]
    async fn accounts_are_unique_by_name_ignoring_case() {
        let store = CsbPasskeyStore::default();
        let account = test_account("Jan de Vries");
        store.create_account(&account).await.unwrap();

        let clash = test_account("jan DE vries");
        assert!(matches!(
            store.create_account(&clash).await,
            Err(AppError::Conflict)
        ));

        let found = store
            .find_account_by_name(&"JAN DE VRIES".parse().unwrap())
            .await
            .unwrap()
            .expect("found by name");
        assert_eq!(found.id, account.id);
        assert!(
            store
                .find_account_by_name(&"Piet".parse().unwrap())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn passkeys_belong_to_an_account_and_are_removed_with_it() {
        let store = CsbPasskeyStore::default();
        let account = test_account("Jan");
        store.create_account(&account).await.unwrap();

        let first = StoredPasskey::new(account.id, "Key 1".parse().unwrap(), test_passkey(1));
        let second = StoredPasskey::new(account.id, "Key 2".parse().unwrap(), test_passkey(2));
        store.insert_passkey(&first).await.unwrap();
        store.insert_passkey(&second).await.unwrap();

        // Same credential id again is a conflict, an unknown account a 404.
        let duplicate = StoredPasskey::new(account.id, "Copy".parse().unwrap(), test_passkey(1));
        assert!(matches!(
            store.insert_passkey(&duplicate).await,
            Err(AppError::Conflict)
        ));
        let orphan = StoredPasskey::new(
            PasskeyAccountId::new(),
            "Orphan".parse().unwrap(),
            test_passkey(3),
        );
        assert!(matches!(
            store.insert_passkey(&orphan).await,
            Err(AppError::GenericNotFound)
        ));

        let listed = store.passkeys_for_account(account.id).await.unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, first.id);

        let accounts = store.list_accounts().await.unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].1.len(), 2);

        let removed = store.delete_passkey(first.id).await.unwrap();
        assert_eq!(removed.map(|p| p.id), Some(first.id));
        assert!(store.delete_passkey(first.id).await.unwrap().is_none());

        let removed = store.delete_account(account.id).await.unwrap();
        assert_eq!(removed.map(|a| a.id), Some(account.id));
        assert!(store.find_passkey(second.id).await.unwrap().is_none());
        assert!(store.list_accounts().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn list_sorts_accounts_by_name() {
        let store = CsbPasskeyStore::default();
        for name in ["piet", "Anna", "Jan"] {
            store.create_account(&test_account(name)).await.unwrap();
        }
        let names: Vec<_> = store
            .list_accounts()
            .await
            .unwrap()
            .into_iter()
            .map(|(account, _)| account.name.to_string())
            .collect();
        assert_eq!(names, ["Anna", "Jan", "piet"]);
    }

    #[test]
    fn from_storage_url_memory_and_local_are_in_memory() {
        for url in ["memory://", "local:///whatever"] {
            let store = CsbPasskeyStore::from_storage_url(url).unwrap();
            assert!(matches!(store, CsbPasskeyStore::InMemory(_)), "{url}");
        }
    }

    #[test]
    fn from_storage_url_rejects_unsupported_scheme() {
        assert!(CsbPasskeyStore::from_storage_url("ftp://x").is_err());
    }
}
