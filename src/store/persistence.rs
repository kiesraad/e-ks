//! Persistence backends for the event store.

use chrono::Utc;
use serde::{Serialize, de::DeserializeOwned};
use std::{path::PathBuf, sync::Arc};

use crate::{
    AppError, ElectionConfig, Scope, StreamId,
    crypto::{MasterKey, StreamKey, WrappedKey},
    utils::StorageScheme,
};

use super::{
    Store, StoreData, StoreEvent, StreamMeta, chain_hash,
    filesystem::{self, replay_from_file},
    memory::{self, MemoryStore},
    store_handle::StoreBackend,
};

#[cfg(feature = "database")]
use super::database::{self, load_from_database, update_in_database};

/// Everything a backend records when a stream is first created. Backends
/// store `encrypted_key` as-is; wrapping and unwrapping stay in this module.
pub(crate) struct NewStream {
    pub stream_id: StreamId,
    pub election: ElectionConfig,
    pub scope: Scope,
    /// Fresh stream key, wrapped by the master key.
    pub encrypted_key: WrappedKey,
}

impl NewStream {
    /// Generate and wrap a fresh stream key for a stream to be created.
    fn generate(
        stream_id: StreamId,
        election: ElectionConfig,
        scope: Scope,
        master: &MasterKey,
    ) -> Result<Self, AppError> {
        let encrypted_key = master.wrap_key(&StreamKey::generate(), stream_id, election)?;
        Ok(Self {
            stream_id,
            election,
            scope,
            encrypted_key,
        })
    }
}

/// Persistence backend selection for a store.
#[derive(Clone, Debug)]
pub enum StorePersistence {
    /// PostgreSQL-backed persistence using a shared connection pool.
    #[cfg(feature = "database")]
    Database(sqlx::PgPool),
    /// Local filesystem persistence under the provided directory.
    Local(PathBuf),
    /// In-memory only (no durable persistence). Carries a shared index of stream
    /// scopes and event hashes so lookups resolve like the other backends.
    Memory(MemoryStore),
}

impl StorePersistence {
    /// Build a persistence backend from a storage URL.
    pub fn from_storage_url(storage_url: &str) -> Result<Self, AppError> {
        match StorageScheme::parse(storage_url)? {
            StorageScheme::Memory => Ok(StorePersistence::Memory(MemoryStore::default())),
            StorageScheme::Local => {
                let path_string = storage_url.strip_prefix("local://").unwrap_or("");
                let path = PathBuf::from(path_string);

                if !path.exists() || !path.is_dir() {
                    return Err(AppError::ConfigLoadError(format!(
                        "Local storage requires a directory path, got: {path_string}"
                    )));
                }

                Ok(StorePersistence::Local(path))
            }
            StorageScheme::Postgres => {
                #[cfg(feature = "database")]
                {
                    let pool = sqlx::PgPool::connect_lazy(storage_url)?;
                    Ok(StorePersistence::Database(pool))
                }
                #[cfg(not(feature = "database"))]
                {
                    Err(crate::utils::database_disabled_error())
                }
            }
        }
    }

    /// Initialize the selected persistence backend (migrations, etc).
    pub async fn init(&self) -> Result<(), AppError> {
        match self {
            #[cfg(feature = "database")]
            StorePersistence::Database(pool) => {
                #[cfg(feature = "migrations")]
                if let Err(err) = database::migrate(pool).await {
                    tracing::warn!(
                        "initial database migration failed; \
                         starting anyway and retrying in the background: {err}"
                    );
                }
                let _ = pool;
            }
            StorePersistence::Local(dir) => {
                filesystem::init_local(dir).await?;
            }
            StorePersistence::Memory(_) => {}
        }

        Ok(())
    }

