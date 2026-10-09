use axum::extract::Path;
use serde::Deserialize;

use crate::{
    OrNotFound,
    pg::request_extractor,
    structs::list_submitters::{ListSubmitter, ListSubmitterId},
};

#[derive(Deserialize)]
struct SubstituteSubmitterPathParams {
    #[serde(alias = "sub_submitter_id")]
    submitter_id: ListSubmitterId,
}

request_extractor!(ListSubmitter, |store, parts, state| {
    let Path(SubstituteSubmitterPathParams { submitter_id }) =
        Path::<SubstituteSubmitterPathParams>::from_request_parts(parts, state).await?;

    store
        .snapshot()
        .substitute_submitter(submitter_id)
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
        test_utils::{response_body_string, sample_list_submitter},
    };

    #[tokio::test]
    async fn substitute_submitter_extractor_loads_submitter() {
        let substitute_submitter = sample_list_submitter(ListSubmitterId::new());

        let app_state = AppState::new_for_tests().await;
        let store = PgStore::new_for_test();
        substitute_submitter
            .create_substitute(&store)
            .await
            .unwrap();

        let app = Router::new()
            .route(
                "/political-group/substitute-submitters/{sub_submitter_id}",
                get(|substitute_submitter: ListSubmitter| async move {
                    substitute_submitter.name.last_name.to_string()
                }),
            )
            .with_state(app_state);

        let mut request = Request::builder()
            .uri(format!(
                "/political-group/substitute-submitters/{}",
                substitute_submitter.id
            ))
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(store.clone());

        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Bos"));
    }
}
