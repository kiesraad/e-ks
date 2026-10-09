use axum::extract::Path;
use serde::Deserialize;

use crate::{
    OrNotFound,
    pg::request_extractor,
    structs::name_authorisations::{NameAuthorisation, NameAuthorisationId},
};

#[derive(Deserialize)]
struct NameAuthorisationPathParams {
    authorisation_id: NameAuthorisationId,
}

request_extractor!(NameAuthorisation, |store, parts, state| {
    let Path(NameAuthorisationPathParams { authorisation_id }) =
        Path::<NameAuthorisationPathParams>::from_request_parts(parts, state).await?;

    store
        .snapshot()
        .name_authorisation(authorisation_id)
        .cloned()
        .or_not_found()
});

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
        AppState, PgStore,
        test_utils::{response_body_string, sample_name_authorisation},
    };

    #[tokio::test]
    async fn name_authorisation_extractor_loads_name() {
        let name_auth = sample_name_authorisation(NameAuthorisationId::new());

        let app_state = AppState::new_for_tests().await;
        let store = PgStore::new_for_test();
        name_auth
            .create(&store)
            .await
            .expect("create name authorisation");

        let app =
            Router::new()
                .route(
                    "/political-group/name-authorisation/{authorisation_id}",
                    get(|name_auth: NameAuthorisation| async move {
                        name_auth.name.last_name.to_string()
                    }),
                )
                .with_state(app_state);

        let mut request = Request::builder()
            .uri(format!(
                "/political-group/name-authorisation/{}",
                name_auth.id
            ))
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(store.clone());

        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Jansen"));
    }
}
