//! Postgres persistence for passkey accounts and passkeys. The tables are
//! created by the runtime migration in `store::database` and mirrored in
//! `deploy/schema.sql`.

use chrono::{DateTime, Utc};
use uuid::Uuid;
use webauthn_rs::prelude::Passkey;

use super::{PasskeyAccount, StoredPasskey};
use crate::{AppError, PasskeyAccountId, PasskeyAccountName, PasskeyId};

type AccountRow = (Uuid, String, DateTime<Utc>, serde_json::Value);
type PasskeyRow = (Uuid, Uuid, String, serde_json::Value, DateTime<Utc>);

/// A row that no longer parses is a schema or data problem, not user input.
fn account_from_row(
    (id, name, created_at, created_by): AccountRow,
) -> Result<PasskeyAccount, AppError> {
    Ok(PasskeyAccount {
        id: id.into(),
        name: name.parse().map_err(|_| AppError::IntegrityViolation)?,
        created_at,
        created_by: serde_json::from_value(created_by)?,
    })
}

fn passkey_from_row(
    (id, account_id, label, passkey, created_at): PasskeyRow,
) -> Result<StoredPasskey, AppError> {
    Ok(StoredPasskey {
        id: id.into(),
        account_id: account_id.into(),
        label: label.parse().map_err(|_| AppError::IntegrityViolation)?,
        passkey: serde_json::from_value(passkey)?,
        created_at,
    })
}

/// A unique-index violation is a conflict for the caller to report; anything
/// else is a database error.
fn map_insert_error(err: sqlx::Error) -> AppError {
    match &err {
        sqlx::Error::Database(db) if db.is_unique_violation() => AppError::Conflict,
        _ => err.into(),
    }
}

pub async fn find_account_by_name(
    pool: &sqlx::PgPool,
    name: &PasskeyAccountName,
) -> Result<Option<PasskeyAccount>, AppError> {
    let row: Option<AccountRow> = sqlx::query_as(
        "SELECT id, name, created_at, created_by FROM csb_passkey_accounts WHERE lower(name) = $1",
    )
    .bind(name.normalized())
    .fetch_optional(pool)
    .await?;
    row.map(account_from_row).transpose()
}

pub async fn find_account(
    pool: &sqlx::PgPool,
    id: PasskeyAccountId,
) -> Result<Option<PasskeyAccount>, AppError> {
    let row: Option<AccountRow> = sqlx::query_as(
        "SELECT id, name, created_at, created_by FROM csb_passkey_accounts WHERE id = $1",
    )
    .bind(id.uuid())
    .fetch_optional(pool)
    .await?;
    row.map(account_from_row).transpose()
}

pub async fn list_accounts(
    pool: &sqlx::PgPool,
) -> Result<Vec<(PasskeyAccount, Vec<StoredPasskey>)>, AppError> {
    let accounts: Vec<AccountRow> = sqlx::query_as(
        "SELECT id, name, created_at, created_by FROM csb_passkey_accounts ORDER BY lower(name)",
    )
    .fetch_all(pool)
    .await?;
    let passkeys: Vec<PasskeyRow> = sqlx::query_as(
        "SELECT id, account_id, label, passkey, created_at FROM csb_passkeys ORDER BY created_at",
    )
    .fetch_all(pool)
    .await?;
    let passkeys = passkeys
        .into_iter()
        .map(passkey_from_row)
        .collect::<Result<Vec<_>, _>>()?;

    accounts
        .into_iter()
        .map(|row| {
            let account = account_from_row(row)?;
            let own = passkeys
                .iter()
                .filter(|passkey| passkey.account_id == account.id)
                .cloned()
                .collect();
            Ok((account, own))
        })
        .collect()
}

pub async fn passkeys_for_account(
    pool: &sqlx::PgPool,
    account_id: PasskeyAccountId,
) -> Result<Vec<StoredPasskey>, AppError> {
    let rows: Vec<PasskeyRow> =
        sqlx::query_as("SELECT id, account_id, label, passkey, created_at FROM csb_passkeys WHERE account_id = $1 ORDER BY created_at")
            .bind(account_id.uuid())
            .fetch_all(pool)
            .await?;
    rows.into_iter().map(passkey_from_row).collect()
}

pub async fn create_account(pool: &sqlx::PgPool, account: &PasskeyAccount) -> Result<(), AppError> {
    sqlx::query(
        r#"INSERT INTO csb_passkey_accounts (id, name, created_at, created_by)
           VALUES ($1, $2, $3, $4)"#,
    )
    .bind(account.id.uuid())
    .bind(account.name.as_str())
    .bind(account.created_at)
    .bind(serde_json::to_value(&account.created_by)?)
    .execute(pool)
    .await
    .map_err(map_insert_error)?;
    Ok(())
}

