//! GET `/csb/login/callback`: GitHub redirects here after consent. Validates
//! the `state` nonce (browser-bound cookie plus one-shot pending check),
//! exchanges the code server-side, enforces the account-id allowlist, and
//! establishes a committee-scoped session.

use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;
use serde::Deserialize;
use tracing::{info, warn};

use crate::{
    AppError, AppRequestState, CsbMainAction, CsbUser, GithubOauthConfig, Locale, Session,
    auth::session_extractor::{establish_session, user_agent_hash},
    csb::{
        index::CsbIndexPath,
        login::{
            CsbLoginCallbackPath, CsbLoginPath,
            github::{self, GithubIdentity},
            pending_state_id, require_github_oauth,
            state_cookie::{STATE_COOKIE_NAME, build_state_removal_cookie},
        },
    },
    form::csrf_token_matches,
};

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    /// Set by GitHub when the user cancelled or the request was rejected.
    error: Option<String>,
}

pub async fn callback<S: AppRequestState>(
    _: CsbLoginCallbackPath,
    State(state): State<S>,
    Query(query): Query<CallbackQuery>,
    jar: CookieJar,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let config = require_github_oauth(state.config())?;

    let Some(code) = validate_callback(&state, &query, &jar).await else {
        return Ok(login_failed(jar));
    };

    let identity = match github::authenticated_user(config, &code).await {
        Ok(identity) => identity,
        Err(err) => {
            warn!("GitHub OAuth code exchange failed: {err}");
            return Ok(login_failed(jar));
        }
    };

    complete_login(&state, config, identity, jar, &headers).await
}

/// Validates the `state` round-trip and returns the authorization code.
/// Fails closed.
async fn validate_callback<S: AppRequestState>(
    state: &S,
    query: &CallbackQuery,
    jar: &CookieJar,
) -> Option<String> {
    if let Some(error) = &query.error {
        warn!("GitHub OAuth callback reported an error: {error}");
        return None;
    }
    let (Some(code), Some(nonce)) = (&query.code, &query.state) else {
        warn!("GitHub OAuth callback missing code or state");
        return None;
    };
    nonce_is_valid(state, nonce, jar)
        .await
        .then(|| code.clone())
}

/// The nonce must match this browser's state cookie (constant-time) and still
/// be pending in the one-shot store (expiry and replay defence).
async fn nonce_is_valid<S: AppRequestState>(state: &S, nonce: &str, jar: &CookieJar) -> bool {
    let cookie_matches = jar
        .get(STATE_COOKIE_NAME)
        .is_some_and(|cookie| csrf_token_matches(nonce, cookie.value()));
    if !cookie_matches {
        warn!("GitHub OAuth callback state does not match the browser's state cookie");
        return false;
    }
    if !state
        .pending_requests()
        .consume_if_pending(&pending_state_id(nonce))
        .await
    {
        warn!("GitHub OAuth callback state is unknown, expired, or replayed");
        return false;
    }
    true
}

/// Enforces the allowlist, then establishes the committee session.
async fn complete_login<S: AppRequestState>(
    state: &S,
    config: &GithubOauthConfig,
    identity: GithubIdentity,
    jar: CookieJar,
    headers: &HeaderMap,
) -> Result<Response, AppError> {
    if !config.allowed_user_ids.contains(&identity.id) {
        warn!("GitHub user {} is not on the CSB allowlist", identity.id);
        return Ok(login_failed(jar));
    }
    establish_committee_session(state, identity, jar, headers).await
}

/// Creates the committee session for an allowlisted GitHub user and records
/// the login on the shared CSB main stream for the audit log.
async fn establish_committee_session<S: AppRequestState>(
    state: &S,
    identity: GithubIdentity,
    jar: CookieJar,
    headers: &HeaderMap,
) -> Result<Response, AppError> {
    let github_user_id = identity.id;
    let user = CsbUser::Github {
        user_id: identity.id,
        login: identity.login,
    };
    let election = state.config().default_election;

    let mut session = Session::for_committee(user.clone(), election, Locale::default());
    session.set_user_agent_hash(user_agent_hash(headers));

    let store = state.csb_main_store(election).await?;
    store.update(CsbMainAction::Login.by(user)).await?;

    let jar = establish_session(state.sessions(), jar, session).await;

    info!("GitHub user {github_user_id} logged in to the CSB");
    Ok((
        jar.remove(build_state_removal_cookie()),
        Redirect::to(&CsbIndexPath {}.to_string()),
    )
        .into_response())
}