    /// Ensure the stream exists (recording `scope` on first creation) and
    /// resolve this persistence target into a [`StoreBackend`]. Persisting
    /// backends get the stream's cipher, unwrapped with `master`; the
    /// in-memory backend has none.
    pub(crate) async fn into_backend_for_stream(
        self,
        stream_id: StreamId,
        election: ElectionConfig,
        scope: Scope,
        master: &MasterKey,
    ) -> Result<StoreBackend, AppError> {
        // The stored key may differ from the generated one: an existing
        // stream keeps the key it was created with.
        let stored_cipher = |wrapped: WrappedKey| {
            Ok::<_, AppError>(Box::new(
                master.unwrap_key(&wrapped, stream_id, election)?.cipher(),
            ))
        };

        match self {
            #[cfg(feature = "database")]
            StorePersistence::Database(pool) => {
                let new = NewStream::generate(stream_id, election, scope, master)?;
                let wrapped = database::ensure_stream(&pool, &new).await?;
                Ok(StoreBackend::Database {
                    pool,
                    cipher: stored_cipher(wrapped)?,
                })
            }
            StorePersistence::Local(dir) => {
                let new = NewStream::generate(stream_id, election, scope, master)?;
                let wrapped = filesystem::ensure_stream_file(&dir, &new).await?;
                Ok(StoreBackend::Local {
                    dir,
                    cipher: stored_cipher(wrapped)?,
                })
            }
            StorePersistence::Memory(store) => {
                memory::ensure_stream(&store, stream_id, election, scope);
                Ok(StoreBackend::Memory { store })
            }
        }
    }

    /// List every `(stream_id, election)` stream with the given scope.
    ///
    /// The database backend reads each stream's recorded scope. Local file
    /// storage only ever holds political-group streams, so it lists every
    /// non-empty stream for [`Scope::PoliticalGroup`] and nothing for any other
    /// scope. The in-memory backend tracks each stream's scope in its shared
    /// index and lists the non-empty streams matching `scope`.
    pub async fn streams_by_scope(
        &self,
        scope: Scope,
    ) -> Result<Vec<(StreamId, ElectionConfig)>, AppError> {
        match self {
            #[cfg(feature = "database")]
            StorePersistence::Database(pool) => {
                super::database::streams_by_scope(pool, scope).await
            }
            StorePersistence::Local(dir) => Ok(filesystem::streams_by_scope(dir, scope).await),
            StorePersistence::Memory(store) => Ok(memory::streams_by_scope(store, scope)),
        }
    }

    /// List [`StreamMeta`] for every stream with the given scope, reading only
    /// each backend's index. The in-memory backend keeps no timestamps.
    pub async fn stream_metadata_by_scope(
        &self,
        scope: Scope,
    ) -> Result<Vec<StreamMeta>, AppError> {
        match self {
            #[cfg(feature = "database")]
            StorePersistence::Database(pool) => {
                super::database::stream_metadata_by_scope(pool, scope).await
            }
            StorePersistence::Local(dir) => filesystem::stream_metadata_by_scope(dir, scope).await,
            StorePersistence::Memory(store) => Ok(memory::stream_metadata_by_scope(store, scope)),
        }
    }

    /// List the elections under the given stream that have persisted events.
    pub async fn elections_for_stream(
        &self,
        stream_id: StreamId,
    ) -> Result<Vec<ElectionConfig>, AppError> {
        match self {
            #[cfg(feature = "database")]
            StorePersistence::Database(pool) => {
                super::database::elections_for_stream(pool, stream_id).await
            }
            StorePersistence::Local(dir) => {
                Ok(filesystem::elections_for_stream(dir, stream_id).await)
            }
            StorePersistence::Memory(store) => Ok(memory::elections_for_stream(store, stream_id)),
        }
    }

    /// Locate the political-group event whose chain hash begins with
    /// `hash_prefix`, returning its `(stream_id, election, event_id)`.
    ///
    /// The database backend indexes events by hash and restricts the lookup to
    /// political-group streams; local file storage only holds political-group
    /// streams, so it scans them directly. The in-memory backend scans its
    /// shared index, likewise restricted to political-group streams.
    pub async fn find_event_by_hash_prefix(
        &self,
        hash_prefix: &[u8],
    ) -> Result<Option<(StreamId, ElectionConfig, usize)>, AppError> {
        match self {
            #[cfg(feature = "database")]
            StorePersistence::Database(pool) => {
                database::find_event_by_hash_prefix(pool, hash_prefix).await
            }
            StorePersistence::Local(dir) => {
                filesystem::find_event_by_hash_prefix(dir, hash_prefix).await
            }
            StorePersistence::Memory(store) => {
                memory::find_event_by_hash_prefix(store, hash_prefix)
            }
        }
    }

    /// Verify the persistence backend is reachable. For the database backend
    /// this acquires a connection and runs `SELECT 1`; for the local backend
    /// it checks the storage directory still exists; the in-memory backend is
    /// always reachable.
    pub async fn health_check(&self) -> Result<(), AppError> {
        match self {
            #[cfg(feature = "database")]
            StorePersistence::Database(pool) => {
                sqlx::query("SELECT 1").execute(pool).await?;
                Ok(())
            }
            StorePersistence::Local(dir) => {
                if dir.is_dir() {
                    Ok(())
                } else {
                    Err(AppError::ConfigLoadError(format!(
                        "Local storage directory missing: {}",
                        dir.display()
                    )))
                }
            }
            StorePersistence::Memory(_) => Ok(()),
        }
    }