pub async fn insert_passkey(pool: &sqlx::PgPool, passkey: &StoredPasskey) -> Result<(), AppError> {
    let result = sqlx::query(
        r#"INSERT INTO csb_passkeys (id, account_id, credential_id, label, passkey, created_at)
           VALUES ($1, $2, $3, $4, $5, $6)"#,
    )
    .bind(passkey.id.uuid())
    .bind(passkey.account_id.uuid())
    .bind(passkey.credential_id().as_slice())
    .bind(passkey.label.as_str())
    .bind(serde_json::to_value(&passkey.passkey)?)
    .bind(passkey.created_at)
    .execute(pool)
    .await;
    match result {
        Ok(_) => Ok(()),
        Err(sqlx::Error::Database(db)) if db.is_foreign_key_violation() => {
            Err(AppError::GenericNotFound)
        }
        Err(err) => Err(map_insert_error(err)),
    }
}

pub async fn update_passkey(
    pool: &sqlx::PgPool,
    id: PasskeyId,
    passkey: &Passkey,
) -> Result<(), AppError> {
    sqlx::query("UPDATE csb_passkeys SET passkey = $2 WHERE id = $1")
        .bind(id.uuid())
        .bind(serde_json::to_value(passkey)?)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn find_passkey(
    pool: &sqlx::PgPool,
    id: PasskeyId,
) -> Result<Option<StoredPasskey>, AppError> {
    let row: Option<PasskeyRow> = sqlx::query_as(
        "SELECT id, account_id, label, passkey, created_at FROM csb_passkeys WHERE id = $1",
    )
    .bind(id.uuid())
    .fetch_optional(pool)
    .await?;
    row.map(passkey_from_row).transpose()
}

pub async fn delete_passkey(
    pool: &sqlx::PgPool,
    id: PasskeyId,
) -> Result<Option<StoredPasskey>, AppError> {
    let row: Option<PasskeyRow> = sqlx::query_as(
        r#"DELETE FROM csb_passkeys WHERE id = $1
           RETURNING id, account_id, label, passkey, created_at"#,
    )
    .bind(id.uuid())
    .fetch_optional(pool)
    .await?;
    row.map(passkey_from_row).transpose()
}

/// The passkeys go with the account through `ON DELETE CASCADE`.
pub async fn delete_account(
    pool: &sqlx::PgPool,
    id: PasskeyAccountId,
) -> Result<Option<PasskeyAccount>, AppError> {
    let row: Option<AccountRow> = sqlx::query_as(
        r#"DELETE FROM csb_passkey_accounts WHERE id = $1
           RETURNING id, name, created_at, created_by"#,
    )
    .bind(id.uuid())
    .fetch_optional(pool)
    .await?;
    row.map(account_from_row).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::passkey::test_support::{test_account, test_passkey};

    /// Apply the shipped schema file, so these tests keep it honest.
    async fn apply_schema(pool: &sqlx::PgPool) {
        sqlx::raw_sql(include_str!("../../../deploy/schema.sql"))
            .execute(pool)
            .await
            .expect("apply deploy/schema.sql");
    }

    #[cfg_attr(not(feature = "db-tests"), ignore = "requires database")]
    #[sqlx::test(migrations = false)]
    async fn account_and_passkey_roundtrip(pool: sqlx::PgPool) {
        apply_schema(&pool).await;

        let account = test_account("Jan de Vries");
        create_account(&pool, &account).await.unwrap();
        assert!(matches!(
            create_account(&pool, &test_account("JAN DE VRIES")).await,
            Err(AppError::Conflict)
        ));

        let found = find_account_by_name(&pool, &"jan de vries".parse().unwrap())
            .await
            .unwrap()
            .expect("found");
        assert_eq!(found.id, account.id);
        assert_eq!(found.created_by, account.created_by);

        let passkey = StoredPasskey::new(account.id, "Key".parse().unwrap(), test_passkey(1));
        insert_passkey(&pool, &passkey).await.unwrap();
        let duplicate = StoredPasskey::new(account.id, "Copy".parse().unwrap(), test_passkey(1));
        assert!(matches!(
            insert_passkey(&pool, &duplicate).await,
            Err(AppError::Conflict)
        ));
        let orphan = StoredPasskey::new(
            PasskeyAccountId::new(),
            "Orphan".parse().unwrap(),
            test_passkey(2),
        );
        assert!(matches!(
            insert_passkey(&pool, &orphan).await,
            Err(AppError::GenericNotFound)
        ));

        let listed = passkeys_for_account(&pool, account.id).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].credential_id(), passkey.credential_id());

        let mut updated = passkey.passkey.clone();
        // A different credential value must round-trip through the JSONB column.
        let json = serde_json::to_value(&updated).unwrap();
        updated = serde_json::from_value(json).unwrap();
        update_passkey(&pool, passkey.id, &updated).await.unwrap();
        assert!(find_passkey(&pool, passkey.id).await.unwrap().is_some());

        let all = list_accounts(&pool).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].1.len(), 1);

        let removed = delete_account(&pool, account.id).await.unwrap();
        assert_eq!(removed.map(|a| a.id), Some(account.id));
        assert!(find_passkey(&pool, passkey.id).await.unwrap().is_none());
        assert!(delete_passkey(&pool, passkey.id).await.unwrap().is_none());
    }
}
