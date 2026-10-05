//! Registry for creating and caching stores by (stream_id, election).
//!
//! Ensures each (stream, election) pair has a single shared `Store` instance within
//! the process, and provides an optional initialization hook for first-time loads.

use std::{
    collections::{HashMap, HashSet},
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::{Mutex, RwLock};
use serde::{Serialize, de::DeserializeOwned};

use super::{Store, StoreData, StorePersistence, StreamMeta};
use crate::{AppError, ElectionConfig, Scope, StreamId, crypto::MasterKey};

type StoreKey = (StreamId, ElectionConfig);
type StoreMap<D> = Arc<RwLock<HashMap<StoreKey, CacheEntry<D>>>>;

/// A cached store plus when it was last handed out, for idle eviction.
struct CacheEntry<D> {
    store: Store<D>,
    last_used: Mutex<Instant>,
}

impl<D> CacheEntry<D> {
    fn new(store: Store<D>) -> Self {
        Self {
            store,
            last_used: Mutex::new(Instant::now()),
        }
    }

    /// Hand out the store, marking the entry as just used.
    fn get(&self) -> Store<D> {
        *self.last_used.lock() = Instant::now();
        self.store.clone()
    }
}

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
    /// Scope the streams are recorded with and listed by; `D::scope()` unless
    /// built with [`Self::with_persistence_in_scope`].
    scope: Scope,
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
            scope: self.scope,
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
    /// stream row it creates is recorded with the projection's scope.
    pub async fn new(storage_url: String, master: MasterKey) -> Result<Self, AppError> {
        let persistence = StorePersistence::from_storage_url(&storage_url)?;
        persistence.init().await?;

        Ok(Self::with_persistence(persistence, master))
    }

    /// Create a registry that shares an already-initialized persistence backend
    /// (e.g. the same Postgres pool) with another registry, but caches a
    /// different `Store<D>` projection under the projection's own scope.
    pub fn with_persistence(persistence: StorePersistence, master: MasterKey) -> Self {
        Self::with_persistence_in_scope(persistence, master, D::scope())
    }

    /// As [`Self::with_persistence`], under `scope` instead of the projection's
    /// own, so two registries over one projection never see each other's streams.
    pub fn with_persistence_in_scope(
        persistence: StorePersistence,
        master: MasterKey,
        scope: Scope,
    ) -> Self {
        Self {
            persistence,
            master,
            scope,
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// The scope this registry's streams are recorded with.
    pub fn scope(&self) -> Scope {
        self.scope
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
            return Ok(existing.get());
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

        let store = Store::new_for_stream_in_scope(
            self.persistence.clone(),
            stream_id,
            election,
            self.scope,
            &self.master,
        )
        .await?;
        store.load().await?;
        if let Some(init) = init {
            init(store.clone()).await?;
        }

        let mut stores = self.inner.write();
        let entry = stores.entry(key).or_insert_with(|| CacheEntry::new(store));

        Ok(entry.get())
    }

    /// List every `(stream_id, election)` stream matching this registry's
    /// [`Scope`].
    pub async fn streams_by_scope(&self) -> Result<Vec<(StreamId, ElectionConfig)>, AppError> {
        self.persistence.streams_by_scope(self.scope).await
    }

    /// List [`StreamMeta`] for every stream matching this registry's scope,
    /// without decrypting or warming any projection.
    pub async fn stream_metadata_by_scope(&self) -> Result<Vec<StreamMeta>, AppError> {
        self.persistence.stream_metadata_by_scope(self.scope).await
    }

    /// Return the store for `(stream_id, election)` only if it is already warm in
    /// the cache; never consults persistence and never loads.
    pub fn get_cached(&self, stream_id: StreamId, election: ElectionConfig) -> Option<Store<D>> {
        self.inner
            .read()
            .get(&(stream_id, election))
            .map(CacheEntry::get)
    }

    /// Evict cached stores that were not handed out for at least `max_idle`,
    /// returning how many were dropped. Only the projection warmth is lost: a
    /// later lookup reloads the stream from persistence. A no-op on the
    /// in-memory backend, where the cached projection is the only copy of the
    /// events.
    pub fn purge_idle(&self, max_idle: Duration) -> usize {
        if matches!(self.persistence, StorePersistence::Memory(_)) {
            return 0;
        }

        let mut stores = self.inner.write();
        let before = stores.len();
        stores.retain(|_, entry| entry.last_used.lock().elapsed() < max_idle);
        before - stores.len()
    }

    /// Fetch (or create and load) every store matching this registry's
    /// [`Scope`].
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
                .filter_map(|((id, election), entry)| {
                    (*id == stream_id && entry.store.data.read().last_event_id() > 0)
                        .then_some(*election)
                })
                .collect()
        };

        let persisted = self.persistence.elections_for_stream(stream_id).await?;
        found.extend(persisted);

        Ok(found.into_iter().collect())
    }
}

