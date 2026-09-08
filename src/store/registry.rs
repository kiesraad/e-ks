//! Registry for creating and caching stores by (stream_id, election).
//!
//! Ensures each (stream, election) pair has a single shared `Store` instance within
//! the process, and provides an optional initialization hook for first-time loads.

use std::{
    collections::{HashMap, HashSet},
    future::Future,
    sync::Arc,
};

use parking_lot::RwLock;
use serde::{Serialize, de::DeserializeOwned};

use super::{Store, StoreData, StorePersistence, StreamMeta};
use crate::{AppError, ElectionConfig, StreamId, crypto::MasterKey};

type StoreKey = (StreamId, ElectionConfig);
type StoreMap<D> = Arc<RwLock<HashMap<StoreKey, Store<D>>>>;

/// Closure type for the init-less ([`StoreRegistry::get_store`]) path, so the
/// `None` case has a concrete type to infer.
type NoInit<D> = fn(Store<D>) -> std::future::Ready<Result<(), AppError>>;

/// Cache of per-(stream, election) stores backed by a shared persistence backend.
pub struct StoreRegistry<D>
where
    D: StoreData,
    D::Event: Serialize + DeserializeOwned,
{
    persistence: StorePersistence,
    master: MasterKey,
    inner: StoreMap<D>,
}

impl<D> Clone for StoreRegistry<D>
where
    D: StoreData,
    D::Event: Serialize + DeserializeOwned,
{
    fn clone(&self) -> Self {
        Self {
            persistence: self.persistence.clone(),
            master: self.master.clone(),
            inner: self.inner.clone(),
        }
    }
}

impl<D> StoreRegistry<D>
where
    D: StoreData,
    D::Event: Serialize + DeserializeOwned,
{
    /// Create a new registry for stores backed by the given storage URL. Every
    /// stream row it creates is recorded with `scope`.
    pub async fn new(storage_url: String, master: MasterKey) -> Result<Self, AppError> {
        let persistence = StorePersistence::from_storage_url(&storage_url)?;
        persistence.init().await?;

        Ok(Self {
            persistence,
            master,
            inner: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Create a registry that shares an already-initialized persistence backend
    /// (e.g. the same Postgres pool) with another registry, but caches a
    /// different `Store<D>` projection and records its own `scope`. Skips
    /// re-initialization since the backend is assumed to be initialized already.
    pub fn with_persistence(persistence: StorePersistence, master: MasterKey) -> Self {
        Self {
            persistence,
            master,
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Expose the underlying persistence backend (used by the app to share a
    /// single PgPool across stores and sessions).
    pub fn persistence(&self) -> &StorePersistence {
        &self.persistence
    }

    /// Fetch an existing store or create and load it for the given (stream, election).
    pub async fn get_or_create(
        &self,
        stream_id: StreamId,
        election: ElectionConfig,
    ) -> Result<Store<D>, AppError> {
        self.get_or_create_with_init(stream_id, election, |_| async { Ok(()) })
            .await
    }

    /// Fetch an **existing** stream's store, loading from persistence on a
    /// cache miss. Returns [`AppError::NotFound`] if no stream with this
    /// registry's scope was ever persisted for `(stream_id, election)`. Never
    /// creates one.
    pub async fn get_store(
        &self,
        stream_id: StreamId,
        election: ElectionConfig,
    ) -> Result<Store<D>, AppError> {
        self.lookup(stream_id, election, None::<NoInit<D>>).await
    }

    /// Fetch or create a store, then run a one-time async init hook before caching.
    pub async fn get_or_create_with_init<F, Fut>(
        &self,
        stream_id: StreamId,
        election: ElectionConfig,
        init: F,
    ) -> Result<Store<D>, AppError>
    where
        F: FnOnce(Store<D>) -> Fut,
        Fut: Future<Output = Result<(), AppError>>,
    {
        self.lookup(stream_id, election, Some(init)).await
    }

    /// The single fetch path. The in-memory map is only ever an optimization:
    /// a cache miss always consults persistence, so no caller can read absent
    /// state by accident.
    ///
    /// `init` doubles as the create policy. `Some(hook)` creates the stream if
    /// it is missing and runs `hook` on first load; `None` is a read-only
    /// lookup that refuses to materialise a stream that was never persisted.
    async fn lookup<F, Fut>(
        &self,
        stream_id: StreamId,
        election: ElectionConfig,
        init: Option<F>,
    ) -> Result<Store<D>, AppError>
    where
        F: FnOnce(Store<D>) -> Fut,
        Fut: Future<Output = Result<(), AppError>>,
    {
        let key = (stream_id, election);

        if let Some(existing) = self.inner.read().get(&key) {
            return Ok(existing.clone());
        }

        if init.is_none()
            && !self
                .streams_by_scope()
                .await?
                .iter()
                .any(|(id, e)| *id == stream_id && *e == election)
        {
            return Err(AppError::NotFound("Stream not found".to_string()));
        }

        let store = Store::new_for_stream_with_persistence(
            self.persistence.clone(),
            stream_id,
            election,
            &self.master,
        )
        .await?;
        store.load().await?;
        if let Some(init) = init {
            init(store.clone()).await?;
        }

        let mut stores = self.inner.write();
        let entry = stores.entry(key).or_insert(store);

        Ok(entry.clone())
    }

    /// List every `(stream_id, election)` stream matching the [crate::Scope]
    /// of the related data type of the store.
    pub async fn streams_by_scope(&self) -> Result<Vec<(StreamId, ElectionConfig)>, AppError> {
        self.persistence.streams_by_scope(D::scope()).await
    }

    /// List [`StreamMeta`] for every stream matching this registry's scope,
    /// without decrypting or warming any projection.
    pub async fn stream_metadata_by_scope(&self) -> Result<Vec<StreamMeta>, AppError> {
        self.persistence.stream_metadata_by_scope(D::scope()).await
    }

    /// Return the store for `(stream_id, election)` only if it is already warm in
    /// the cache; never consults persistence and never loads.
    pub fn get_cached(&self, stream_id: StreamId, election: ElectionConfig) -> Option<Store<D>> {
        self.inner.read().get(&(stream_id, election)).cloned()
    }

    /// Fetch (or create and load) every store matching the [crate::Scope]
    /// of the related data type of the store.
    pub async fn stores_by_scope(&self) -> Result<Vec<Store<D>>, AppError> {
        let mut stores = Vec::new();
        for (stream_id, election) in self.streams_by_scope().await? {
            stores.push(self.get_or_create(stream_id, election).await?);
        }
        Ok(stores)
    }

    pub async fn stores_for_election(
        &self,
        election: ElectionConfig,
    ) -> Result<Vec<Store<D>>, AppError> {
        let mut stores = Vec::new();
        for (stream_id, e) in self.streams_by_scope().await? {
            if e == election {
                stores.push(self.get_or_create(stream_id, election).await?);
            }
        }
        Ok(stores)
    }

    /// List the elections under the given stream that have persisted events,
    /// consulting the in-memory cache first.
    pub async fn elections_for_stream(
        &self,
        stream_id: StreamId,
    ) -> Result<Vec<ElectionConfig>, AppError> {
        let mut found: HashSet<ElectionConfig> = {
            let cached = self.inner.read();
            cached
                .iter()
                .filter_map(|((id, election), store)| {
                    (*id == stream_id && store.data.read().last_event_id() > 0).then_some(*election)
                })
                .collect()
        };

        let persisted = self.persistence.elections_for_stream(stream_id).await?;
        found.extend(persisted);

        Ok(found.into_iter().collect())
    }
}
