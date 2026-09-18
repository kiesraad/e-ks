//! Postgres persistence for ACME challenge tokens.
//! Requires the manually applied `deploy/schema.sql`.

use chrono::{DateTime, Duration, Utc};

use crate::AppError;

/// Expiry cutoff for challenge rows; an hour covers CA retries.
fn cutoff() -> DateTime<Utc> {
    Utc::now() - Duration::hours(1)
}

/// Record a challenge token, sweeping expired rows first.
pub async fn put_challenge(
    pool: &sqlx::PgPool,
    token: &str,
    key_authorization: &str,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM acme_challenges WHERE created_at < $1")
        .bind(cutoff())
        .execute(pool)
        .await?;

    sqlx::query(
        r#"INSERT INTO acme_challenges (token, key_authorization, created_at)
           VALUES ($1, $2, $3)
           ON CONFLICT (token) DO UPDATE
           SET key_authorization = EXCLUDED.key_authorization,
               created_at = EXCLUDED.created_at"#,
    )
    .bind(token)
    .bind(key_authorization)
    .bind(Utc::now())
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn find_challenge(pool: &sqlx::PgPool, token: &str) -> Result<Option<String>, AppError> {
    let row: Option<(String,)> = sqlx::query_as(
        r#"SELECT key_authorization FROM acme_challenges
           WHERE token = $1 AND created_at >= $2"#,
    )
    .bind(token)
    .bind(cutoff())
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(key_authorization,)| key_authorization))
}

pub async fn delete_challenge(pool: &sqlx::PgPool, token: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM acme_challenges WHERE token = $1")
        .bind(token)
        .execute(pool)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Apply the shipped schema file, so these tests keep it honest.
    async fn apply_schema(pool: &sqlx::PgPool) {
        sqlx::raw_sql(include_str!("../../deploy/schema.sql"))
            .execute(pool)
            .await
            .expect("apply deploy/schema.sql");
    }

    #[cfg_attr(not(feature = "db-tests"), ignore = "requires database")]
    #[tokio::test]
    async fn challenge_roundtrip() {
        crate::test_db::with_pool(|pool| async move {
            apply_schema(&pool).await;

            put_challenge(&pool, "tok", "tok.thumbprint").await.unwrap();
            assert_eq!(
                find_challenge(&pool, "tok").await.unwrap().as_deref(),
                Some("tok.thumbprint")
            );
            assert_eq!(find_challenge(&pool, "other").await.unwrap(), None);

            // Upsert replaces the key authorization for a re-used token.
            put_challenge(&pool, "tok", "tok.renewed").await.unwrap();
            assert_eq!(
                find_challenge(&pool, "tok").await.unwrap().as_deref(),
                Some("tok.renewed")
            );

            delete_challenge(&pool, "tok").await.unwrap();
            assert_eq!(find_challenge(&pool, "tok").await.unwrap(), None);
        })
        .await;
    }

    #[cfg_attr(not(feature = "db-tests"), ignore = "requires database")]
    #[tokio::test]
    async fn expired_challenges_are_invisible_and_swept() {
        crate::test_db::with_pool(|pool| async move {
            apply_schema(&pool).await;

            sqlx::query(
                "INSERT INTO acme_challenges (token, key_authorization, created_at) VALUES ($1, $2, $3)",
            )
            .bind("stale")
            .bind("stale.thumbprint")
            .bind(Utc::now() - Duration::hours(2))
            .execute(&pool)
            .await
            .unwrap();

            assert_eq!(find_challenge(&pool, "stale").await.unwrap(), None);

            // A write sweeps expired rows.
            put_challenge(&pool, "fresh", "fresh.thumbprint")
                .await
                .unwrap();
            let (count,): (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM acme_challenges WHERE token = 'stale'")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(count, 0);
        })
        .await;
    }
}