/// Periodically evict cached store projections not handed out for `max_idle`.
pub async fn run_store_cache_sweeper<D>(registry: StoreRegistry<D>, max_idle: Duration)
where
    D: StoreData,
    D::Event: Serialize + DeserializeOwned,
{
    const SWEEP_INTERVAL: Duration = Duration::from_secs(5 * 60);

    let mut ticker = tokio::time::interval(SWEEP_INTERVAL);
    loop {
        ticker.tick().await;
        let evicted = registry.purge_idle(max_idle);
        if evicted > 0 {
            tracing::debug!(evicted, "evicted idle store projections");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, CsbAction, CsbStoreData, CsbUser, ElectionConfig, PgEvent, PgStoreData};

    /// Two registries over one projection and backend, under different scopes.
    async fn two_scopes() -> (StoreRegistry<CsbStoreData>, StoreRegistry<CsbStoreData>) {
        let config = Config::new_test();
        let master = MasterKey::new(&config.master_encryption_key);
        let first = StoreRegistry::<CsbStoreData>::new("memory://".to_string(), master.clone())
            .await
            .expect("memory registry");
        let second = StoreRegistry::with_persistence_in_scope(
            first.persistence().clone(),
            master,
            Scope::PreSubmittedToCsb,
        );
        (first, second)
    }

    #[test]
    fn a_registry_defaults_to_the_scope_of_its_projection() {
        let config = Config::new_test();
        let registry = StoreRegistry::<CsbStoreData>::with_persistence(
            StorePersistence::from_storage_url("memory://").expect("memory backend"),
            MasterKey::new(&config.master_encryption_key),
        );

        assert_eq!(registry.scope(), CsbStoreData::scope());
    }

    /// Registry over the filesystem backend in a fresh temp directory. The
    /// local backend only persists political-group streams, hence
    /// [`PgStoreData`].
    async fn local_registry() -> StoreRegistry<PgStoreData> {
        let dir = std::env::temp_dir().join(format!("eks-registry-test-{}", StreamId::new()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let config = Config::new_test();
        StoreRegistry::new(
            format!("local://{}", dir.display()),
            MasterKey::new(&config.master_encryption_key),
        )
        .await
        .expect("local registry")
    }

    #[tokio::test]
    async fn purge_idle_evicts_untouched_stores_but_keeps_persisted_data() -> Result<(), AppError> {
        let registry = local_registry().await;
        let stream_id = StreamId::new();
        registry
            .get_or_create(stream_id, ElectionConfig::EK27)
            .await?
            .update(PgEvent::Login)
            .await?;

        // Recently used, so nothing is idle for an hour yet.
        assert_eq!(registry.purge_idle(Duration::from_secs(3600)), 0);
        assert!(
            registry
                .get_cached(stream_id, ElectionConfig::EK27)
                .is_some()
        );

        assert_eq!(registry.purge_idle(Duration::ZERO), 1);
        assert!(
            registry
                .get_cached(stream_id, ElectionConfig::EK27)
                .is_none()
        );

        // The evicted stream reloads from persistence with its events intact.
        let reloaded = registry.get_store(stream_id, ElectionConfig::EK27).await?;
        assert_eq!(reloaded.current_event_id(), 1);

        Ok(())
    }

    /// The in-memory backend keeps events only in the cached projections, so
    /// purging there would destroy data and must be a no-op.
    #[tokio::test]
    async fn purge_idle_is_a_noop_on_the_memory_backend() -> Result<(), AppError> {
        let (registry, _) = two_scopes().await;
        let stream_id = StreamId::new();
        registry
            .get_or_create(stream_id, ElectionConfig::EK27)
            .await?
            .update(CsbAction::CreateEmpty.by(CsbUser::new_test()))
            .await?;

        assert_eq!(registry.purge_idle(Duration::ZERO), 0);

        let store = registry
            .get_cached(stream_id, ElectionConfig::EK27)
            .expect("still cached");
        assert_eq!(store.current_event_id(), 1);

        Ok(())
    }

    #[tokio::test]
    async fn registries_under_different_scopes_do_not_see_each_others_streams()
    -> Result<(), AppError> {
        let (examination, pre_submission) = two_scopes().await;
        let stream_id = StreamId::new();

        pre_submission
            .get_or_create(stream_id, ElectionConfig::EK27)
            .await?
            .update(CsbAction::CreateEmpty.by(CsbUser::new_test()))
            .await?;

        assert_eq!(
            pre_submission.streams_by_scope().await?,
            vec![(stream_id, ElectionConfig::EK27)]
        );
        assert!(examination.streams_by_scope().await?.is_empty());
        assert!(matches!(
            examination.get_store(stream_id, ElectionConfig::EK27).await,
            Err(AppError::NotFound(_))
        ));

        Ok(())
    }
}
