//! Database-backed persistence for the event store.

use super::{EncryptedEvent, EventHash};
use chrono::{DateTime, Utc};
use serde::{Serialize, de::DeserializeOwned};

use super::{
    Replay, Store, StoreData, StoreEvent, StreamMeta, chain_hash, event_aad, persistence::NewStream,
};
use crate::{
    AppError, ElectionConfig, Scope, StreamId,
    crypto::{EventCipher, WrappedKey},
};

#[cfg(feature = "database")]
impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for StoreEvent<Vec<u8>> {
    /// Map a database row into a store event whose `payload` is the encrypted blob.
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;

        let event_id: i64 = row.try_get("event_id")?;
        let payload: Vec<u8> = row.try_get("payload")?;
        let created_at: DateTime<Utc> = row.try_get("created_at")?;
        let hash_bytes: Vec<u8> = row.try_get("hash")?;
        let hash: EventHash = hash_bytes.as_slice().try_into().map_err(|_| {
            sqlx::Error::Decode(
                format!(
                    "event {event_id} hash is {} bytes, expected 32",
                    hash_bytes.len()
                )
                .into(),
            )
        })?;

        Ok(Self {
            event_id: event_id as usize,
            payload,
            created_at,
            hash,
        })
    }
}

/// Initialize the database schema for event and session persistence.
#[cfg(feature = "migrations")]
pub async fn migrate(pool: &sqlx::PgPool) -> Result<(), AppError> {
    let mut conn = pool.acquire().await?;

    if let Err(error) = async {
        create_streams_table(&mut conn).await?;
        create_events_table(&mut conn).await?;
        create_sessions_table(&mut conn).await?;
        create_pending_requests_table(&mut conn).await?;
        Ok::<(), AppError>(())
    }
    .await
    {
        tracing::warn!("Database migration failed, there might me a concurrent migration: {error}");
    }

    Ok(())
}

#[cfg(feature = "migrations")]
async fn create_streams_table(conn: &mut sqlx::PgConnection) -> Result<(), AppError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS streams (
          stream_id UUID NOT NULL,
          election TEXT NOT NULL,
          last_event_id BIGINT NOT NULL,
          scope TEXT NOT NULL DEFAULT 'political_group',
          encrypted_key BYTEA,
          PRIMARY KEY (stream_id, election)
        )
        "#,
    )
    .execute(&mut *conn)
    .await?;

    // Upgrade path for databases created before per-stream keys existed.
    sqlx::query("ALTER TABLE streams ADD COLUMN IF NOT EXISTS encrypted_key BYTEA")
        .execute(&mut *conn)
        .await?;

    Ok(())
}

#[cfg(feature = "migrations")]
async fn create_events_table(conn: &mut sqlx::PgConnection) -> Result<(), AppError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS events (
          stream_id UUID NOT NULL,
          election TEXT NOT NULL,
          event_id BIGINT NOT NULL,
          created_at timestamp with time zone NOT NULL,
          hash bytea NOT NULL,
          payload bytea NOT NULL,
          PRIMARY KEY (stream_id, election, event_id)
        )
        "#,
    )
    .execute(&mut *conn)
    .await?;

    // Supports looking up an event (and its stream) by its chain hash.
    sqlx::query(r#"CREATE INDEX IF NOT EXISTS events_hash_idx ON events(hash)"#)
        .execute(&mut *conn)
        .await?;

    Ok(())
}

#[cfg(feature = "migrations")]
async fn create_sessions_table(conn: &mut sqlx::PgConnection) -> Result<(), AppError> {
    // `token` holds the token's SHA-256 hash, not the token itself; `identity`
    // holds the serialized `SessionUser`.
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS sessions (
          token TEXT PRIMARY KEY,
          identity JSONB NOT NULL,
          locale TEXT NOT NULL,
          last_activity TIMESTAMPTZ NOT NULL,
          created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
          user_agent_hash TEXT,
          csrf_token TEXT NOT NULL
        )
        "#,
    )
    .execute(&mut *conn)
    .await?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS sessions_last_activity_idx
           ON sessions(last_activity)"#,
    )
    .execute(&mut *conn)
    .await?;

    Ok(())
}

