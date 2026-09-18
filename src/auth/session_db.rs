//! Postgres-backed session persistence helpers.
//!
//! Keeps all `sqlx` / `serde_json` usage in one file so the `session_store`
//! module itself depends only on the generic session types.

#![cfg(feature = "database")]
use std::str::FromStr;

use chrono::{DateTime, Utc};
use tracing::warn;

use crate::{
    AppError, Locale, Session, SessionUser, TokenValue,
    auth::session::{session_absolute_timeout, session_idle_timeout},
};

/// A `sessions` row, mapped by column name. `token` holds the token hash;
/// `identity` holds the serialized [`SessionUser`].
struct SessionRow {
    token: String,
    identity: serde_json::Value,
    locale: String,
    last_activity: DateTime<Utc>,
    created_at: DateTime<Utc>,
    user_agent_hash: Option<String>,
    csrf_token: String,
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for SessionRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;

        Ok(Self {
            token: row.try_get("token")?,
            identity: row.try_get("identity")?,
            locale: row.try_get("locale")?,
            last_activity: row.try_get("last_activity")?,
            created_at: row.try_get("created_at")?,
            user_agent_hash: row.try_get("user_agent_hash")?,
            csrf_token: row.try_get("csrf_token")?,
        })
    }
}

/// Insert or update a session row (`token` column holds the hash). `created_at`
/// and `user_agent_hash` are omitted from `ON CONFLICT` so a touch can't reset
/// them.
pub async fn upsert(pool: &sqlx::PgPool, session: &Session) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO sessions
            (token, identity, locale, last_activity, created_at, user_agent_hash, csrf_token)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (token) DO UPDATE SET
            identity = EXCLUDED.identity,
            locale = EXCLUDED.locale,
            last_activity = EXCLUDED.last_activity,
            csrf_token = EXCLUDED.csrf_token
        "#,
    )
    .bind(session.token_hash())
    .bind(serde_json::to_value(&session.user)?)
    .bind(session.locale.as_str())
    .bind(session.last_activity)
    .bind(session.created_at)
    .bind(&session.user_agent_hash)
    .bind(&session.csrf_token().0)
    .execute(pool)
    .await?;

    Ok(())
}

/// Fetch a single session by its token hash.
///
/// Fails closed: a row whose identity does not parse is deleted and reported
/// as no session (forcing a re-login), never silently mapped to a default
/// identity. Not surfaced as an error, so one corrupt row cannot trip the
/// maintenance gate.
pub async fn load(pool: &sqlx::PgPool, token_hash: &str) -> Result<Option<Session>, AppError> {
    let row: Option<SessionRow> = sqlx::query_as(
        r#"SELECT token, identity, locale, last_activity, created_at, user_agent_hash, csrf_token
           FROM sessions WHERE token = $1"#,
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };
    match session_from_row(row) {
        Some(session) => Ok(Some(session)),
        None => {
            delete(pool, token_hash).await?;
            Ok(None)
        }
    }
}

/// Maps a row to a `Session`; `None` when the identity does not parse.
fn session_from_row(row: SessionRow) -> Option<Session> {
    let user = match serde_json::from_value::<SessionUser>(row.identity) {
        Ok(user) => user,
        Err(err) => {
            warn!("dropping session with unreadable identity: {err}");
            return None;
        }
    };

    Some(Session {
        token_hash: row.token,
        raw_token: None, // never carried by a reloaded session
        csrf_token: TokenValue(row.csrf_token),
        created_at: row.created_at,
        last_activity: row.last_activity,
        user_agent_hash: row.user_agent_hash,
        user,
        locale: Locale::from_str(&row.locale).unwrap_or_default(),
    })
}

