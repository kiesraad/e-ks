//! Contract the request extractors and handlers expect from the router state.

use crate::{
    AppError, Config, ElectionConfig, IdDeriver, PendingRequestStore, SessionStore, StreamId,
    projection::{CsbMainStore, CsbMainStoreData, CsbStoreData, CsbStream, PgStoreData},
    store::{Store, StoreRegistry},
    structs::brp::BrpClient,
};

/// What a request needs from whatever state the router was built with.
///
/// Returns `impl Future` rather than `async fn` so the bounds stay explicit,
/// matching `auth_service::AuthState`.
pub trait AppRequestState: Clone + Send + Sync + 'static {
    fn config(&self) -> &'static Config;

    /// Active sessions for this application instance.
    fn sessions(&self) -> &SessionStore;

    /// Derives the per-user stream id from an authenticated identity.
    fn id_deriver(&self) -> &IdDeriver;

    /// One-shot request ids with a TTL (SAML `InResponseTo`, OAuth `state`).
    fn pending_requests(&self) -> &PendingRequestStore;

    /// Registry for the per-import CSB stores.
    fn csb_store_registry(&self) -> &StoreRegistry<CsbStoreData>;

    /// Registry for the political-group stores.
    fn store_registry(&self) -> &StoreRegistry<PgStoreData>;

    /// Registry for the single global CSB main stream.
    fn csb_main_store_registry(&self) -> &StoreRegistry<CsbMainStoreData>;

    /// Client used to verify candidates against the BRP.
    fn brp_client(&self) -> &BrpClient;

    /// Fetch (or create) the CSB store for a (stream, election).
    fn csb_store_for_stream(
        &self,
        stream_id: StreamId,
        election: ElectionConfig,
    ) -> impl Future<Output = Result<CsbStream, AppError>> + Send;

    /// Fetch (or create) the single global CSB main store for an election.
    fn csb_main_store(
        &self,
        election: ElectionConfig,
    ) -> impl Future<Output = Result<CsbMainStore, AppError>> + Send;

    /// Fetch (or create, optionally loading fixtures) a political-group store.
    fn store_for_stream(
        &self,
        stream_id: StreamId,
        election: ElectionConfig,
        load_fixtures: bool,
    ) -> impl Future<Output = Result<Store<PgStoreData>, AppError>> + Send;

    /// Which elections already have persisted data under this stream.
    fn existing_elections_for_stream(
        &self,
        stream_id: StreamId,
    ) -> impl Future<Output = Result<Vec<ElectionConfig>, AppError>> + Send;
}
