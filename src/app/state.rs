//! Application state container and request extractors.
//! Holds, among others: configuration, store, and CSRF tokens for handlers.

use auth_service::AuthServiceState;
use axum::extract::FromRef;
use secrecy::ExposeSecret;

use crate::{
    AppError, AppRequestState, Config, CsbMainStore, CsbMainStoreData, CsbStoreData, CsbStream,
    DbHealth, ElectionConfig, IdDeriver, PendingRequestStore, PgStoreData, SessionStore, StreamId,
    crypto::MasterKey,
    projection::CSB_MAIN_STREAM_ID,
    store::{Store, StoreRegistry},
    structs::brp::BrpClient,
};

#[cfg(feature = "fixtures")]
use crate::PgStore;

/// Shared application state for request handlers and extractors.
#[derive(FromRef, Clone)]
pub struct AppState {
    pub config: &'static Config,
    pub store_registry: StoreRegistry<PgStoreData>,
    /// Registry for per-import CSB stores (one per imported political group)
    pub csb_store_registry: StoreRegistry<CsbStoreData>,
    /// Registry for the single global CSB main stream shared by all committee members
    pub csb_main_store_registry: StoreRegistry<CsbMainStoreData>,
    /// Active sessions for this application instance (backed by the configured storage).
    pub sessions: SessionStore,
    /// Outstanding SAML AuthnRequest IDs for the `InResponseTo` replay check
    /// (eID §9.7), backed by the configured storage so they survive restarts
    /// and are shared across instances.
    pub pending_requests: PendingRequestStore,
    /// ACME challenge tokens, shared across instances.
    #[cfg(feature = "acme")]
    pub acme_store: crate::AcmeStore,
    pub id_deriver: IdDeriver,
    pub auth_service_state: AuthServiceState,
    pub db_health: DbHealth,
    pub brp_client: BrpClient,
}

impl AppRequestState for AppState {
    fn config(&self) -> &'static Config {
        self.config
    }

    fn sessions(&self) -> &SessionStore {
        &self.sessions
    }

    fn id_deriver(&self) -> &IdDeriver {
        &self.id_deriver
    }

    fn pending_requests(&self) -> &PendingRequestStore {
        &self.pending_requests
    }

    fn csb_store_registry(&self) -> &StoreRegistry<CsbStoreData> {
        &self.csb_store_registry
    }

    fn store_registry(&self) -> &StoreRegistry<PgStoreData> {
        &self.store_registry
    }

    fn csb_main_store_registry(&self) -> &StoreRegistry<CsbMainStoreData> {
        &self.csb_main_store_registry
    }

    fn brp_client(&self) -> &BrpClient {
        &self.brp_client
    }

    async fn csb_store_for_stream(
        &self,
        stream_id: StreamId,
        election: ElectionConfig,
    ) -> Result<CsbStream, AppError> {
        AppState::csb_store_for_stream(self, stream_id, election).await
    }

    async fn csb_main_store(&self, election: ElectionConfig) -> Result<CsbMainStore, AppError> {
        AppState::csb_main_store(self, election).await
    }

    async fn store_for_stream(
        &self,
        stream_id: StreamId,
        election: ElectionConfig,
        load_fixtures: bool,
    ) -> Result<Store<PgStoreData>, AppError> {
        AppState::store_for_stream(self, stream_id, election, load_fixtures).await
    }

    async fn existing_elections_for_stream(
        &self,
        stream_id: StreamId,
    ) -> Result<Vec<ElectionConfig>, AppError> {
        AppState::existing_elections_for_stream(self, stream_id).await
    }
}

impl AppState {
    pub async fn new() -> Result<Self, AppError> {
        let config = Config::from_env()?;

        Self::new_with_config(config).await
    }

    pub async fn new_with_config(config: Config) -> Result<Self, AppError> {
        let master = MasterKey::new(&config.master_encryption_key);
        let store_registry = StoreRegistry::new(
            config.storage_url.expose_secret().to_string(),
            master.clone(),
        )
        .await?;
        // Both CSB registries reuse the PG registry's persistence backend
        let csb_store_registry =
            StoreRegistry::with_persistence(store_registry.persistence().clone(), master.clone());
        let csb_main_store_registry =
            StoreRegistry::with_persistence(store_registry.persistence().clone(), master);
        let sessions = SessionStore::from_storage_url(config.storage_url.expose_secret())?;
        let pending_requests =
            PendingRequestStore::from_storage_url(config.storage_url.expose_secret())?;
        let id_deriver = IdDeriver::new(&config.id_derivation_key);

        let auth_service_state = if config.disable_auth_service {
            AuthServiceState::new_empty()
        } else {
            AuthServiceState::new_from_env().await?
        };

        let brp_client = BrpClient::new(
            &config.brp_client.base_url,
            config.brp_client.api_key.clone(),
            &config.brp_client.persons_endpoint,
            config.brp_client.timeout,
        );

        #[cfg(feature = "acme")]
        let acme_store = crate::AcmeStore::from_storage_url(config.storage_url.expose_secret())?;

        Ok(Self {
            config: Box::leak(Box::new(config)),
            store_registry,
            csb_store_registry,
            csb_main_store_registry,
            sessions,
            pending_requests,
            #[cfg(feature = "acme")]
            acme_store,
            id_deriver,
            auth_service_state,
            db_health: DbHealth::default(),
            brp_client,
        })
    }