/// Clears the state cookie and sends the browser back to the login page with
/// a generic error, deliberately not revealing which check failed.
fn login_failed(jar: CookieJar) -> Response {
    let login_error = format!("{}?error=github", CsbLoginPath);
    (
        jar.remove(build_state_removal_cookie()),
        Redirect::to(&login_error),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{StatusCode, header};

    use crate::{
        AppState,
        csb::login::{state_cookie::build_state_cookie, test_support},
        store::StoreEvent,
    };

    const LOGIN_ERROR_LOCATION: &str = "/csb/login?error=github";

    /// The allowlisted account, with the login GitHub returns for it.
    fn allowed_identity() -> GithubIdentity {
        GithubIdentity {
            id: test_support::allowed_user_id(),
            login: Some("octocat".parse().expect("valid login")),
        }
    }

    async fn github_test_state() -> AppState {
        AppState::new_for_tests_with_config(test_support::github_test_config()).await
    }

    fn callback_query(
        code: Option<&str>,
        nonce: Option<&str>,
        error: Option<&str>,
    ) -> CallbackQuery {
        CallbackQuery {
            code: code.map(str::to_string),
            state: nonce.map(str::to_string),
            error: error.map(str::to_string),
        }
    }

    fn jar_with_state_cookie(nonce: &str) -> CookieJar {
        CookieJar::new().add(build_state_cookie(nonce.to_string()))
    }

    fn location(response: &Response) -> &str {
        response
            .headers()
            .get(header::LOCATION)
            .expect("redirect location")
            .to_str()
            .expect("ascii location")
    }

    /// The session token from the response's `Set-Cookie`, if one was set.
    fn session_token(response: &Response) -> Option<String> {
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter_map(|value| value.split(';').next())
            .filter_map(|pair| pair.split_once('='))
            .find(|(name, _)| *name == crate::SESSION_COOKIE_NAME)
            .map(|(_, token)| token.to_string())
    }

    #[tokio::test]
    async fn callback_is_not_found_without_github_config() {
        let state = AppState::new_for_tests().await;

        let err = callback(
            CsbLoginCallbackPath,
            State(state),
            Query(callback_query(Some("c0de"), Some("n0nce"), None)),
            CookieJar::new(),
            HeaderMap::new(),
        )
        .await
        .expect_err("404 without config");

        assert!(matches!(err, AppError::GenericNotFound));
    }

    #[tokio::test]
    async fn callback_rejects_reported_error() {
        let state = github_test_state().await;
        state
            .pending_requests
            .register(pending_state_id("n0nce"))
            .await;

        let response = callback(
            CsbLoginCallbackPath,
            State(state),
            Query(callback_query(
                Some("c0de"),
                Some("n0nce"),
                Some("access_denied"),
            )),
            jar_with_state_cookie("n0nce"),
            HeaderMap::new(),
        )
        .await
        .expect("response");

        assert_eq!(location(&response), LOGIN_ERROR_LOCATION);
        assert!(session_token(&response).is_none());
    }

    #[tokio::test]
    async fn callback_rejects_state_without_matching_cookie() {
        let state = github_test_state().await;
        state
            .pending_requests
            .register(pending_state_id("n0nce"))
            .await;

        for jar in [CookieJar::new(), jar_with_state_cookie("other")] {
            let response = callback(
                CsbLoginCallbackPath,
                State(state.clone()),
                Query(callback_query(Some("c0de"), Some("n0nce"), None)),
                jar,
                HeaderMap::new(),
            )
            .await
            .expect("response");

            assert_eq!(location(&response), LOGIN_ERROR_LOCATION);
            assert!(session_token(&response).is_none());
        }
    }

    #[tokio::test]
    async fn callback_rejects_unknown_or_replayed_state() {
        let state = github_test_state().await;
        // Nothing registered: an attacker-minted nonce (or a replay after the
        // one-shot consume) must be rejected before any GitHub call is made.
        let response = callback(
            CsbLoginCallbackPath,
            State(state),
            Query(callback_query(Some("c0de"), Some("n0nce"), None)),
            jar_with_state_cookie("n0nce"),
            HeaderMap::new(),
        )
        .await
        .expect("response");

        assert_eq!(location(&response), LOGIN_ERROR_LOCATION);
        assert!(session_token(&response).is_none());
    }

    #[tokio::test]
    async fn callback_rejects_missing_code_or_state() {
        let state = github_test_state().await;

        for (code, nonce) in [(None, Some("n0nce")), (Some("c0de"), None), (None, None)] {
            let response = callback(
                CsbLoginCallbackPath,
                State(state.clone()),
                Query(callback_query(code, nonce, None)),
                jar_with_state_cookie("n0nce"),
                HeaderMap::new(),
            )
            .await
            .expect("response");

            assert_eq!(location(&response), LOGIN_ERROR_LOCATION);
        }
    }

    #[tokio::test]
    async fn complete_login_rejects_user_not_on_allowlist() {
        let state = github_test_state().await;
        let config = state.config.github_oauth.clone().expect("github config");
        let intruder = GithubIdentity {
            id: "999".parse().expect("valid id"),
            login: Some("intruder".parse().expect("valid login")),
        };

        let response = complete_login(
            &state,
            &config,
            intruder,
            CookieJar::new(),
            &HeaderMap::new(),
        )
        .await
        .expect("response");

        assert_eq!(location(&response), LOGIN_ERROR_LOCATION);
        assert!(session_token(&response).is_none());
    }

    #[tokio::test]
    async fn complete_login_creates_committee_session_and_audit_event() {
        let state = github_test_state().await;
        let config = state.config.github_oauth.clone().expect("github config");

        let response = complete_login(
            &state,
            &config,
            allowed_identity(),
            CookieJar::new(),
            &HeaderMap::new(),
        )
        .await
        .expect("response");

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(location(&response), "/csb");

        let token = session_token(&response).expect("session cookie");
        let session = state
            .sessions
            .get(&token)
            .await
            .expect("load session")
            .expect("session");
        assert_eq!(session.scope(), crate::Scope::CentralElectoralCommittee);
        assert_eq!(session.user.election(), Some(state.config.default_election));

        let store = state
            .csb_main_store(state.config.default_election)
            .await
            .expect("main store");
        assert!(matches!(
            store.data.read().events.as_slice(),
            &[StoreEvent {
                payload: crate::CsbMainEvent {
                    user: CsbUser::Github { .. },
                    action: CsbMainAction::Login,
                },
                ..
            }]
        ));
    }

    /// Signing out a committee session is recorded on the shared CSB main
    /// stream, not on a PG stream.
    #[tokio::test]
    async fn logout_is_recorded_on_the_csb_main_stream() {
        use auth_service::AuthState;

        let state = github_test_state().await;
        let config = state.config.github_oauth.clone().expect("github config");

        let response = complete_login(
            &state,
            &config,
            allowed_identity(),
            CookieJar::new(),
            &HeaderMap::new(),
        )
        .await
        .expect("response");
        let token = session_token(&response).expect("session cookie");

        let jar = CookieJar::new().add(axum_extra::extract::cookie::Cookie::new(
            crate::SESSION_COOKIE_NAME,
            token,
        ));
        let _ = state.logout_session(jar).await;

        let store = state
            .csb_main_store(state.config.default_election)
            .await
            .expect("main store");
        assert!(
            store.data.read().events.iter().any(|event| matches!(
                event.payload,
                crate::CsbMainEvent {
                    user: CsbUser::Github { .. },
                    action: CsbMainAction::Logout,
                }
            )),
            "logout must be recorded on the CSB main stream"
        );
    }

    #[tokio::test]
    async fn complete_login_drops_previous_session() {
        let state = github_test_state().await;
        let config = state.config.github_oauth.clone().expect("github config");
        let old = Session::new_test();
        let old_token = old.token_string();
        state.sessions.insert(old).await;
        let jar = CookieJar::new().add(axum_extra::extract::cookie::Cookie::new(
            crate::SESSION_COOKIE_NAME,
            old_token.clone(),
        ));

        let _ = complete_login(&state, &config, allowed_identity(), jar, &HeaderMap::new())
            .await
            .expect("response");

        assert!(
            state
                .sessions
                .get_existing(Some(&old_token))
                .await
                .expect("load session")
                .is_none()
        );
    }
}
