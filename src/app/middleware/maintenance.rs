//! Database maintenance gate.
//!
//! When the database is unavailable (see [`crate::DbHealth`]), every
//! DB-dependent route is short-circuited to a static 503 maintenance page
use crate::{AppError, AppState, Context, DbHealth, HtmlTemplate, Locale, LocaleValues, filters};
use askama::Template;
use axum::{
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

#[derive(Template)]
#[template(path = "pg/common/pages/maintenance.html")]
struct MaintenanceTemplate {
    retry_path: String,
}

/// Paths that must keep working while the database is unavailable: the health
/// probe (so orchestration can still read liveness), static assets / live
/// reload (so the maintenance page can load its style sheet), and the
/// always-public files (robots.txt, .well-known).
fn is_exempt(path: &str) -> bool {
    path == "/health"
        || path == "/robots.txt"
        || path.starts_with("/static/")
        || path.starts_with("/.well-known/")
        || path.starts_with("/livereload/")
}

/// Middleware that serves the maintenance page for DB-dependent routes while
/// the database is marked unavailable.
pub async fn db_gate_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if state.db_health.is_healthy() || is_exempt(request.uri().path()) {
        return next.run(request).await;
    }

    maintenance_response(&request)
}

/// Turn a request-path error into a response, tripping the database-health gate
/// when the failure is an infrastructure outage so subsequent requests get the
/// maintenance page too. Non-infrastructure errors fall through to the normal
/// error response.
pub fn handle_db_error(health: &DbHealth, err: AppError, request: &Request) -> Response {
    if err.is_infrastructure_failure() {
        health.mark_unavailable(&err);
        maintenance_response(request)
    } else {
        err.into_response()
    }
}

/// Render the static 503 maintenance page, localized from `Accept-Language`,
/// with a `Retry-After` header and a "try again" link back to the request path.
fn maintenance_response(request: &Request) -> Response {
    let retry_path = request
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());

    let mut response = HtmlTemplate(
        MaintenanceTemplate { retry_path },
        LocaleValues {
            locale: Locale::default(),
        },
    )
    .into_response();

    *response.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
    response
        .headers_mut()
        .insert(header::RETRY_AFTER, header::HeaderValue::from_static("30"));

    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::response_body_string;
    use axum::body::Body;

    fn request_for(uri: &str) -> Request {
        Request::builder().uri(uri).body(Body::empty()).unwrap()
    }

    #[test]
    fn exempts_health_static_and_wellknown() {
        assert!(is_exempt("/health"));
        assert!(is_exempt("/robots.txt"));
        assert!(is_exempt("/static/index.css"));
        assert!(is_exempt("/.well-known/security.txt"));
        assert!(!is_exempt("/"));
        assert!(!is_exempt("/login"));
    }

    #[tokio::test]
    async fn maintenance_response_is_503_with_retry_after() {
        let response = maintenance_response(&request_for("/persons?success=true"));

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers().get(header::RETRY_AFTER).unwrap(), "30");

        let body = response_body_string(response).await;
        // The retry link points back at the originating page.
        assert!(body.contains("href=\"/persons?success=true\""));
    }

    #[tokio::test]
    async fn maintenance_response_localizes_from_accept_language() {
        let request = Request::builder()
            .uri("/")
            .header(header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
            .body(Body::empty())
            .unwrap();
        let response = maintenance_response(&request);
        let body = response_body_string(response).await;
        assert!(body.contains("Tijdelijk niet beschikbaar"));
    }

    #[cfg(feature = "database")]
    #[tokio::test]
    async fn handle_db_error_trips_gate_on_infrastructure_failure() {
        let health = DbHealth::default();
        let err = AppError::DatabaseError(sqlx::Error::PoolTimedOut);

        let response = handle_db_error(&health, err, &request_for("/"));

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(
            !health.is_healthy(),
            "infrastructure failure must trip the gate"
        );
    }

    #[tokio::test]
    async fn handle_db_error_passes_through_logic_errors() {
        let health = DbHealth::default();

        let response = handle_db_error(&health, AppError::GenericNotFound, &request_for("/"));

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(health.is_healthy(), "a logic error must not trip the gate");
    }
}
