//! `POST /csb/login/finish`: receives the assertion the browser obtained from
//! the security key, verifies it, and establishes a committee-scoped session.

use axum::{
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use serde::Deserialize;
use tracing::{info, warn};

use crate::{
    AppError, AppRequestState, CsbMainAction, CsbUser, CsbUsername, CsbWebauthnConfig, Form,
    Locale, Session,
    auth::session_extractor::{establish_session, user_agent_hash},
    csb::{
        index::CsbIndexPath,
        login::{
            CsbLoginFinishPath,
            assertion::{Assertion, Expected},
            pages::login::login_failed,
            pending_challenge_id, require_webauthn,
            state_cookie::{STATE_COOKIE_NAME, build_state_removal_cookie},
        },
    },
};

/// The ceremony result, as the browser script serialises it.
#[derive(Debug, Deserialize)]
pub struct CredentialForm {
    credential: String,
}

pub async fn login_finish<S: AppRequestState>(
    _: CsbLoginFinishPath,
    State(state): State<S>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<CredentialForm>,
) -> Result<Response, AppError> {
    let config = require_webauthn(state.config())?;

    let Some(username) = verify_login(&state, config, &jar, &form.credential).await else {
        return Ok(login_failed(jar));
    };

    establish_committee_session(&state, username, jar, &headers).await
}

/// Burns the browser's challenge, finds the key that answered, and verifies
/// the assertion. Fails closed: `None` means "login failed", whatever the
/// reason.
async fn verify_login<S: AppRequestState>(
    state: &S,
    config: &CsbWebauthnConfig,
    jar: &CookieJar,
    credential: &str,
) -> Option<CsbUsername> {
    let challenge = issued_challenge(state, jar).await?;
    let assertion: Assertion = serde_json::from_str(credential)
        .inspect_err(|err| warn!("CSB login assertion is malformed: {err}"))
        .ok()?;
    let Some(user) = config.user_by_credential_id(&assertion.raw_id) else {
        warn!("CSB login assertion is for an unknown security key");
        return None;
    };
    assertion
        .verify(&Expected {
            challenge: &challenge,
            origin: &config.origin,
            rp_id_hash: &config.rp_id_hash(),
            public_key: &user.public_key,
        })
        .inspect_err(|err| warn!("CSB login assertion rejected: {err}"))
        .ok()?;
    Some(user.username.clone())
}

/// The challenge this browser was issued, consumed from the one-shot store:
/// a second finish with the same challenge is a replay, whether or not the
/// first one succeeded.
async fn issued_challenge<S: AppRequestState>(state: &S, jar: &CookieJar) -> Option<Vec<u8>> {
    let Some(challenge) = jar
        .get(STATE_COOKIE_NAME)
        .map(|cookie| cookie.value().to_string())
    else {
        warn!("CSB login finished without a challenge cookie");
        return None;
    };
    if !state
        .pending_requests()
        .consume_if_pending(&pending_challenge_id(&challenge))
        .await
    {
        warn!("CSB login challenge is unknown, expired, or replayed");
        return None;
    }
    BASE64_URL_SAFE_NO_PAD.decode(&challenge).ok()
}

/// Creates the committee session for an authenticated user and records the
/// login on the shared CSB main stream for the audit log.
async fn establish_committee_session<S: AppRequestState>(
    state: &S,
    username: CsbUsername,
    jar: CookieJar,
    headers: &HeaderMap,
) -> Result<Response, AppError> {
    let user = CsbUser::SecurityKey {
        username: username.clone(),
    };
    let election = state.config().default_election;

    let mut session = Session::for_committee(user.clone(), election, Locale::default());
    session.set_user_agent_hash(user_agent_hash(headers));

    let store = state.csb_main_store(election).await?;
    store.update(CsbMainAction::Login.by(user)).await?;

    let jar = establish_session(state.sessions(), jar, session).await;

    info!("committee member {username} logged in to the CSB");
    Ok((
        jar.remove(build_state_removal_cookie()),
        Redirect::to(&CsbIndexPath {}.to_string()),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, http::StatusCode};
    use tower::ServiceExt;

    use crate::{
        AppState,
        csb::login::{
            pages::tests::{cookie_value, form_post, location, start_login, test_app},
            test_support::{self, TestAssertion},
        },
        store::StoreEvent,
    };

    const LOGIN_ERROR_LOCATION: &str = "/csb/login?error=1";

    async fn finish_login(app: &Router, cookie: Option<&str>, credential: &str) -> Response {
        let request = form_post("/csb/login/finish", &[("credential", credential)], cookie);
        app.clone().oneshot(request).await.expect("response")
    }

    fn assert_login_failed(response: &Response) {
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(location(response), LOGIN_ERROR_LOCATION);
        assert!(cookie_value(response, crate::SESSION_COOKIE_NAME).is_none());
    }

    #[tokio::test]
    async fn valid_assertion_establishes_committee_session() {
        let (state, app) = test_app().await;
        let (cookie, challenge) = start_login(&app).await;

        let response = finish_login(
            &app,
            Some(&cookie),
            &test_support::signed_assertion(&challenge),
        )
        .await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(location(&response), "/csb");
        let token = cookie_value(&response, crate::SESSION_COOKIE_NAME).expect("session cookie");
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
                    user: CsbUser::SecurityKey { .. },
                    action: CsbMainAction::Login,
                },
                ..
            }]
        ));
    }

    /// The same challenge and assertion cannot log in twice.
    #[tokio::test]
    async fn replayed_assertion_is_rejected() {
        let (_state, app) = test_app().await;
        let (cookie, challenge) = start_login(&app).await;
        let assertion = test_support::signed_assertion(&challenge);

        let first = finish_login(&app, Some(&cookie), &assertion).await;
        assert_eq!(location(&first), "/csb");

        assert_login_failed(&finish_login(&app, Some(&cookie), &assertion).await);
    }

    /// A failed attempt burns the challenge too: the browser has to reload.
    #[tokio::test]
    async fn failed_attempt_burns_the_challenge() {
        let (_state, app) = test_app().await;
        let (cookie, challenge) = start_login(&app).await;

        let mut bad = TestAssertion::new(&challenge);
        bad.origin = "https://evil.example".to_string();
        assert_login_failed(&finish_login(&app, Some(&cookie), &bad.json()).await);

        let good = test_support::signed_assertion(&challenge);
        assert_login_failed(&finish_login(&app, Some(&cookie), &good).await);
    }

    #[tokio::test]
    async fn assertions_failing_a_check_are_rejected() {
        let (_state, app) = test_app().await;

        let tampers: [fn(&mut TestAssertion); 4] = [
            |a| a.user_verified = false,
            |a| a.challenge = vec![9; 32],
            |a| a.rp_id = "example.nl".to_string(),
            |a| {
                a.signing_key =
                    p256::ecdsa::SigningKey::from_bytes(&[0x43; 32].into()).expect("key")
            },
        ];
        for tamper in tampers {
            let (cookie, challenge) = start_login(&app).await;
            let mut assertion = TestAssertion::new(&challenge);
            tamper(&mut assertion);
            assert_login_failed(&finish_login(&app, Some(&cookie), &assertion.json()).await);
        }
    }

    /// A key that is not configured cannot log in, even with a valid
    /// signature for the challenge.
    #[tokio::test]
    async fn unknown_security_key_is_rejected() {
        let (_state, app) = test_app().await;
        let (cookie, challenge) = start_login(&app).await;

        let mut assertion = TestAssertion::new(&challenge);
        assertion.credential_id = vec![0x22; 64];
        assert_login_failed(&finish_login(&app, Some(&cookie), &assertion.json()).await);
    }

    /// The challenge must come back in this browser's cookie: without one,
    /// with one for a challenge that was never issued, or with garbage, the
    /// login fails and the real challenge stays usable.
    #[tokio::test]
    async fn finish_without_valid_challenge_cookie_is_rejected() {
        let (_state, app) = test_app().await;
        let (cookie, challenge) = start_login(&app).await;
        let assertion = test_support::signed_assertion(&challenge);

        assert_login_failed(&finish_login(&app, None, &assertion).await);
        assert_login_failed(&finish_login(&app, Some("bm90LWlzc3VlZA"), &assertion).await);
        assert_login_failed(&finish_login(&app, Some("not base64!"), &assertion).await);

        let response = finish_login(&app, Some(&cookie), &assertion).await;
        assert_eq!(location(&response), "/csb");
    }

    #[tokio::test]
    async fn malformed_credential_is_rejected() {
        let (_state, app) = test_app().await;

        for credential in ["", "{}", "not json", r#"{"rawId":"AAAA","response":{}}"#] {
            // Each attempt needs a fresh challenge: a failed one burns it.
            let (cookie, _) = start_login(&app).await;
            assert_login_failed(&finish_login(&app, Some(&cookie), credential).await);
        }
    }

    /// The session cookie of a previous login is dropped (fixation defence).
    #[tokio::test]
    async fn establish_session_drops_previous_session() {
        let state = AppState::new_for_tests_with_config(test_support::webauthn_test_config()).await;
        let old = Session::new_test();
        let old_token = old.token_string();
        state.sessions.insert(old).await;
        let jar = CookieJar::new().add(axum_extra::extract::cookie::Cookie::new(
            crate::SESSION_COOKIE_NAME,
            old_token.clone(),
        ));

        let _ = establish_committee_session(&state, test_support::alice(), jar, &HeaderMap::new())
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

    /// Signing out a committee session is recorded on the shared CSB main
    /// stream, not on a PG stream.
    #[tokio::test]
    async fn logout_is_recorded_on_the_csb_main_stream() {
        use auth_service::AuthState;

        let state = AppState::new_for_tests_with_config(test_support::webauthn_test_config()).await;

        let response = establish_committee_session(
            &state,
            test_support::alice(),
            CookieJar::new(),
            &HeaderMap::new(),
        )
        .await
        .expect("response");
        let token = cookie_value(&response, crate::SESSION_COOKIE_NAME).expect("session cookie");

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
                    user: CsbUser::SecurityKey { .. },
                    action: CsbMainAction::Logout,
                }
            )),
            "logout must be recorded on the CSB main stream"
        );
    }
}