#[cfg(feature = "migrations")]
async fn create_pending_requests_table(conn: &mut sqlx::PgConnection) -> Result<(), AppError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS pending_requests (
          id TEXT PRIMARY KEY,
          created_at TIMESTAMPTZ NOT NULL
        )
        "#,
    )
    .execute(&mut *conn)
    .await?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS pending_requests_created_at_idx
           ON pending_requests(created_at)"#,
    )
    .execute(&mut *conn)
    .await?;

    Ok(())
}

/// Probe every table the application depends on, so a missing or broken schema
/// (for example a dropped `sessions` table) surfaces as an error rather than a
/// confusing per-request failure later
pub async fn verify_schema(pool: &sqlx::PgPool) -> Result<(), AppError> {
    // `LIMIT 0` checks each table exists and is readable without scanning rows.
    const TABLE_PROBES: [&str; 4] = [
        "SELECT 1 FROM streams LIMIT 0",
        "SELECT 1 FROM events LIMIT 0",
        "SELECT 1 FROM sessions LIMIT 0",
        "SELECT 1 FROM pending_requests LIMIT 0",
    ];

    for probe in TABLE_PROBES {
        sqlx::query(probe).execute(pool).await?;
    }
    Ok(())
}

/// Ensure a stream row exists, recording `new`'s scope and wrapped key on
/// first creation (later calls leave both untouched), and return the stored
/// wrapped key. The `COALESCE` backfills keys onto pre-upgrade rows; their
/// old events fail on decrypt.
pub async fn ensure_stream(pool: &sqlx::PgPool, new: &NewStream) -> Result<WrappedKey, AppError> {
    let wrapped: Vec<u8> = sqlx::query_scalar(
        r#"INSERT INTO streams (stream_id, election, last_event_id, scope, encrypted_key)
        VALUES ($1, $2, 0, $3, $4)
        ON CONFLICT (stream_id, election) DO UPDATE
          SET encrypted_key = COALESCE(streams.encrypted_key, EXCLUDED.encrypted_key)
        RETURNING encrypted_key"#,
    )
    .bind(new.stream_id.uuid())
    .bind(new.election.stable_id())
    .bind(new.scope.as_str())
    .bind(new.encrypted_key.as_bytes())
    .fetch_one(pool)
    .await?;

    Ok(WrappedKey::from(wrapped))
}

/// List every `(stream_id, election)` stream with the given scope that has
/// persisted data.
///
/// A stream is identified by its `(stream_id, election)` pair, so a single
/// `stream_id` can appear multiple times, once per election. Empty placeholder
/// rows (`last_event_id = 0`) are excluded, mirroring [`elections_for_stream`].
pub async fn streams_by_scope(
    pool: &sqlx::PgPool,
    scope: Scope,
) -> Result<Vec<(StreamId, ElectionConfig)>, AppError> {
    let rows: Vec<(uuid::Uuid, String)> = sqlx::query_as(
        r#"SELECT stream_id, election
           FROM streams
           WHERE scope = $1 AND last_event_id > 0"#,
    )
    .bind(scope.as_str())
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|(id, code)| {
            ElectionConfig::from_stable_id(&code).map(|election| (StreamId(id), election))
        })
        .collect())
}

