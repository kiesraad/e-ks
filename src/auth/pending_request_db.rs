//! Postgres-backed persistence for outstanding AuthnRequest IDs (all sqlx usage
//! lives here so the store module stays generic).

#![cfg(feature = "database")]

use auth_service::PENDING_REQUEST_TTL;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;

use crate::AppError;

/// Expiry cutoff: `now - PENDING_REQUEST_TTL` (eID §7.5, 15-minute artifact window).
fn cutoff() -> Timestamp {
    Timestamp::now() - SignedDuration::from_secs(PENDING_REQUEST_TTL.as_secs() as i64)
}

/// Record an outgoing AuthnRequest ID, sweeping expired entries first.
pub async fn register(pool: &sqlx::PgPool, id: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM pending_requests WHERE created_at < $1")
        .bind(cutoff().to_sqlx())
        .execute(pool)
        .await?;

    sqlx::query(
        r#"INSERT INTO pending_requests (id, created_at) VALUES ($1, $2)
           ON CONFLICT (id) DO UPDATE SET created_at = EXCLUDED.created_at"#,
    )
    .bind(id)
    .bind(Timestamp::now().to_sqlx())
    .execute(pool)
    .await?;

    Ok(())
}

/// Consume a pending AuthnRequest ID (eID §7.6.3.5 / §9.7). Single-statement
/// DELETE so check and consume are atomic: concurrent ACS callbacks can't both succeed.
pub async fn consume_if_pending(pool: &sqlx::PgPool, id: &str) -> Result<bool, AppError> {
    let row: Option<(String,)> = sqlx::query_as(
        r#"DELETE FROM pending_requests
           WHERE id = $1 AND created_at >= $2
           RETURNING id"#,
    )
    .bind(id)
    .bind(cutoff().to_sqlx())
    .fetch_optional(pool)
    .await?;

    Ok(row.is_some())
}
