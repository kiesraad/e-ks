//! Session and store-loading middleware.

use axum::{
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::{
    extract::{CookieJar, cookie::Cookie},
    routing::TypedPath,
};
use chrono::Utc;
use serde::{Serialize, de::DeserializeOwned};

use super::maintenance::handle_db_error;
use crate::{
    AppError, AppState, CsbStore, PgStore, SESSION_COOKIE_NAME, Session, SessionUser,
    auth::{csrf_guard::enforce_csrf, session_extractor::user_agent_hash},
    common::{LoginStartPath, PgIndexPath, SelectElectionPath},
    csb::index::CsbIndexPath,
    csrf_rejection_response,
    finalise::FinalisePath,
    store::{Store, StoreData},
};

/// Middleware that loads or creates a session and stores it in request extensions.
pub async fn session_middleware(
    State(state): State<AppState>,
    jar: CookieJar,
    request: Request,
    next: Next,
) -> Response {
    #[cfg(feature = "dev-features")]
    if request.uri().path() == super::dev_login::DEV_LOGIN_PATH {
        return next.run(request).await;
    }

    let token = jar.get(SESSION_COOKIE_NAME).map(Cookie::value);

    let mut session = match state.sessions.get_existing(token).await {
        Ok(Some(session)) => session,
        // Send unauthenticated users to the login start page (DigiD button +
        // explanation), not straight into the SAML flow at `/login`.
        Ok(None) => return Redirect::to(&LoginStartPath.to_string()).into_response(),
        // A database error here must not masquerade as "logged out": trip the
        // maintenance gate instead of redirecting to login.
        Err(err) => return handle_db_error(&state.db_health, err, &request),
    };

    if let Some(rejection) =
        reject_on_user_agent_mismatch(&state, &session, request.headers(), token).await
    {
        return rejection;
    }

    let mut request = match enforce_csrf(request, &session).await {
        Ok(request) => request,
        Err(rejection) => return csrf_rejection_response(rejection, session.locale),
    };

    session.last_activity = Utc::now();
    state.sessions.touch(&session).await;

    super::csb_access::report_alert_hours_activity(&state, &session, &request);
    request.extensions_mut().insert(session);

    next.run(request).await
}

/// User-agent pinning: reject (and drop) sessions replayed from a different client.
///
/// Returns the login redirect to short-circuit the middleware, or `None` when the UA
/// matches (or when the session did not record a UA).
async fn reject_on_user_agent_mismatch(
    state: &AppState,
    session: &Session,
    headers: &HeaderMap,
    token: Option<&str>,
) -> Option<Response> {
    let expected = session.user_agent_hash.as_deref()?;
    if user_agent_hash(headers) == expected {
        return None;
    }

    tracing::warn!("session user-agent mismatch; dropping session");
    if let Some(token) = token {
        state.sessions.remove(token).await;
    }
    Some(Redirect::to(&LoginStartPath.to_string()).into_response())
}

/// Middleware that resolves the scoped store for the session's current election.
pub async fn store_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let Some(session) = request.extensions().get::<Session>() else {
        return next.run(request).await;
    };

    match &session.user {
        // A committee session only reaches app routes while correcting the
        // paper documents of an imported stream: it then gets a
        // paper-corrections store view, so its events are wrapped and
        // persisted on the CSB stream and it can never create a `PgStore` in
        // its CSB stream partition. Otherwise it belongs on the CSB routes.
        SessionUser::CentralElectoralCommittee {
            user,
            election,
            paper_correction_stream_id,
        } => {
            let Some(stream_id) = *paper_correction_stream_id else {
                return Redirect::to(&CsbIndexPath {}.to_string()).into_response();
            };

            // Finalising (and generating documents) is not part of paper
            // corrections: the documents were already handed in on paper.
            let path = request.uri().path();
            if path.starts_with(FinalisePath::PATH) || path.starts_with("/generate/") {
                return Redirect::to(&PgIndexPath.to_string()).into_response();
            }

            let user = user.clone();
            let resolved = state
                .csb_store_registry
                .get_store(stream_id, *election)
                .await;
            inject_loaded_store(&state, resolved, request, next, |store| {
                CsbStore::acting_as(store, user).paper_corrections()
            })
            .await
        }
        SessionUser::PoliticalGroup {
            stream_id,
            election,
            ..
        } => {
            // Redirect to `/select-election` when the session has not yet
            // picked an election.
            let Some(election) = *election else {
                return Redirect::to(&SelectElectionPath.to_string()).into_response();
            };

            let resolved = state.store_for_stream(*stream_id, election, false).await;
            let limits = state.config.rate_limits;
            inject_loaded_store(&state, resolved, request, next, move |store| {
                PgStore::own(store).with_limits(limits)
            })
            .await
        }
    }
}

/// Middleware that loads the global CSB main store for the session's current
/// election and injects it into request extensions.
pub async fn csb_store_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let Some(session) = request.extensions().get::<Session>() else {
        return next.run(request).await;
    };

    // Only the central electoral committee may reach CSB routes; a committee
    // identity always carries its election.
    let SessionUser::CentralElectoralCommittee { election, .. } = &session.user else {
        return AppError::Unauthorised.into_response();
    };

    let resolved = state.csb_main_store(*election).await;
    inject_loaded_store(&state, resolved, request, next, |store| store).await
}