/// List [`StreamMeta`] for every stream with the given scope in one aggregate
/// query. `event_count` is `streams.last_event_id`; the timestamps are
/// `MIN`/`MAX` over the events' plain `created_at`. Empty placeholders
/// (`last_event_id = 0`) are excluded, mirroring [`streams_by_scope`].
pub async fn stream_metadata_by_scope(
    pool: &sqlx::PgPool,
    scope: Scope,
) -> Result<Vec<StreamMeta>, AppError> {
    /// `(stream_id, election, last_event_id, first created_at, last created_at)`.
    type MetaRow = (
        uuid::Uuid,
        String,
        i64,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
    );

    let rows: Vec<MetaRow> = sqlx::query_as(
        r#"SELECT s.stream_id, s.election, s.last_event_id,
                      MIN(e.created_at) AS created_at, MAX(e.created_at) AS last_event_at
               FROM streams s
               LEFT JOIN events e
                 ON e.stream_id = s.stream_id AND e.election = s.election
               WHERE s.scope = $1 AND s.last_event_id > 0
               GROUP BY s.stream_id, s.election, s.last_event_id"#,
    )
    .bind(scope.as_str())
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|(id, code, last_id, created_at, last_event_at)| {
            ElectionConfig::from_stable_id(&code).map(|election| StreamMeta {
                stream_id: StreamId(id),
                election,
                event_count: last_id as usize,
                created_at,
                last_event_at,
            })
        })
        .collect())
}

/// List the elections that have persisted events under the given stream.
pub async fn elections_for_stream(
    pool: &sqlx::PgPool,
    stream_id: StreamId,
) -> Result<Vec<ElectionConfig>, AppError> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT election FROM streams
         WHERE stream_id = $1 AND last_event_id > 0",
    )
    .bind(stream_id.uuid())
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|(code,)| ElectionConfig::from_stable_id(&code))
        .collect())
}

/// Locate the political-group event whose chain hash begins with `hash_prefix`.
///
/// Returns the `(stream_id, election, event_id)` of the single matching event,
/// or `None` if nothing matches. The lookup is restricted to
/// [`Scope::PoliticalGroup`] streams so a prefix can only ever resolve to an
/// app-store event (never a CSB event). The prefix match uses a `substring`
/// comparison, which cannot use the `events_hash_idx` btree, so it scans the
/// `events` table; acceptable at current volumes, but revisit with a
/// left-anchored range predicate if it grows. An ambiguous prefix matching
/// more than one event is reported as [`AppError::AmbiguousHash`].
pub async fn find_event_by_hash_prefix(
    pool: &sqlx::PgPool,
    hash_prefix: &[u8],
) -> Result<Option<(StreamId, ElectionConfig, usize)>, AppError> {
    let rows: Vec<(uuid::Uuid, String, i64)> = sqlx::query_as(
        r#"
        SELECT e.stream_id, e.election, e.event_id
        FROM events e
        JOIN streams s ON s.stream_id = e.stream_id AND s.election = e.election
        WHERE s.scope = $1
          AND substring(e.hash from 1 for octet_length($2)) = $2
        LIMIT 2
        "#,
    )
    .bind(Scope::PoliticalGroup.as_str())
    .bind(hash_prefix)
    .fetch_all(pool)
    .await?;

    if rows.len() > 1 {
        return Err(AppError::AmbiguousHash);
    }

    Ok(rows
        .into_iter()
        .next()
        .and_then(|(stream_id, code, event_id)| {
            ElectionConfig::from_stable_id(&code)
                .map(|election| (StreamId(stream_id), election, event_id as usize))
        }))
}

/// Load and replay missing events from the database into the store.
pub async fn load_from_database<D>(
    store: &Store<D>,
    pool: &sqlx::PgPool,
    cipher: &EventCipher,
) -> Result<(), AppError>
where
    D: StoreData,
    D::Event: DeserializeOwned,
{
    let mut tx = pool.begin().await?;

    if let Err(err) = catch_up(store, &mut tx, cipher).await {
        tx.rollback().await?;
        return Err(err);
    }

    Ok(())
}

