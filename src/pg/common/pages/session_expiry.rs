//! `/session`: the remaining session lifetime for the expiry warning in the
//! browser (`frontend/scripts/generic-ui/session-expiry.ts`).
//!
//! A user may have several tabs open. A tab that is about to warn first asks
//! the server how much time is really left, since another tab may have been
//! active in the meantime; that peek must not count as activity itself, or an
//! idle tab would keep the session alive forever. The session middleware
//! therefore skips the activity refresh for `GET /session` only (see
//! [`SessionExpiryPath::is_activity_free`]). Extending goes through `POST`,
//! which the middleware refreshes and CSRF-checks like any other mutation.

use axum::{
    Json,
    http::{Method, Request},
};
use axum_extra::routing::TypedPath;

use crate::{Session, SessionExpiry, common::SessionExpiryPath};

impl SessionExpiryPath {
    /// Whether `request` is the status peek that must leave `last_activity`
    /// untouched. Only the `GET`: the `POST` on the same path is the extend.
    pub(crate) fn is_activity_free<B>(request: &Request<B>) -> bool {
        request.method() == Method::GET && request.uri().path() == Self::PATH
    }
}

/// `GET /session`: how long the session has left. The middleware did not
/// refresh the session for this request, so the answer reflects the activity
/// of all tabs together.
pub async fn session_expiry(_: SessionExpiryPath, session: Session) -> Json<SessionExpiry> {
    Json(session.expiry())
}

/// `POST /session`: extends the session. The middleware already refreshed
/// `last_activity` (and verified the CSRF token in the header), so this only
/// reports the new remaining lifetime.
pub async fn extend_session(_: SessionExpiryPath, session: Session) -> Json<SessionExpiry> {
    Json(session.expiry())
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode, header},
        middleware,
    };
    use axum_extra::routing::{RouterExt, TypedPath};
    use chrono::{Duration, Utc};
    use tower::ServiceExt;

    use super::*;
    use crate::{
        AppState, SESSION_COOKIE_NAME, auth::csrf_guard::CSRF_HEADER, session_middleware,
        test_utils::response_body_string,
    };

    /// The `/session` routes behind the session middleware, plus a session that
    /// has been idle for ten minutes; returns the app, its cookie and CSRF token.
    async fn setup() -> (AppState, Router, String, String) {
        let state = AppState::new_for_tests().await;
        let app = Router::new()
            .typed_get(session_expiry)
            .typed_post(extend_session)
            .layer(middleware::from_fn_with_state(
                state.clone(),
                session_middleware,
            ))
            .with_state(state.clone());

        let mut session = Session::new_test();
        session.last_activity = Utc::now() - Duration::minutes(10);
        let token = session.token_string();
        let csrf = session.csrf_token().to_string();
        state.sessions.insert(session).await;

        (state, app, format!("{SESSION_COOKIE_NAME}={token}"), csrf)
    }

    async fn stored_expiry(state: &AppState, cookie: &str) -> SessionExpiry {
        let token = cookie.split_once('=').expect("cookie pair").1;
        state
            .sessions
            .get_existing(Some(token))
            .await
            .expect("load session")
            .expect("session present")
            .expiry()
    }

    async fn json_expiry(response: axum::response::Response) -> SessionExpiry {
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
        let body = response_body_string(response).await;
        let value: serde_json::Value = serde_json::from_str(&body).expect("json body");
        SessionExpiry {
            expires_in_secs: value["expires_in_secs"].as_u64().expect("expires_in_secs"),
            warning_lead_secs: value["warning_lead_secs"]
                .as_u64()
                .expect("warning_lead_secs"),
            extendable: value["extendable"].as_bool().expect("extendable"),
        }
    }

    /// Peeking reports the time left and does not count as activity.
    #[tokio::test]
    async fn get_reports_remaining_time_without_extending() {
        let (state, app, cookie, _csrf) = setup().await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri(SessionExpiryPath::PATH)
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");

        let expiry = json_expiry(response).await;
        // ten minutes idle out of fifteen: about five minutes left
        assert!(expiry.expires_in_secs <= 5 * 60, "{expiry:?}");
        assert!(expiry.expires_in_secs > 5 * 60 - 10, "{expiry:?}");
        assert_eq!(expiry.warning_lead_secs, 60);
        assert!(expiry.extendable);
        // the stored session is still ten minutes idle
        assert!(stored_expiry(&state, &cookie).await.expires_in_secs <= 5 * 60);
    }

    /// Extending refreshes the session and reports the full idle timeout.
    #[tokio::test]
    async fn post_extends_the_session() {
        let (state, app, cookie, csrf) = setup().await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(SessionExpiryPath::PATH)
                    .header(header::COOKIE, &cookie)
                    .header(CSRF_HEADER, csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");

        let expiry = json_expiry(response).await;
        assert!(expiry.expires_in_secs > 15 * 60 - 10, "{expiry:?}");
        assert!(stored_expiry(&state, &cookie).await.expires_in_secs > 15 * 60 - 10);
    }

    /// A page cannot be tricked into extending someone's session cross-site:
    /// the extend needs the CSRF token like every other mutation.
    #[tokio::test]
    async fn post_without_csrf_token_is_rejected() {
        let (state, app, cookie, _csrf) = setup().await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(SessionExpiryPath::PATH)
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(stored_expiry(&state, &cookie).await.expires_in_secs <= 5 * 60);
    }

    /// Without a live session the peek gets the login redirect, which the
    /// script reads as "session gone".
    #[tokio::test]
    async fn get_without_session_redirects_to_login() {
        let (_state, app, _cookie, _csrf) = setup().await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri(SessionExpiryPath::PATH)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/login");
    }
}
