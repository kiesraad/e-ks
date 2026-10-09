use axum::extract::Path;

use crate::{
    AppError, OrNotFound, pg::request_extractor, structs::candidate_lists::CandidateList, trans,
};

use super::CandidateListPathParams;

request_extractor!(CandidateList, |store, context, parts, state| {
    let Path(CandidateListPathParams { list_id }) =
        Path::<CandidateListPathParams>::from_request_parts(parts, state).await?;

    store
        .snapshot()
        .candidate_list(list_id)
        .cloned()
        .or_not_found()
        .map_err(|_| {
            AppError::NotFound(trans!(
                "candidate_list.not_found",
                context.session.locale,
                list_id
            ))
        })
});

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        middleware,
        routing::get,
    };
    use tower::ServiceExt;

    use crate::{
        AppState, Locale, PgStore, render_error_pages,
        structs::candidate_lists::CandidateListId,
        test_utils::{response_body_string, sample_candidate_list},
        trans,
    };

    #[tokio::test]
    async fn candidate_list_extractor_loads_list() {
        let list = sample_candidate_list(CandidateListId::new());

        let app_state = AppState::new_for_tests().await;
        let store = PgStore::new_for_test();
        list.create(&store).await.expect("create candidate list");

        let app = Router::new()
            .route(
                "/candidate-lists/{list_id}",
                get(|candidate_list: CandidateList| async move { candidate_list.id.to_string() }),
            )
            .with_state(app_state);

        let mut request = Request::builder()
            .uri(format!("/candidate-lists/{}", list.id))
            .body(Body::empty())
            .unwrap();
        let session = crate::Session::new_test_with_locale(Locale::En);
        request.extensions_mut().insert(session);
        request.extensions_mut().insert(store.clone());

        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains(&list.id.to_string()));
    }

    #[tokio::test]
    async fn candidate_list_extractor_returns_not_found() {
        let state = AppState::new_for_tests().await;
        let list_id = CandidateListId::new();
        let store = PgStore::new_for_test();

        let app = Router::new()
            .route(
                "/candidate-lists/{list_id}",
                get(|candidate_list: CandidateList| async move { candidate_list.id.to_string() }),
            )
            .layer(middleware::from_fn_with_state(
                state.clone(),
                render_error_pages,
            ))
            .with_state(state);

        let response = app
            .oneshot({
                let mut request = Request::builder()
                    .uri(format!("/candidate-lists/{}", list_id))
                    .body(Body::empty())
                    .unwrap();
                let session = crate::Session::new_test_with_locale(Locale::En);
                request.extensions_mut().insert(session);
                request.extensions_mut().insert(store.clone());
                request
            })
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = response_body_string(response).await;
        let expected = trans!("candidate_list.not_found", Locale::En, list_id);
        assert!(body.contains(&expected));
    }
}