/// Replay a resolved store's latest events, wrap it into the extension value
/// handlers extract, inject it into the request, and continue down the
/// middleware chain.
async fn inject_loaded_store<D, E>(
    state: &AppState,
    resolved: Result<Store<D>, AppError>,
    mut request: Request,
    next: Next,
    wrap: impl FnOnce(Store<D>) -> E,
) -> Response
where
    D: StoreData,
    D::Event: Serialize + DeserializeOwned,
    E: Clone + Send + Sync + 'static,
{
    // A failure to resolve or replay is an  infrastructure problem, so it trips
    // the maintenance gate rather than serving a stale or missing store.
    let store = match resolved {
        Ok(store) => store,
        Err(err) => return handle_db_error(&state.db_health, err, &request),
    };

    // catch up with the latest events
    if let Err(err) = store.load().await {
        return handle_db_error(&state.db_health, err, &request);
    }

    request.extensions_mut().insert(wrap(store));

    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{session::hash_token, session_extractor::user_agent_hash};
    use axum::{
        Router,
        body::Body,
        http::{Request as HttpRequest, StatusCode, header},
        middleware,
        routing::get,
    };
    use tower::ServiceExt;

    use crate::{AppState, SESSION_COOKIE_NAME, Session, test_utils::response_body_string};

    /// Redirects to the login start page when no session cookie is present.
    #[tokio::test]
    async fn middleware_redirects_to_login_without_cookie() {
        let state = AppState::new_for_tests().await;
        let app = Router::new()
            .route(
                "/",
                get(|session: Session| async move { session.token_hash().to_string() }),
            )
            .layer(middleware::from_fn_with_state(
                state.clone(),
                session_middleware,
            ))
            .with_state(state);

        let response = app
            .oneshot(HttpRequest::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/login");
    }

    /// Reuses the existing session when the cookie is provided.
    #[tokio::test]
    async fn middleware_reuses_session_with_cookie() {
        let state = AppState::new_for_tests().await;
        let app = Router::new()
            .route(
                "/",
                get(|session: Session| async move { session.token_hash().to_string() }),
            )
            .layer(middleware::from_fn_with_state(
                state.clone(),
                session_middleware,
            ))
            .with_state(state.clone());

        let session = Session::new_test();
        let token = session.token_string();
        state.sessions.insert(session).await;
        let cookie_value = format!("{SESSION_COOKIE_NAME}={token}");

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header(header::COOKIE, &cookie_value)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");

        let status = response.status();
        let sets_cookie = response.headers().get(header::SET_COOKIE).is_some();
        let returned_hash = response_body_string(response).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!sets_cookie);
        // Session reused: the handler saw it, keyed by the cookie token's hash.
        assert_eq!(returned_hash, hash_token(&token));
    }

    /// Builds an app whose only route echoes the session token hash behind the
    /// session middleware.
    fn session_app(state: AppState) -> Router {
        Router::new()
            .route(
                "/",
                get(|session: Session| async move { session.token_hash().to_string() }),
            )
            .layer(middleware::from_fn_with_state(
                state.clone(),
                session_middleware,
            ))
            .with_state(state)
    }

    /// Inserts a session pinned to `user_agent` and returns its raw cookie token.
    async fn insert_pinned_session(state: &AppState, user_agent: &str) -> String {
        let mut ua_headers = axum::http::HeaderMap::new();
        ua_headers.insert(header::USER_AGENT, user_agent.parse().unwrap());

        let mut session = Session::new_test();
        session.set_user_agent_hash(user_agent_hash(&ua_headers));
        let token = session.token_string();
        state.sessions.insert(session).await;
        token
    }

    /// Sets up an app with a session pinned to `browser-1`, then replays the cookie
    /// with `request_user_agent`. Returns the state, raw token, and the response.
    async fn replay_pinned_session_with_user_agent(
        request_user_agent: &str,
    ) -> (AppState, String, axum::response::Response) {
        let state = AppState::new_for_tests().await;
        let app = session_app(state.clone());
        let token = insert_pinned_session(&state, "browser-1").await;

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header(header::COOKIE, format!("{SESSION_COOKIE_NAME}={token}"))
                    .header(header::USER_AGENT, request_user_agent)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");

        (state, token, response)
    }

    /// A cookie replayed with a different User-Agent is rejected and dropped.
    #[tokio::test]
    async fn middleware_rejects_mismatched_user_agent() {
        let (state, token, response) = replay_pinned_session_with_user_agent("browser-2").await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/login");
        assert!(
            state
                .sessions
                .get_existing(Some(&token))
                .await
                .expect("load session")
                .is_none(),
            "a session rejected on UA mismatch must be dropped"
        );
    }

    /// The same client (matching User-Agent) is accepted and the session reused.
    #[tokio::test]
    async fn middleware_accepts_matching_user_agent() {
        let (_state, token, response) = replay_pinned_session_with_user_agent("browser-1").await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_body_string(response).await, hash_token(&token));
    }

    /// Builds an app with a POST route behind the session middleware, plus a
    /// session; returns the app, its cookie header value, and the raw CSRF token.
    async fn post_app_with_session(state: &AppState) -> (Router, String, String) {
        let app = Router::new()
            .route("/", axum::routing::post(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                session_middleware,
            ))
            .with_state(state.clone());

        let session = Session::new_test();
        let token = session.token_string();
        let csrf = session.csrf_token().0.clone();
        state.sessions.insert(session).await;

        (app, format!("{SESSION_COOKIE_NAME}={token}"), csrf)
    }

    fn urlencoded_post(cookie: &str, body: String) -> HttpRequest<Body> {
        HttpRequest::builder()
            .method("POST")
            .uri("/")
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap()
    }

    /// A POST without a CSRF token gets the styled rejection page.
    #[tokio::test]
    async fn middleware_rejects_post_without_csrf_token() {
        let state = AppState::new_for_tests().await;
        let (app, cookie, _csrf) = post_app_with_session(&state).await;

        let response = app
            .oneshot(urlencoded_post(&cookie, "a=b".into()))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_body_string(response).await;
        assert!(body.contains("Formulier verlopen"));
    }

    /// A POST with the session's token reaches the handler.
    #[tokio::test]
    async fn middleware_accepts_post_with_valid_csrf_token() {
        let state = AppState::new_for_tests().await;
        let (app, cookie, csrf) = post_app_with_session(&state).await;

        let response = app
            .oneshot(urlencoded_post(&cookie, format!("csrf_token={csrf}")))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
    }

    /// App routes behind session + store middleware whose handler echoes the CSB
    /// stream the injected store sends paper corrections to (empty when the store
    /// writes to its own stream).
    fn store_app(state: AppState) -> Router {
        Router::new()
            .route(
                "/",
                get(|store: crate::PgStore| async move {
                    store
                        .paper_corrections_stream_id()
                        .map(|id| id.to_string())
                        .unwrap_or_default()
                }),
            )
            .route("/finalise", get(|| async { "finalise" }))
            .route(
                "/generate/{locale}/documents.zip",
                get(|| async { "documents" }),
            )
            .layer(middleware::from_fn_with_state(
                state.clone(),
                store_middleware,
            ))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                session_middleware,
            ))
            .with_state(state)
    }

    /// Insert a committee session and return its cookie value. `correcting` sets
    /// the paper-corrections stream.
    async fn insert_committee_session(
        state: &AppState,
        correcting: Option<crate::StreamId>,
    ) -> String {
        let mut session = Session::new_test_committee();
        session
            .set_paper_correction_stream_id(correcting)
            .expect("committee session");
        let token = session.token_string();
        state.sessions.insert(session).await;
        format!("{SESSION_COOKIE_NAME}={token}")
    }

    /// Persist a CSB stream carrying a single import event and return its id.
    async fn seed_csb_stream(state: &AppState) -> crate::StreamId {
        let stream_id = crate::StreamId::new();
        let store = state
            .csb_store_for_stream(stream_id, crate::ElectionConfig::EK27)
            .await
            .expect("csb store");
        store
            .update(
                crate::CsbAction::Import {
                    hash: [0u8; 32],
                    source_stream_id: crate::StreamId::new(),
                    snapshot: Box::new(crate::PgStoreData::default()),
                }
                .by(crate::CsbUser::new_test()),
            )
            .await
            .expect("import");
        stream_id
    }

    /// A committee session that is not correcting paper documents stays off the
    /// app routes.
    #[tokio::test]
    async fn csb_session_without_correction_stream_is_redirected_to_csb_index() {
        let state = AppState::new_for_tests().await;
        let cookie = insert_committee_session(&state, None).await;

        let response = store_app(state)
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/csb");
    }

    /// A committee session in paper-corrections mode reaches app routes with a
    /// store that writes to the CSB stream.
    #[tokio::test]
    async fn csb_session_in_corrections_mode_gets_a_paper_corrections_store() {
        let state = AppState::new_for_tests().await;
        let stream_id = seed_csb_stream(&state).await;
        let cookie = insert_committee_session(&state, Some(stream_id)).await;

        let response = store_app(state)
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert_eq!(body, stream_id.to_string());
    }

    /// Neither the finalise flow nor the document ZIP it generates is
    /// reachable while correcting paper documents.
    #[tokio::test]
    async fn csb_session_in_corrections_mode_cannot_reach_finalise_or_documents() {
        let state = AppState::new_for_tests().await;
        let stream_id = seed_csb_stream(&state).await;
        let cookie = insert_committee_session(&state, Some(stream_id)).await;
        let app = store_app(state);

        for uri in ["/finalise", "/generate/nl/documents.zip"] {
            let response = app
                .clone()
                .oneshot(
                    HttpRequest::builder()
                        .uri(uri)
                        .header(header::COOKIE, &cookie)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .expect("response");

            assert_eq!(response.status(), StatusCode::SEE_OTHER, "{uri}");
            assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/");
        }
    }
}
