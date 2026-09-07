//! Error pages for the app (political group) routes.
//!
//! An [`AppError`](crate::AppError) becomes a response that carries its error
//! page as an [`ErrorPage`] extension (see `view::error_response`);
//! [`render_error_pages`] renders that page in the political-group layout.
//! The CSRF rejection page is rendered here directly, as the rejection
//! short-circuits above that layer.

use askama::Template;
use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use tracing::warn;

use crate::{
    Context, ErrorPage, HtmlTemplate, Locale, LocaleValues, auth::csrf_guard::CsrfRejection,
    filters, trans,
};

#[derive(Template)]
#[template(path = "pg/common/pages/error.html")]
struct ErrorTemplate {
    page: ErrorPage,
}

#[derive(Template)]
#[template(path = "pg/common/pages/request_error.html")]
struct RequestErrorTemplate {
    title: String,
    message: String,
}

/// Styled CSRF rejection page, rendered directly with the session locale
/// (the rejection short-circuits above `render_error_pages`).
pub(crate) fn csrf_rejection_response(rejection: CsrfRejection, locale: Locale) -> Response {
    let (status_code, title, message) = match rejection {
        CsrfRejection::InvalidToken => (
            StatusCode::BAD_REQUEST,
            trans!("common.request_error.csrf_title", locale),
            trans!("common.request_error.csrf_message", locale),
        ),
        CsrfRejection::BodyTooLarge => (
            StatusCode::PAYLOAD_TOO_LARGE,
            trans!("common.request_error.too_large_title", locale),
            trans!("common.request_error.too_large_message", locale),
        ),
        CsrfRejection::UnreadableBody => (
            StatusCode::BAD_REQUEST,
            trans!("common.request_error.unreadable_title", locale),
            trans!("common.request_error.unreadable_message", locale),
        ),
    };
    warn!(?rejection, "mutating request rejected by CSRF enforcement");

    let mut response = HtmlTemplate(
        RequestErrorTemplate { title, message },
        LocaleValues { locale },
    )
    .into_response();
    *response.status_mut() = status_code;
    response
}

/// Middleware that renders the error page an `AppError` response carries in
/// the political-group layout.
pub async fn render_error_pages(context: Context, request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;

    match ErrorPage::take_from(&mut response, context.session.locale) {
        None => response,
        Some(page) => (
            page.status_code,
            HtmlTemplate(ErrorTemplate { page }, context),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppError, AppState, Locale, test_utils};
    use axum::{Router, body::Body, extract::Request, http::StatusCode, middleware, routing::get};
    use tower::ServiceExt;

    #[tokio::test]
    async fn not_found_renders_template_with_message() {
        let state = AppState::new_for_tests().await;
        let store = crate::PgStore::new_for_test();
        let app = Router::new()
            .route(
                "/",
                get(|| async { AppError::NotFound("missing".to_string()) }),
            )
            .layer(middleware::from_fn_with_state(state, render_error_pages));

        let mut request = Request::builder().uri("/").body(Body::empty()).unwrap();
        let session = crate::Session::new_test_with_locale(Locale::En);
        request.extensions_mut().insert(session);
        request.extensions_mut().insert(store);
        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = test_utils::response_body_string(response).await;
        assert!(body.contains("Error code 404"));
        assert!(body.contains("missing"));
    }

    /// The rate-limit page is translated with the session locale.
    #[tokio::test]
    async fn rate_limit_page_is_rendered_in_the_session_locale() {
        let state = AppState::new_for_tests().await;
        let store = crate::PgStore::new_for_test();
        let app = Router::new()
            .route(
                "/",
                get(|| async { AppError::EventLimitReached { max: 20_000 } }),
            )
            .layer(middleware::from_fn_with_state(state, render_error_pages));

        let mut request = Request::builder().uri("/").body(Body::empty()).unwrap();
        request
            .extensions_mut()
            .insert(crate::Session::new_test_with_locale(Locale::Nl));
        request.extensions_mut().insert(store);
        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = test_utils::response_body_string(response).await;
        assert!(body.contains("Foutcode 429"), "{body}");
        assert!(body.contains("Te veel verzoeken"), "{body}");
    }

    #[cfg(feature = "database")]
    #[tokio::test]
    async fn database_error_maps_to_internal_server_error() {
        let state = AppState::new_for_tests().await;
        let store = crate::PgStore::new_for_test();
        let app = Router::new()
            .route(
                "/",
                get(|| async { AppError::DatabaseError(sqlx::Error::RowNotFound) }),
            )
            .layer(middleware::from_fn_with_state(state, render_error_pages));
        let mut request = Request::builder().uri("/").body(Body::empty()).unwrap();
        let session = crate::Session::new_test_with_locale(Locale::En);
        request.extensions_mut().insert(session);
        request.extensions_mut().insert(store);
        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[cfg(feature = "acme")]
    #[tokio::test]
    async fn acme_error_maps_to_internal_server_error() {
        let state = AppState::new_for_tests().await;
        let store = crate::PgStore::new_for_test();
        let app = Router::new()
            .route(
                "/",
                get(|| async { AppError::AcmeError(instant_acme::Error::Str("boom")) }),
            )
            .layer(middleware::from_fn_with_state(state, render_error_pages));
        let mut request = Request::builder().uri("/").body(Body::empty()).unwrap();
        let session = crate::Session::new_test_with_locale(Locale::En);

        request.extensions_mut().insert(session);
        request.extensions_mut().insert(store);
        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
