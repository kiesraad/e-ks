//! `POST /csb/login/finish`: receives the assertion the browser obtained from
//! the security key, verifies it against the key's last recorded login on
//! the CSB main stream, records this one there, and establishes a
//! committee-scoped session.

use std::net::{IpAddr, SocketAddr};

use axum::{
    Extension,
    extract::{ConnectInfo, State},
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;
use serde::Deserialize;
use tracing::{info, warn};

use crate::{
    AppError, AppRequestState, CsbMainStore, CsbUser, CsbUsername, Form, Locale, Session,
    auth::{
        csb_webauthn::{AssertionError, LoginFailure, VerifiedLogin, verify_login},
        session_extractor::{establish_session, user_agent_hash},
    },
    csb::{
        index::CsbIndexPath,
        login::{CsbLoginFinishPath, pages::login::login_failed, require_webauthn},
    },
    utils::peer_ip_label,
};

/// The ceremony result, as the browser script serialises it.
#[derive(Debug, Deserialize)]
pub struct CredentialForm {
    credential: String,
}

/// Alerting matches failed attempts on `event = "csb.login_failed"`.
pub async fn login_finish<S: AppRequestState>(
    _: CsbLoginFinishPath,
    State(state): State<S>,
    connect_info: Option<Extension<ConnectInfo<SocketAddr>>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<CredentialForm>,
) -> Result<Response, AppError> {
    let config = require_webauthn(state.config())?;
    let store = state
        .csb_main_store(state.config().default_election)
        .await?;
    // This route sits outside `csb_store_middleware`, which would otherwise
    // catch the cached projection up with logins recorded elsewhere.
    store.load().await?;
    let peer = connect_info.map(|Extension(ConnectInfo(addr))| addr.ip());

    let verified = verify_login(
        state.pending_requests(),
        config,
        &form.credential,
        |credential_id| store.last_sign_count(credential_id),
    )
    .await;
    match verified {
        Ok(login) => establish_committee_session(&state, &store, login, jar, &headers, peer).await,
        Err(failure) => Ok(login_failed_with(failure, peer)),
    }
}

/// Reports the failed attempt and renders the generic failure page.
fn login_failed_with(failure: LoginFailure, peer: Option<IpAddr>) -> Response {
    warn!(
        event = "csb.login_failed",
        ip = %peer_ip_label(peer),
        user = failure.username().map(CsbUsername::as_str),
        reason = %failure,
        "CSB login attempt failed"
    );
    login_failed()
}

/// Records the login, with the key's signature counter, on the shared CSB
/// main stream (WebAuthn §7.2 step 24; the audit log shows it too) as one
/// step with the counter check, and creates the committee session.
async fn establish_committee_session<S: AppRequestState>(
    state: &S,
    store: &CsbMainStore,
    login: VerifiedLogin,
    jar: CookieJar,
    headers: &HeaderMap,
    peer: Option<IpAddr>,
) -> Result<Response, AppError> {
    let VerifiedLogin {
        username,
        credential_id,
        sign_count,
    } = login;
    let user = CsbUser::SecurityKey {
        username: username.clone(),
    };
    let election = state.config().default_election;

    let mut session = Session::for_committee(user.clone(), election, Locale::default());
    session.set_user_agent_hash(user_agent_hash(headers));

    if !store
        .record_security_key_login(user, credential_id, sign_count)
        .await?
    {
        let failure = LoginFailure::Rejected {
            username,
            error: AssertionError::SignCountNotAdvanced,
        };
        return Ok(login_failed_with(failure, peer));
    }

    let jar = establish_session(state.sessions(), jar, session).await?;

    info!("committee member {username} logged in to the CSB");
    Ok((jar, Redirect::to(&CsbIndexPath {}.to_string())).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, http::StatusCode};
    use tower::ServiceExt;
    use tracing_test::traced_test;

    use crate::{
        AppState, CsbMainAction,
        auth::csb_webauthn::test_support::{self, TestAssertion},
        csb::login::pages::tests::{cookie_value, form_post, location, start_login, test_app},
        store::StoreEvent,
    };

    const LOGIN_ERROR_LOCATION: &str = "/csb/login?error=1";

    async fn finish_login(app: &Router, credential: &str) -> Response {
        let request = form_post("/csb/login/finish", &[("credential", credential)]);
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
        let challenge = start_login(&app).await;

        let response = finish_login(&app, &test_support::signed_assertion(&challenge)).await;

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
            store.snapshot().events.as_slice(),
            &[StoreEvent {
                payload: crate::CsbMainEvent {
                    user: CsbUser::SecurityKey { .. },
                    action: CsbMainAction::SecurityKeyLogin { sign_count: 7, .. },
                },
                ..
            }]
        ));
        assert_eq!(
            store.last_sign_count(&test_support::test_credential_id()),
            7
        );
    }

    /// The counter of the last login is kept on the main stream: the next
    /// login with the same key must report a higher one.
    #[tokio::test]
    async fn signature_counter_is_checked_against_the_last_login() {
        let (state, app) = test_app().await;
        let app = &app;
        let login = |count: u32| async move {
            let challenge = start_login(app).await;
            let mut assertion = TestAssertion::new(&challenge);
            assertion.sign_count = count;
            finish_login(app, &assertion.json()).await
        };

        assert_eq!(location(&login(7).await), "/csb");
        assert_login_failed(&login(7).await);
        assert_login_failed(&login(6).await);
        assert_eq!(location(&login(8).await), "/csb");

        let store = state
            .csb_main_store(state.config.default_election)
            .await
            .expect("main store");
        assert_eq!(
            store.last_sign_count(&test_support::test_credential_id()),
            8
        );
    }

    /// The same challenge and assertion cannot log in twice.
    #[tokio::test]
    async fn replayed_assertion_is_rejected() {
        let (_state, app) = test_app().await;
        let challenge = start_login(&app).await;
        let assertion = test_support::signed_assertion(&challenge);

        let first = finish_login(&app, &assertion).await;
        assert_eq!(location(&first), "/csb");

        assert_login_failed(&finish_login(&app, &assertion).await);
    }

    /// A failed attempt burns the challenge too: the browser has to reload.
    #[tokio::test]
    async fn failed_attempt_burns_the_challenge() {
        let (_state, app) = test_app().await;
        let challenge = start_login(&app).await;

        let mut bad = TestAssertion::new(&challenge);
        bad.origin = "https://evil.example".to_string();
        assert_login_failed(&finish_login(&app, &bad.json()).await);

        let good = test_support::signed_assertion(&challenge);
        assert_login_failed(&finish_login(&app, &good).await);
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
            let challenge = start_login(&app).await;
            let mut assertion = TestAssertion::new(&challenge);
            tamper(&mut assertion);
            assert_login_failed(&finish_login(&app, &assertion.json()).await);
        }
    }

    #[traced_test]
    #[tokio::test]
    async fn failed_attempt_is_reported() {
        let (_state, app) = test_app().await;
        let challenge = start_login(&app).await;

        let mut assertion = TestAssertion::new(&challenge);
        assertion.user_verified = false;
        let mut request = form_post("/csb/login/finish", &[("credential", &assertion.json())]);
        let peer: SocketAddr = "203.0.113.9:4000".parse().expect("socket address");
        request.extensions_mut().insert(ConnectInfo(peer));
        assert_login_failed(&app.oneshot(request).await.expect("response"));

        assert!(logs_contain("csb.login_failed"));
        assert!(logs_contain("203.0.113.9"));
        assert!(logs_contain("alice"));
        assert!(logs_contain("user verification flag is not set"));
    }

    /// A key that is not configured cannot log in, even with a valid
    /// signature for the challenge.
    #[tokio::test]
    async fn unknown_security_key_is_rejected() {
        let (_state, app) = test_app().await;
        let challenge = start_login(&app).await;

        let mut assertion = TestAssertion::new(&challenge);
        assertion.credential_id = vec![0x22; 64].into();
        assert_login_failed(&finish_login(&app, &assertion.json()).await);
    }

    /// The key must have signed a challenge this server issued: one it never
    /// issued, or one of the wrong size, fails, and the real challenge stays
    /// usable.
    #[tokio::test]
    async fn unissued_challenge_is_rejected() {
        let (_state, app) = test_app().await;
        let challenge = start_login(&app).await;

        assert_login_failed(&finish_login(&app, &test_support::signed_assertion(&[9; 32])).await);
        assert_login_failed(&finish_login(&app, &test_support::signed_assertion(&[9; 31])).await);

        let response = finish_login(&app, &test_support::signed_assertion(&challenge)).await;
        assert_eq!(location(&response), "/csb");
    }

    #[tokio::test]
    async fn malformed_credential_is_rejected() {
        let (_state, app) = test_app().await;

        for credential in ["", "{}", "not json", r#"{"rawId":"AAAA","response":{}}"#] {
            assert_login_failed(&finish_login(&app, credential).await);
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

        let store = state
            .csb_main_store(state.config.default_election)
            .await
            .expect("main store");
        let _ = establish_committee_session(
            &state,
            &store,
            test_support::alice_login(),
            jar,
            &HeaderMap::new(),
            None,
        )
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

        let store = state
            .csb_main_store(state.config.default_election)
            .await
            .expect("main store");
        let response = establish_committee_session(
            &state,
            &store,
            test_support::alice_login(),
            CookieJar::new(),
            &HeaderMap::new(),
            None,
        )
        .await
        .expect("response");
        let token = cookie_value(&response, crate::SESSION_COOKIE_NAME).expect("session cookie");

        let jar = CookieJar::new().add(axum_extra::extract::cookie::Cookie::new(
            crate::SESSION_COOKIE_NAME,
            token,
        ));
        let _ = state.logout_session(jar).await.expect("logout session");

        let store = state
            .csb_main_store(state.config.default_election)
            .await
            .expect("main store");
        assert!(
            store.snapshot().events.iter().any(|event| matches!(
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
