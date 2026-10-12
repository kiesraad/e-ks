use axum::extract::Path;
use serde::Deserialize;

use crate::{
    AppError, Locale,
    pg::request_extractor,
    structs::persons::{Person, PersonId},
    trans,
};

#[derive(Deserialize)]
struct PersonPathParams {
    #[serde(alias = "person_id")]
    person_id: PersonId,
}

request_extractor!(Person, |store, parts, state| {
    let locale = Locale::from_request_parts(parts, state).await?;
    let Path(PersonPathParams { person_id }) =
        Path::<PersonPathParams>::from_request_parts(parts, state).await?;

    store
        .get_person(person_id)
        .map_err(|_| AppError::NotFound(trans!("person.not_found", locale, person_id)))
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
        test_utils::{response_body_string, sample_person},
    };

    #[tokio::test]
    async fn person_extractor_loads_person() {
        let person = sample_person(PersonId::new());

        let app_state = AppState::new_for_tests().await;
        let store = PgStore::new_for_test();
        person.create(&store).await.unwrap();

        let app = Router::new()
            .route(
                "/persons/{person_id}",
                get(|person: Person| async move { person.name.last_name.to_string() }),
            )
            .with_state(app_state);

        let mut request = Request::builder()
            .uri(format!("/persons/{}", person.id))
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(store);

        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Jansen"));
    }

    #[tokio::test]
    async fn person_extractor_returns_not_found() {
        let person_id = PersonId::new();

        let app_state = AppState::new_for_tests().await;
        let store = PgStore::new_for_test();

        let app = Router::new()
            .route(
                "/persons/{person_id}",
                get(|person: Person| async move { person.name.last_name.to_string() }),
            )
            .layer(middleware::from_fn_with_state(
                app_state.clone(),
                render_error_pages,
            ))
            .with_state(app_state);

        let response = app
            .oneshot({
                let mut request = Request::builder()
                    .uri(format!("/persons/{}", person_id))
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

        let expected = trans!("person.not_found", Locale::En, person_id);
        assert!(body.contains(&expected));
    }
}