/// Append the event to the database and apply it to the store.
pub async fn update_in_database<D>(
    store: &Store<D>,
    pool: &sqlx::PgPool,
    cipher: &EventCipher,
    event: D::Event,
) -> Result<(), AppError>
where
    D: StoreData,
    D::Event: Serialize + DeserializeOwned,
{
    let mut tx = pool.begin().await?;

    let (last_id, replay) = match catch_up(store, &mut tx, cipher).await {
        Ok(caught_up) => caught_up,
        Err(err) => {
            tx.rollback().await?;
            return Err(err);
        }
    };

    if let Err(err) = replay.reject_append(store.stream_id) {
        tx.rollback().await?;
        return Err(err);
    }

    let next_id = last_id + 1;
    let created_at = Utc::now();
    let prev_hash = replay.chain_tip;

    let hash = match insert_event(
        store, cipher, next_id, created_at, &event, &prev_hash, &mut tx,
    )
    .await
    {
        Ok(hash) => hash,
        Err(err) => {
            tx.rollback().await?;
            return Err(err);
        }
    };

    tx.commit().await?;

    store.apply_persisted_event(next_id, event, created_at, hash);

    Ok(())
}

/// Bring the in-memory store up to date by replaying missing events.
///
/// Returns the stream's stored last event id (ahead of the projection when the
/// replay was truncated; see [`Replay`]) alongside the replay outcome.
async fn catch_up<D>(
    store: &Store<D>,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    cipher: &EventCipher,
) -> Result<(usize, Replay), AppError>
where
    D: StoreData,
    D::Event: DeserializeOwned,
{
    let last_id: usize = store.data.read().last_event_id();
    let election_id = store.election.stable_id();

    let stream_last_id: i64 = sqlx::query_scalar(
        r#"SELECT last_event_id
        FROM streams
        WHERE stream_id = $1 AND election = $2
        FOR UPDATE"#,
    )
    .bind(store.stream_id.uuid())
    .bind(&election_id)
    .fetch_one(&mut **tx)
    .await?;

    let missing: Vec<StoreEvent<Vec<u8>>> = sqlx::query_as::<_, StoreEvent<Vec<u8>>>(
        r#"
        SELECT event_id, payload, created_at, hash
        FROM events
        WHERE stream_id = $1 AND election = $2 AND event_id > $3
        ORDER BY event_id ASC
        "#,
    )
    .bind(store.stream_id.uuid())
    .bind(&election_id)
    .bind(last_id as i64)
    .fetch_all(&mut **tx)
    .await?;

    let mut data = store.data.write();
    let replay = super::apply_encrypted_events(
        &mut *data,
        cipher,
        missing.into_iter().map(|e| EncryptedEvent {
            event_id: e.event_id,
            created_at: e.created_at,
            hash: e.hash,
            payload: e.payload,
        }),
    )?;

    if let Some(event_id) = replay.truncated_at {
        tracing::error!(
            stream_id = %store.stream_id,
            election = %election_id,
            event_id,
            "stream is only partially readable by this build; \
             serving the projection as of the preceding event"
        );
    }

    Ok((stream_last_id as usize, replay))
}

/// Encrypt `payload`, insert the event row within an open transaction, bump
/// `streams.last_event_id`, and return the event's chain hash (computed over the
/// encrypted blob).
async fn insert_event<D, E: Serialize>(
    store: &Store<D>,
    cipher: &EventCipher,
    event_id: usize,
    created_at: DateTime<Utc>,
    payload: &E,
    prev_hash: &EventHash,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<[u8; 32], AppError>
where
    D: StoreData,
{
    let aad = event_aad(event_id, created_at, prev_hash);
    let encrypted_payload = cipher.encrypt(payload, &aad)?;
    let hash = chain_hash(prev_hash, event_id, created_at, &encrypted_payload);
    let election_id = store.election.stable_id();

    sqlx::query(
        r#"INSERT INTO events (stream_id, election, event_id, created_at, hash, payload)
        VALUES ($1, $2, $3, $4, $5, $6)"#,
    )
    .bind(store.stream_id.uuid())
    .bind(&election_id)
    .bind(event_id as i64)
    .bind(created_at)
    .bind(hash.as_slice())
    .bind(encrypted_payload)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        r#"UPDATE streams SET last_event_id = $3
           WHERE stream_id = $1 AND election = $2"#,
    )
    .bind(store.stream_id.uuid())
    .bind(&election_id)
    .bind(event_id as i64)
    .execute(&mut **tx)
    .await?;

    Ok(hash)
}
