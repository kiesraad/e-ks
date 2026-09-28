use axum::{
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::{TypedHeader, headers};

use crate::{
    PgEvent, PgStore,
    auth::csrf_guard::is_header_token_request,
    common::{HideDownloadWarningPath, PgIndexPath},
    redirect_to_referer,
};

pub async fn hide_download_warning(
    _: HideDownloadWarningPath,
    referer: Option<TypedHeader<headers::Referer>>,
    headers: HeaderMap,
    store: PgStore,
) -> Result<Response, crate::AppError> {
    store.update(PgEvent::HideDownloadWarning).await?;

    // A script dismisses the banner in place and stays on the page, so it gets
    // no redirect: reloading would throw away unsaved form input.
    if is_header_token_request(&headers) {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // Back to the page the banner was dismissed on, never off-site (see
    // [`redirect_to_referer`]).
    Ok(match referer {
        Some(TypedHeader(referer)) => redirect_to_referer(&referer, PgIndexPath).into_response(),
        None => Redirect::to(&PgIndexPath.to_string()).into_response(),
    })
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::Body,
        http::{Request, header},
        middleware,
    };
    use axum_extra::routing::RouterExt;
    use tower::ServiceExt;

    use crate::{
        AppState, ElectionConfig, auth::csrf_guard::CSRF_HEADER, session_middleware,
        store_middleware,
    };

    use super::*;

    /// A router holding only this route, with a session and a store that has
    /// already recorded a download, so the warning shows.
    async fn setup() -> (Router, PgStore, String, String) {
        let state = AppState::new_for_tests().await;
        let app = Router::new()
            .typed_post(hide_download_warning)
            .layer(middleware::from_fn_with_state(
                state.clone(),
                store_middleware,
            ))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                session_middleware,
            ))
            .with_state(state.clone());

        let stream_id = crate::StreamId::new();
        let store = crate::PgStore::own(
            state
                .store_for_stream(stream_id, ElectionConfig::EK27, false)
                .await
                .expect("store"),
        );

        let mut session = crate::Session::new_test_for_stream(stream_id);
        session.set_test_election(ElectionConfig::EK27);
        let token = session.token_string();
        let csrf = session.csrf_token().to_string();
        state.sessions.insert(session).await;

        // after download, the warning should show
        store
            .update(PgEvent::DownloadFile {
                file_name: "documents.zip".to_string(),
                download_path: "/download".to_string(),
            })
            .await
            .unwrap();
        assert!(store.should_show_download_warning());

        (app, store, token, csrf)
    }

    fn post(token: &str) -> axum::http::request::Builder {
        Request::builder()
            .method("POST")
            .uri("/hide-download-warning")
            .header(
                header::COOKIE,
                format!("{}={}", crate::SESSION_COOKIE_NAME, token),
            )
    }

    #[tokio::test]
    async fn hide_download_warning_records_event_and_redirects() {
        let (app, store, token, csrf) = setup().await;

        let request = post(&token)
            .header(header::REFERER, "https://example.com/candidate-lists")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(format!("csrf_token={csrf}")))
            .unwrap();

        let response = app.oneshot(request).await.expect("response");

        // we should be redirected and the warning should no longer show. Only
        // the referrer's path survives, so the redirect stays on this origin.
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response.headers().get(header::LOCATION).unwrap(),
            "/candidate-lists",
        );
        assert!(!store.should_show_download_warning());
    }

    /// The script sends its token in the header and expects to stay put.
    #[tokio::test]
    async fn hide_download_warning_answers_no_content_to_a_header_token() {
        let (app, store, token, csrf) = setup().await;

        let request = post(&token)
            .header(header::REFERER, "https://example.com/candidate-lists")
            .header(CSRF_HEADER, csrf)
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(response.headers().get(header::LOCATION).is_none());
        assert!(!store.should_show_download_warning());
    }

    /// Without a referrer the event is still recorded, rather than the request
    /// being rejected outright.
    #[tokio::test]
    async fn hide_download_warning_without_referer_falls_back_to_the_index() {
        let (app, store, token, csrf) = setup().await;

        let request = post(&token)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(format!("csrf_token={csrf}")))
            .unwrap();

        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/");
        assert!(!store.should_show_download_warning());
    }
}
