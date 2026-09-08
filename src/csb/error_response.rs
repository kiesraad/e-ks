//! Error pages for the CSB routes.
//!
//! An [`AppError`](crate::AppError) becomes a response that carries its error
//! page as an [`ErrorPage`] extension (see `view::error_response`); on the
//! app routes `render_error_pages` renders that page in the political-group
//! layout. This is the CSB counterpart: the same page, rendered in the CSB
//! layout with the [`CsbContext`].

use askama::Template;
use axum::{
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{Context, CsbContext, ErrorPage, HtmlTemplate, filters};

#[derive(Template)]
#[template(path = "csb/common/pages/error.html")]
struct CsbErrorTemplate {
    page: ErrorPage,
}

/// Middleware that renders the error page an `AppError` response carries in
/// the CSB layout. It sits inside `csb_store_middleware`, so it covers every
/// error a CSB handler or extractor returns.
pub async fn render_csb_error_pages(context: CsbContext, request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;

    match ErrorPage::take_from(&mut response, context.session.locale) {
        None => response,
        Some(page) => (
            page.status_code,
            HtmlTemplate(CsbErrorTemplate { page }, context),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppError, AppState, CsbUser, ElectionConfig, Locale, Session,
        test_utils::response_body_string,
    };
    use axum::{
        Router,
        body::Body,
        http::StatusCode,
        middleware,
        routing::{MethodRouter, get},
    };
    use tower::ServiceExt;

    /// Drive a request through `render_csb_error_pages` to `handler`, as a
    /// committee member with an English locale.
    async fn respond_through(handler: MethodRouter) -> Response {
        let state = AppState::new_for_tests().await;
        let app = Router::new()
            .route("/", handler)
            .layer(middleware::from_fn_with_state(
                state,
                render_csb_error_pages,
            ));

        let mut request = Request::builder().uri("/").body(Body::empty()).unwrap();
        request.extensions_mut().insert(Session::for_committee(
            CsbUser::new_test(),
            ElectionConfig::EK27,
            Locale::En,
        ));
        app.oneshot(request).await.expect("response")
    }

    #[tokio::test]
    async fn not_found_renders_csb_error_page() {
        let response =
            respond_through(get(|| async { AppError::NotFound("missing".to_string()) })).await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = response_body_string(response).await;
        assert!(body.contains("Error code 404"), "{body}");
        assert!(body.contains("missing"), "{body}");
        // The CSB layout, not the political-group one: home is the CSB index.
        assert!(body.contains("href=\"/csb\""), "{body}");
        assert!(!body.contains("href=\"/\""), "{body}");
    }

    #[tokio::test]
    async fn internal_error_renders_csb_error_page() {
        let response = respond_through(get(|| async { AppError::InternalServerError })).await;

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = response_body_string(response).await;
        assert!(body.contains("Error code 500"), "{body}");
        assert!(
            body.contains("An internal server error occurred."),
            "{body}"
        );
    }

    #[tokio::test]
    async fn successful_responses_pass_through() {
        let response = respond_through(get(|| async { "fine" })).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_body_string(response).await, "fine");
    }
}