/// Write the mutable fields of an existing session row. Like [`upsert`] but a
/// plain UPDATE, so it never re-creates a row a concurrent logout deleted.
/// `created_at` and `user_agent_hash` are fixed at login, and the identity can
/// only change within its role (`identity ? $5` matches the serde external
/// tag): role changes go through session establishment, never through
/// mutation.
pub async fn update(pool: &sqlx::PgPool, session: &Session) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE sessions SET
            identity = $2,
            locale = $3,
            last_activity = $4,
            csrf_token = $6
        WHERE token = $1 AND identity ? $5
        "#,
    )
    .bind(session.token_hash())
    .bind(serde_json::to_value(&session.user)?)
    .bind(session.locale.as_str())
    .bind(session.last_activity)
    .bind(session.user.tag())
    .bind(&session.csrf_token().0)
    .execute(pool)
    .await?;

    Ok(())
}

/// Refresh `last_activity` of an existing session row. A conditional UPDATE
/// (not an upsert), so it never re-creates a row a concurrent logout deleted.
pub async fn touch(
    pool: &sqlx::PgPool,
    token_hash: &str,
    last_activity: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query("UPDATE sessions SET last_activity = $2 WHERE token = $1")
        .bind(token_hash)
        .bind(last_activity)
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete a single session by its token hash.
pub async fn delete(pool: &sqlx::PgPool, token_hash: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM sessions WHERE token = $1")
        .bind(token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete all sessions that have passed either the idle timeout or the absolute
/// lifetime cap (mirrors [`Session::is_expired`]).
pub async fn cleanup_expired(pool: &sqlx::PgPool) -> Result<(), AppError> {
    let now = Utc::now();
    let idle_cutoff = now - session_idle_timeout();
    let absolute_cutoff = now - session_absolute_timeout();
    sqlx::query("DELETE FROM sessions WHERE last_activity < $1 OR created_at < $2")
        .bind(idle_cutoff)
        .bind(absolute_cutoff)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CsbUser, ElectionConfig, StreamId};

    fn sample_row(identity: serde_json::Value) -> SessionRow {
        SessionRow {
            token: "token-hash-abc".to_string(),
            identity,
            locale: Locale::default().as_str().to_string(),
            last_activity: Utc::now(),
            created_at: Utc::now(),
            user_agent_hash: Some("ua-hash".to_string()),
            csrf_token: "csrf-token-abc".to_string(),
        }
    }

    /// Every identity shape survives the row mapping, so CSB events keep
    /// referencing the right user and SP-initiated logout (eID §7.7.1) keeps
    /// its NameID for database-backed sessions.
    #[test]
    fn session_from_row_roundtrips_every_identity() {
        let identities = [
            SessionUser::PoliticalGroup {
                stream_id: StreamId::new(),
                saml_name_id: "name-id-xyz".to_string(),
                election: None,
            },
            SessionUser::PoliticalGroup {
                stream_id: StreamId::new(),
                saml_name_id: String::new(),
                election: Some(ElectionConfig::EK27),
            },
            SessionUser::CentralElectoralCommittee {
                user: CsbUser::new_test(),
                election: ElectionConfig::EK27,
                paper_correction_stream_id: None,
            },
            SessionUser::CentralElectoralCommittee {
                user: CsbUser::Github {
                    user_id: "583231".parse().expect("valid id"),
                },
                election: ElectionConfig::EK27,
                paper_correction_stream_id: Some(StreamId::new()),
            },
        ];

        for user in identities {
            let row = sample_row(serde_json::to_value(&user).expect("serialize"));
            let session = session_from_row(row).expect("maps row");
            assert_eq!(session.user, user);
        }
    }

    /// A row whose identity does not parse maps to no session (fail closed),
    /// never to a default identity.
    #[test]
    fn session_from_row_rejects_unreadable_identity() {
        for identity in [
            serde_json::json!(null),
            serde_json::json!({"Unknown": {}}),
            serde_json::json!({"PoliticalGroup": {"missing": "fields"}}),
        ] {
            assert!(session_from_row(sample_row(identity)).is_none());
        }
    }
}