    pub(crate) async fn store_for_stream(
        &self,
        stream_id: StreamId,
        election: ElectionConfig,
        load_fixtures: bool,
    ) -> Result<Store<PgStoreData>, AppError> {
        #[cfg(feature = "fixtures")]
        {
            self.store_registry
                .get_or_create_with_init(stream_id, election, |store| async move {
                    if store.data.read().events.is_empty() && load_fixtures {
                        crate::fixtures::load(&PgStore::own(store), None).await?;
                    }
                    Ok(())
                })
                .await
        }
        #[cfg(not(feature = "fixtures"))]
        {
            let _ = load_fixtures; // avoid unused parameter warning

            self.store_registry.get_or_create(stream_id, election).await
        }
    }

    /// Fetch (or create and load) the CSB store for a (stream, election).
    pub(crate) async fn csb_store_for_stream(
        &self,
        stream_id: StreamId,
        election: ElectionConfig,
    ) -> Result<CsbStream, AppError> {
        self.csb_store_registry
            .get_or_create(stream_id, election)
            .await
    }

    /// Fetch (or create) the global CSB main store for the given election.
    /// All committee members share a single stream [`CSB_MAIN_STREAM_ID`].
    pub(crate) async fn csb_main_store(
        &self,
        election: ElectionConfig,
    ) -> Result<CsbMainStore, AppError> {
        self.csb_main_store_registry
            .get_or_create(CSB_MAIN_STREAM_ID, election)
            .await
    }

    /// List which elections already have persisted data under the user's stream.
    pub(crate) async fn existing_elections_for_stream(
        &self,
        stream_id: StreamId,
    ) -> Result<Vec<ElectionConfig>, AppError> {
        self.store_registry.elections_for_stream(stream_id).await
    }

    #[cfg(test)]
    pub async fn new_for_tests() -> Self {
        Self::new_for_tests_with_config(Config::new_test()).await
    }

    /// Test-only constructor that wires up the same dependencies as
    /// [`AppState::new_with_config`] but skips the auth-service env loading
    /// (and IdP metadata fetch) — tests don't run a real IdP.
    #[cfg(test)]
    pub async fn new_for_tests_with_config(config: Config) -> Self {
        let id_deriver = IdDeriver::new(&config.id_derivation_key);
        let master = MasterKey::new(&config.master_encryption_key);
        let sessions = SessionStore::from_storage_url(config.storage_url.expose_secret())
            .expect("test SessionStore must initialize");
        let pending_requests =
            PendingRequestStore::from_storage_url(config.storage_url.expose_secret())
                .expect("test PendingRequestStore must initialize");
        let auth_service_state = AuthServiceState::new_empty();

        let store_registry = StoreRegistry::new(
            config.storage_url.expose_secret().to_string(),
            master.clone(),
        )
        .await
        .expect("test StoreRegistry must initialize");
        let csb_store_registry =
            StoreRegistry::with_persistence(store_registry.persistence().clone(), master.clone());
        let csb_main_store_registry =
            StoreRegistry::with_persistence(store_registry.persistence().clone(), master);

        let brp_client = BrpClient::new(
            &config.brp_client.base_url,
            config.brp_client.api_key.clone(),
            &config.brp_client.persons_endpoint,
            config.brp_client.timeout,
        );

        #[cfg(feature = "acme")]
        let acme_store = crate::AcmeStore::from_storage_url(config.storage_url.expose_secret())
            .expect("test AcmeStore must initialize");

        Self {
            store_registry,
            csb_store_registry,
            csb_main_store_registry,
            config: Box::leak(Box::new(config)),
            sessions,
            pending_requests,
            #[cfg(feature = "acme")]
            acme_store,
            id_deriver,
            auth_service_state,
            db_health: DbHealth::default(),
            brp_client,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn new_for_tests_sets_config_and_tokens() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let config = Config::new_test();

        assert_eq!(
            state.config.storage_url.expose_secret(),
            config.storage_url.expose_secret()
        );

        Ok(())
    }

    #[tokio::test]
    async fn existing_elections_for_stream_is_empty_for_fresh_id() {
        let state = AppState::new_for_tests().await;
        let stream_id = crate::StreamId::new();

        let elections = state
            .existing_elections_for_stream(stream_id)
            .await
            .unwrap();
        assert!(elections.is_empty());
    }
}
