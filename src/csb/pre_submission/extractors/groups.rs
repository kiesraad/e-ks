use axum::{extract::FromRequestParts, http::request::Parts};

use crate::{
    AppError, AppRequestState, CsbStore, CsbStream, Session, StreamId,
    csb::{
        examination::structs::BrpCheckState,
        pre_submission::paths::{CsbPreSubmissionBrpCheckPath, CsbPreSubmissionGroupPath},
    },
    projection::WithCorrections,
};

/// A political group imported for the pre-submission check.
pub struct PreSubmissionGroup {
    pub stream_id: StreamId,
    pub appellation: String,
    pub brp: BrpCheckState,
}

impl PreSubmissionGroup {
    pub fn from_store(store: &CsbStream) -> Self {
        Self {
            stream_id: store.stream_id,
            appellation: store.get_appellation(WithCorrections::All),
            brp: BrpCheckState::for_political_group(store),
        }
    }

    pub fn path(&self) -> CsbPreSubmissionGroupPath {
        CsbPreSubmissionGroupPath {
            stream_id: self.stream_id,
        }
    }

    pub fn brp_check_path(&self) -> CsbPreSubmissionBrpCheckPath {
        CsbPreSubmissionBrpCheckPath {
            stream_id: self.stream_id,
        }
    }
}

/// The pre-submission stream named by the `stream_id` path parameter, bound to
/// the acting committee member. Streams of the examination are not found here.
pub struct PreSubmissionStore(pub CsbStore);

impl std::ops::Deref for PreSubmissionStore {
    type Target = CsbStore;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S: AppRequestState> FromRequestParts<S> for PreSubmissionStore {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        CsbStore::from_registry(parts, state, state.pre_submission_store_registry())
            .await
            .map(Self)
    }
}

/// The pre-submission imports of the election the session works on.
pub struct PreSubmissionGroups(pub Vec<PreSubmissionGroup>);

impl<S: AppRequestState> FromRequestParts<S> for PreSubmissionGroups {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let session = Session::from_request_parts(parts, state).await?;
        let election = session.require_current_election()?;

        let mut groups = Vec::new();
        for store in state
            .pre_submission_store_registry()
            .stores_for_election(election)
            .await?
        {
            if !store.is_deleted() {
                groups.push(PreSubmissionGroup::from_store(&store));
            }
        }

        Ok(PreSubmissionGroups(groups))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        routing::get,
    };
    use tower::ServiceExt;

    use crate::{
        AppState, CsbAction, CsbUser, ElectionConfig, Locale, PgStoreData, Province,
        store::StoreRegistry, test_utils::sample_political_group,
    };

    /// Seed a stream in `registry` with an import of the sample group.
    async fn seed(
        registry: &StoreRegistry<crate::CsbStoreData>,
        election: ElectionConfig,
    ) -> StreamId {
        let stream_id = StreamId::new();
        let store = registry.get_or_create(stream_id, election).await.unwrap();
        store
            .update(
                CsbAction::Import {
                    hash: [0u8; 32],
                    source_stream_id: StreamId::new(),
                    snapshot: Box::new(PgStoreData {
                        political_group: sample_political_group(),
                        ..PgStoreData::default()
                    }),
                }
                .by(CsbUser::new_test()),
            )
            .await
            .unwrap();
        stream_id
    }

    fn committee_parts(election: ElectionConfig) -> Parts {
        let mut parts = Request::builder()
            .uri("/csb/pre-submission")
            .body(Body::empty())
            .unwrap()
            .into_parts()
            .0;
        parts.extensions.insert(Session::for_committee(
            CsbUser::new_test(),
            election,
            Locale::default(),
        ));
        parts
    }

    #[tokio::test]
    async fn lists_the_pre_submission_imports_and_not_the_examination_ones() {
        let state = AppState::new_for_tests().await;
        let pre_submitted = seed(state.pre_submission_store_registry(), ElectionConfig::EK27).await;
        seed(state.csb_store_registry(), ElectionConfig::EK27).await;

        let mut parts = committee_parts(ElectionConfig::EK27);
        let PreSubmissionGroups(groups) =
            PreSubmissionGroups::from_request_parts(&mut parts, &state)
                .await
                .unwrap();

        let stream_ids: Vec<_> = groups.iter().map(|g| g.stream_id).collect();
        assert_eq!(stream_ids, vec![pre_submitted]);
        assert_eq!(groups[0].appellation, "Kiesraad Demo");
        assert_eq!(groups[0].brp, BrpCheckState::Correct);
    }

    #[tokio::test]
    async fn skips_imports_of_another_election() {
        let state = AppState::new_for_tests().await;
        let own = seed(state.pre_submission_store_registry(), ElectionConfig::EK27).await;
        seed(
            state.pre_submission_store_registry(),
            ElectionConfig::PS27(Province::Groningen),
        )
        .await;

        let mut parts = committee_parts(ElectionConfig::EK27);
        let PreSubmissionGroups(groups) =
            PreSubmissionGroups::from_request_parts(&mut parts, &state)
                .await
                .unwrap();

        let stream_ids: Vec<_> = groups.iter().map(|g| g.stream_id).collect();
        assert_eq!(stream_ids, vec![own]);
    }

    async fn request_store(state: AppState, stream_id: StreamId) -> axum::response::Response {
        let app = Router::new()
            .route(
                "/csb/pre-submission/{stream_id}",
                get(|store: PreSubmissionStore| async move { store.stream_id.to_string() }),
            )
            .with_state(state);

        let mut request = Request::builder()
            .uri(format!("/csb/pre-submission/{stream_id}"))
            .body(Body::empty())
            .unwrap();
        let mut session = Session::new_test_committee();
        session.set_test_election(ElectionConfig::EK27);
        request.extensions_mut().insert(session);

        app.oneshot(request).await.unwrap()
    }

    #[tokio::test]
    async fn store_extractor_finds_pre_submission_streams_only() {
        let state = AppState::new_for_tests().await;
        let pre_submitted = seed(state.pre_submission_store_registry(), ElectionConfig::EK27).await;
        let examined = seed(state.csb_store_registry(), ElectionConfig::EK27).await;

        let response = request_store(state.clone(), pre_submitted).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            crate::test_utils::response_body_string(response).await,
            pre_submitted.to_string()
        );

        // An examination import is not reachable under the pre-submission routes.
        let response = request_store(state, examined).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