    pub async fn migrate(&self) -> Result<(), AppError> {
        match self {
            #[cfg(feature = "database")]
            StorePersistence::Database(_pool) => {
                #[cfg(feature = "migrations")]
                database::migrate(_pool).await?;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Verify the persistence backend is ready to serve: reachable with its
    /// schema present.
    pub async fn verify_ready(&self) -> Result<(), AppError> {
        match self {
            #[cfg(feature = "database")]
            StorePersistence::Database(pool) => database::verify_schema(pool).await,
            other => other.health_check().await,
        }
    }
}

impl<D> Store<D>
where
    D: StoreData,
{
    /// Create a new store scoped to a specific (stream_id, election) pair.
    pub async fn new_for_stream(
        storage_url: &str,
        stream_id: StreamId,
        election: ElectionConfig,
        master: &MasterKey,
    ) -> Result<Self, AppError> {
        let persistence = StorePersistence::from_storage_url(storage_url)?;
        persistence.init().await?;
        Self::new_for_stream_with_persistence(persistence, stream_id, election, master).await
    }

    /// Create a new store for a stream using an already-initialized persistence
    /// backend. Scope and wrapped key are recorded on first creation.
    pub async fn new_for_stream_with_persistence(
        persistence: StorePersistence,
        stream_id: StreamId,
        election: ElectionConfig,
        master: &MasterKey,
    ) -> Result<Self, AppError> {
        let backend = persistence
            .into_backend_for_stream(stream_id, election, D::scope(), master)
            .await?;

        Ok(Store {
            stream_id,
            election,
            backend,
            data: Arc::new(parking_lot::RwLock::new(D::default())),
        })
    }

    #[cfg(feature = "database")]
    /// Create a new store backed by the provided database pool for a (stream, election).
    pub async fn new_with_pool_for_stream(
        pool: sqlx::PgPool,
        stream_id: StreamId,
        election: ElectionConfig,
        master: &MasterKey,
    ) -> Result<Self, AppError> {
        let persistence = StorePersistence::Database(pool);
        persistence.init().await?;
        Self::new_for_stream_with_persistence(persistence, stream_id, election, master).await
    }
}

impl<D> Store<D>
where
    D: StoreData,
    D::Event: Serialize + DeserializeOwned,
{
    /// Load and replay persisted events into the in-memory store.
    pub async fn load(&self) -> Result<(), AppError> {
        match &self.backend {
            #[cfg(feature = "database")]
            StoreBackend::Database { pool, cipher } => {
                load_from_database(self, pool, cipher).await?;
            }
            StoreBackend::Local { dir, cipher } => {
                replay_from_file(self, dir, cipher).await?;
            }
            // The in-memory projection is the only copy of the events, so there
            // is nothing to replay back into it.
            StoreBackend::Memory { .. } => {}
        }

        Ok(())
    }

    /// Persist an event and apply it to the in-memory store.
    pub async fn update(&self, event: D::Event) -> Result<(), AppError> {
        match &self.backend {
            #[cfg(feature = "database")]
            StoreBackend::Database { pool, cipher } => {
                update_in_database(self, pool, cipher, event).await
            }
            StoreBackend::Local { dir, cipher } => {
                let (last_id, replay) = replay_from_file(self, dir, cipher).await?;
                replay.reject_append(self.stream_id)?;
                let next_id = last_id + 1;
                let created_at = Utc::now();
                let prev_hash = replay.chain_tip;

                let path = filesystem::stream_path(dir, self.stream_id, self.election);
                let hash = filesystem::append_event(
                    &path, cipher, next_id, created_at, &event, &prev_hash,
                )
                .await?;

                self.apply_persisted_event(next_id, event, created_at, hash);

                Ok(())
            }
            StoreBackend::Memory { store } => {
                let mut data = self.data.write();
                let event_id = data.last_event_id() + 1;
                let created_at = Utc::now();
                let prev_hash = data.last_event_hash();
                // Nothing is persisted, so the chain hash is over the plain encoding.
                let body = postcard::to_allocvec(&event).map_err(|e| {
                    AppError::ServerError(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
                })?;
                let hash = chain_hash(&prev_hash, event_id, created_at, &body);
                // Record the hash in the shared index so cross-stream lookups
                // (by scope, by hash prefix) resolve like the other backends.
                memory::record_event(store, self.stream_id, self.election, event_id, hash);
                data.apply(StoreEvent {
                    event_id,
                    payload: event,
                    created_at,
                    hash,
                });

                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Scope;
    use secrecy::SecretString;
    use std::{fs, path::PathBuf};
    use uuid::Uuid;

    const TEST_ELECTION: ElectionConfig = ElectionConfig::EK27;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("store-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn test_master() -> MasterKey {
        MasterKey::new(&SecretString::from("test-secret"))
    }

    #[test]
    fn from_storage_url_accepts_memory() {
        let persistence = StorePersistence::from_storage_url("memory://").unwrap();

        assert!(matches!(persistence, StorePersistence::Memory(_)));
    }

    #[test]
    fn from_storage_url_accepts_local_directory() {
        let dir = temp_dir();
        let url = format!("local://{}", dir.display());

        let persistence = StorePersistence::from_storage_url(&url).unwrap();

        match persistence {
            StorePersistence::Local(path) => assert_eq!(path, dir),
            _ => panic!("expected local persistence"),
        }
    }

    #[test]
    fn from_storage_url_rejects_missing_local_directory() {
        let dir = std::env::temp_dir().join(format!("missing-{}", Uuid::new_v4()));
        let url = format!("local://{}", dir.display());

        let err = StorePersistence::from_storage_url(&url).unwrap_err();

        match err {
            AppError::ConfigLoadError(_) => {}
            _ => panic!("expected config load error"),
        }
    }

    #[test]
    fn from_storage_url_rejects_invalid_url() {
        let err = StorePersistence::from_storage_url("not a url").unwrap_err();
        match err {
            AppError::ConfigLoadError(_) => {}
            _ => panic!("expected config load error"),
        }
    }

    #[test]
    fn from_storage_url_rejects_unsupported_scheme() {
        let err = StorePersistence::from_storage_url("s3://bucket/key").unwrap_err();
        match err {
            AppError::ConfigLoadError(_) => {}
            _ => panic!("expected config load error"),
        }
    }

    #[derive(Default)]
    struct TestData {
        events: Vec<StoreEvent<usize>>,
        applied: Vec<usize>,
    }

    impl StoreData for TestData {
        type Event = usize;

        fn apply(&mut self, event: StoreEvent<Self::Event>) {
            self.applied.push(event.payload);
            self.events.push(event);
        }

        fn events(&self) -> &[StoreEvent<Self::Event>] {
            &self.events
        }

        fn scope() -> Scope {
            Scope::PoliticalGroup
        }
    }

    fn test_store() -> Store<TestData> {
        Store {
            stream_id: StreamId::new(),
            election: TEST_ELECTION,
            backend: StoreBackend::Memory {
                store: MemoryStore::default(),
            },
            data: std::sync::Arc::new(parking_lot::RwLock::new(TestData::default())),
        }
    }

    #[tokio::test]
    async fn update_in_memory_increments_event_id() -> Result<(), AppError> {
        let store = test_store();

        store.update(10).await?;
        store.update(11).await?;

        let data = store.data.read();
        assert_eq!(data.last_event_id(), 2);
        assert_eq!(data.applied, vec![10, 11]);

        Ok(())
    }

    #[tokio::test]
    async fn correct_key_loads_persisted_events() -> Result<(), AppError> {
        let dir = temp_dir();
        let master = test_master();
        let stream_id = StreamId::new();
        let persistence = StorePersistence::Local(dir.clone());

        let store = Store::<TestData>::new_for_stream_with_persistence(
            persistence.clone(),
            stream_id,
            TEST_ELECTION,
            &master,
        )
        .await?;

        store.update(10).await?;
        store.update(20).await?;

        let fresh = Store::<TestData>::new_for_stream_with_persistence(
            persistence,
            stream_id,
            TEST_ELECTION,
            &master,
        )
        .await?;
        fresh.load().await?;

        let data = fresh.data.read();
        assert_eq!(data.last_event_id(), 2);
        assert_eq!(data.applied, vec![10, 20]);

        Ok(())
    }

    #[tokio::test]
    async fn stream_metadata_by_scope_reports_counts_and_timestamps() -> Result<(), AppError> {
        let dir = temp_dir();
        let master = test_master();
        let persistence = StorePersistence::Local(dir.clone());
        let stream_id = StreamId::new();

        let store = Store::<TestData>::new_for_stream_with_persistence(
            persistence.clone(),
            stream_id,
            TEST_ELECTION,
            &master,
        )
        .await?;
        store.update(10).await?;
        store.update(20).await?;

        let meta = persistence
            .stream_metadata_by_scope(Scope::PoliticalGroup)
            .await?;
        assert_eq!(meta.len(), 1);
        let entry = &meta[0];
        assert_eq!(entry.stream_id, stream_id);
        assert_eq!(entry.event_count, 2);
        let created = entry.created_at.expect("created timestamp");
        let last = entry.last_event_at.expect("last timestamp");
        assert!(created <= last);

        assert!(
            persistence
                .stream_metadata_by_scope(Scope::CentralElectoralCommittee)
                .await?
                .is_empty()
        );

        Ok(())
    }

    #[tokio::test]
    async fn wrong_secret_cannot_unwrap_stream_key() -> Result<(), AppError> {
        let dir = temp_dir();
        let master = test_master();
        let stream_id = StreamId::new();
        let persistence = StorePersistence::Local(dir.clone());

        let store = Store::<TestData>::new_for_stream_with_persistence(
            persistence.clone(),
            stream_id,
            TEST_ELECTION,
            &master,
        )
        .await?;

        store.update(10).await?;
        store.update(20).await?;

        // wrong master secret: the stored key cannot be unwrapped
        let wrong_master = MasterKey::new(&SecretString::from("wrong-secret"));
        let Err(err) = Store::<TestData>::new_for_stream_with_persistence(
            persistence,
            stream_id,
            TEST_ELECTION,
            &wrong_master,
        )
        .await
        else {
            panic!("store construction must fail with the wrong secret");
        };
        assert!(matches!(err, AppError::EventDecodeError(_)));

        Ok(())
    }

    #[tokio::test]
    async fn wrong_stream_cannot_load_events_from_other_stream() -> Result<(), AppError> {
        let dir = temp_dir();
        let master = test_master();
        let stream_a = StreamId::new();
        let stream_b = StreamId::new();
        let persistence = StorePersistence::Local(dir.clone());

        let store_a = Store::<TestData>::new_for_stream_with_persistence(
            persistence.clone(),
            stream_a,
            TEST_ELECTION,
            &master,
        )
        .await?;

        store_a.update(42).await?;

        // Copy stream_a's file to stream_b's path, so stream_b (with its own
        // fresh key) tries to read data encrypted with stream_a's key.
        let src = dir.join(format!("{stream_a}_{}.bin", TEST_ELECTION.stable_id()));
        let dst = dir.join(format!("{stream_b}_{}.bin", TEST_ELECTION.stable_id()));
        std::fs::copy(&src, &dst).expect("copy stream file");

        let store_b = Store::<TestData>::new_for_stream_with_persistence(
            persistence.clone(),
            stream_b,
            TEST_ELECTION,
            &master,
        )
        .await?;

        let err = store_b
            .load()
            .await
            .expect_err("load must fail for events encrypted with another stream's key");
        assert!(matches!(err, AppError::EventDecodeError(_)));
        assert!(store_b.data.read().applied.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn transplanted_key_file_cannot_be_unwrapped() -> Result<(), AppError> {
        let dir = temp_dir();
        let master = test_master();
        let stream_a = StreamId::new();
        let stream_b = StreamId::new();
        let persistence = StorePersistence::Local(dir.clone());

        Store::<TestData>::new_for_stream_with_persistence(
            persistence.clone(),
            stream_a,
            TEST_ELECTION,
            &master,
        )
        .await?;

        // the wrap AAD binds the key to stream_a, so it must not unwrap for stream_b
        let src = dir.join(format!("{stream_a}_{}.key", TEST_ELECTION.stable_id()));
        let dst = dir.join(format!("{stream_b}_{}.key", TEST_ELECTION.stable_id()));
        std::fs::copy(&src, &dst).expect("copy key file");

        let Err(err) = Store::<TestData>::new_for_stream_with_persistence(
            persistence,
            stream_b,
            TEST_ELECTION,
            &master,
        )
        .await
        else {
            panic!("a transplanted key file must not unwrap");
        };
        assert!(matches!(err, AppError::EventDecodeError(_)));

        Ok(())
    }
}
