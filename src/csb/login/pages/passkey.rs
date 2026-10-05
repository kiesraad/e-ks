//! `POST /csb/login/passkey/start` and `POST /csb/login/passkey/finish`: the
//! passkey login ceremony, driven by the login page's script. Both run before
//! a session exists; the global fetch-metadata layer blocks cross-site POSTs,
//! and the ceremony cookie binds the finish to the browser that started.

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use axum_extra::extract::CookieJar;
use serde::Deserialize;
use tracing::{info, warn};
use webauthn_rs::prelude::{
    Passkey, PasskeyAuthentication, PublicKeyCredential, RequestChallengeResponse,
};

use crate::{
    AppError, AppRequestState, CsbUser, PasskeyAccountId, PasskeyAccountName, PasskeyLogin,
    auth::passkey::{
        LoginCeremony, Purpose, build_state_removal_cookie, pending_login_id, require_passkey_login,
    },
    csb::login::{
        CsbPasskeyLoginFinishPath, CsbPasskeyLoginStartPath, establish_committee_session,
    },
    form::generate_csrf_token,
};

#[derive(Debug, Deserialize)]
pub struct PasskeyLoginStart {
    name: String,
}

/// Returns the request options for the named account, or a decoy of the
/// same shape when no such account exists, and seals the ceremony state into
/// the browser's cookie.
pub async fn start<S: AppRequestState>(
    _: CsbPasskeyLoginStartPath,
    State(state): State<S>,
    jar: CookieJar,
    payload: Result<Json<PasskeyLoginStart>, JsonRejection>,
) -> Result<Response, AppError> {
    // 404 for an unconfigured deployment, whatever was posted.
    let login = require_passkey_login(&state)?;
    let Json(payload) = payload?;
    let nonce = generate_csrf_token().0;

    let (challenge, ceremony) = match payload.name.parse::<PasskeyAccountName>() {
        Ok(name) => real_or_decoy(login, &name, &nonce).await?,
        Err(_) => decoy(login, &payload.name, &nonce)?,
    };

    state
        .pending_requests()
        .register(pending_login_id(&nonce))
        .await;
    let cookie = login.state().seal(&ceremony, Purpose::Login)?;

    Ok((jar.add(cookie), Json(challenge)).into_response())
}

async fn real_or_decoy(
    login: &PasskeyLogin,
    name: &PasskeyAccountName,
    nonce: &str,
) -> Result<(RequestChallengeResponse, LoginCeremony), AppError> {
    let Some(account) = login.store().find_account_by_name(name).await? else {
        return decoy(login, name.as_str(), nonce);
    };
    let passkeys: Vec<Passkey> = login
        .store()
        .passkeys_for_account(account.id)
        .await?
        .into_iter()
        .map(|stored| stored.passkey)
        .collect();

    match login.webauthn().start_passkey_authentication(&passkeys) {
        Ok((challenge, authentication)) => Ok((
            challenge,
            LoginCeremony::Real {
                nonce: nonce.to_string(),
                account_id: account.id,
                authentication,
            },
        )),
        // An account without passkeys cannot log in; answer like an unknown name.
        Err(err) => {
            warn!(
                "passkey login start for {} fell back to a decoy: {err}",
                account.id
            );
            decoy(login, name.as_str(), nonce)
        }
    }
}

fn decoy(
    login: &PasskeyLogin,
    name: &str,
    nonce: &str,
) -> Result<(RequestChallengeResponse, LoginCeremony), AppError> {
    Ok((
        login.decoy_challenge(name)?,
        LoginCeremony::Decoy {
            nonce: nonce.to_string(),
        },
    ))
}

/// Verifies the assertion against the ceremony sealed in the cookie and the
/// account's current passkeys, then establishes the committee session.
/// Answers `204`; the script navigates on. Every failure is a bare `400`,
/// deliberately not revealing which check failed.
pub async fn finish<S: AppRequestState>(
    _: CsbPasskeyLoginFinishPath,
    State(state): State<S>,
    jar: CookieJar,
    headers: HeaderMap,
    credential: Result<Json<PublicKeyCredential>, JsonRejection>,
) -> Result<Response, AppError> {
    let login = require_passkey_login(&state)?;
    let Json(credential) = credential?;

    let Some((account_id, authentication)) = open_ceremony(&state, login, &jar).await else {
        return Ok(login_failed(jar));
    };
    let Some(user) = verify_assertion(login, &credential, account_id, &authentication).await?
    else {
        return Ok(login_failed(jar));
    };

    let name = match &user {
        CsbUser::Passkey { name, .. } => name.to_string(),
        _ => unreachable!("verify_assertion yields passkey users"),
    };
    let jar = establish_committee_session(&state, user, jar, &headers).await?;

    info!("passkey account {name} logged in to the CSB");
    Ok((
        jar.remove(build_state_removal_cookie()),
        StatusCode::NO_CONTENT,
    )
        .into_response())
}

/// The real ceremony behind the browser's cookie, once its nonce has been
/// consumed (expiry and replay defence). `None` fails closed: no usable
/// cookie, a spent nonce, or a decoy ceremony.
async fn open_ceremony<S: AppRequestState>(
    state: &S,
    login: &PasskeyLogin,
    jar: &CookieJar,
) -> Option<(PasskeyAccountId, PasskeyAuthentication)> {
    let ceremony = login.state().open::<LoginCeremony>(jar, Purpose::Login)?;
    let pending = state
        .pending_requests()
        .consume_if_pending(&pending_login_id(ceremony.nonce()))
        .await;
    match ceremony {
        LoginCeremony::Real {
            account_id,
            authentication,
            ..
        } if pending => Some((account_id, authentication)),
        _ => {
            warn!("passkey login ceremony is spent, expired, or for a name without an account");
            None
        }
    }
}

/// Verifies the assertion and persists the moved signature counter. The
/// ceremony holds the passkeys as they were at start, so a passkey or
/// account revoked since then is refused here. `None` means refused.
async fn verify_assertion(
    login: &PasskeyLogin,
    credential: &PublicKeyCredential,
    account_id: PasskeyAccountId,
    authentication: &PasskeyAuthentication,
) -> Result<Option<CsbUser>, AppError> {
    let result = match login
        .webauthn()
        .finish_passkey_authentication(credential, authentication)
    {
        Ok(result) => result,
        Err(err) => {
            warn!("passkey assertion rejected: {err}");
            return Ok(None);
        }
    };

    let account = login.store().find_account(account_id).await?;
    let stored = login
        .store()
        .passkeys_for_account(account_id)
        .await?
        .into_iter()
        .find(|stored| stored.credential_id() == result.cred_id());
    let (Some(account), Some(mut stored)) = (account, stored) else {
        warn!("passkey login for account {account_id} whose passkey or account was revoked");
        return Ok(None);
    };

    if result.needs_update() {
        stored.passkey.update_credential(&result);
        login
            .store()
            .update_passkey(stored.id, &stored.passkey)
            .await?;
    }
    Ok(Some(CsbUser::Passkey {
        account_id,
        name: account.name,
    }))
}

/// Clears the ceremony cookie; the script shows the generic error.
fn login_failed(jar: CookieJar) -> Response {
    (
        jar.remove(build_state_removal_cookie()),
        StatusCode::BAD_REQUEST,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header;

    use crate::{
        AppState, SessionUser, StoredPasskey,
        auth::passkey::{
            STATE_COOKIE_NAME,
            test_support::{SoftAuthenticator, test_account},
        },
        csb::login::test_support,
        test_utils::response_body_string,
    };

    async fn state_with_account(name: &str) -> (AppState, PasskeyAccountId, SoftAuthenticator) {
        let state = AppState::new_for_tests_with_config(test_support::passkey_test_config()).await;
        let login = state.passkeys.as_ref().expect("passkeys configured");
        let account = test_account(name);
        login.store().create_account(&account).await.unwrap();

        let mut authenticator = SoftAuthenticator::new(1);
        let (ccr, reg_state) = login
            .webauthn()
            .start_passkey_registration(account.id.uuid(), name, name, None)
            .unwrap();
        let passkey = login
            .webauthn()
            .finish_passkey_registration(&authenticator.register(&ccr), &reg_state)
            .unwrap();
        login
            .store()
            .insert_passkey(&StoredPasskey::new(
                account.id,
                "Key".parse().unwrap(),
                passkey,
            ))
            .await
            .unwrap();
        (state, account.id, authenticator)
    }

    /// The ceremony cookie from a start response, as the browser would send
    /// it back on the finish request.
    fn jar_from(response: &Response) -> CookieJar {
        let cookie = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|value| value.starts_with(STATE_COOKIE_NAME))
            .map(|value| value.split(';').next().unwrap().to_string())
            .expect("state cookie");
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, cookie.parse().unwrap());
        CookieJar::from_headers(&headers)
    }

    async fn start_for(state: &AppState, name: &str) -> Response {
        start(
            CsbPasskeyLoginStartPath,
            State(state.clone()),
            CookieJar::new(),
            Ok(Json(PasskeyLoginStart {
                name: name.to_string(),
            })),
        )
        .await
        .expect("start response")
    }

    async fn challenge_from(response: Response) -> RequestChallengeResponse {
        serde_json::from_str(&response_body_string(response).await).expect("challenge json")
    }

    async fn finish_with(
        state: &AppState,
        jar: CookieJar,
        assertion: PublicKeyCredential,
    ) -> Response {
        finish(
            CsbPasskeyLoginFinishPath,
            State(state.clone()),
            jar,
            HeaderMap::new(),
            Ok(Json(assertion)),
        )
        .await
        .expect("finish response")
    }

    /// A fresh ceremony for the account, answered by its authenticator.
    async fn started_ceremony(
        state: &AppState,
        authenticator: &mut SoftAuthenticator,
    ) -> (CookieJar, PublicKeyCredential) {
        let started = start_for(state, "Jan de Vries").await;
        let jar = jar_from(&started);
        let challenge = challenge_from(started).await;
        (
            jar,
            authenticator.authenticate(&challenge).expect("allowed"),
        )
    }

    fn set_cookies(response: &Response) -> Vec<String> {
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect()
    }

    #[tokio::test]
    async fn start_and_finish_are_not_found_without_passkey_config() {
        let state = AppState::new_for_tests().await;

        let err = start(
            CsbPasskeyLoginStartPath,
            State(state.clone()),
            CookieJar::new(),
            Ok(Json(PasskeyLoginStart {
                name: "Jan".to_string(),
            })),
        )
        .await
        .expect_err("404 without config");
        assert!(matches!(err, AppError::GenericNotFound));
    }

    #[tokio::test]
    async fn start_answers_known_and_unknown_names_alike() {
        let (state, _, authenticator) = state_with_account("Jan de Vries").await;

        let known = start_for(&state, "jan DE vries").await;
        let unknown = start_for(&state, "Nobody").await;
        assert_eq!(known.status(), StatusCode::OK);
        assert_eq!(unknown.status(), StatusCode::OK);
        assert!(known.headers().contains_key(header::SET_COOKIE));
        assert!(unknown.headers().contains_key(header::SET_COOKIE));

        let known = challenge_from(known).await;
        let unknown = challenge_from(unknown).await;
        assert_eq!(known.public_key.allow_credentials.len(), 1);
        assert_eq!(
            known.public_key.allow_credentials[0].id,
            authenticator.credential_id()
        );
        assert!(!unknown.public_key.allow_credentials.is_empty());
        assert_ne!(
            unknown.public_key.allow_credentials[0].id,
            authenticator.credential_id()
        );
    }

    #[tokio::test]
    async fn finish_establishes_a_session_for_a_valid_assertion() {
        let (state, account_id, mut authenticator) = state_with_account("Jan de Vries").await;

        let (jar, assertion) = started_ceremony(&state, &mut authenticator).await;
        let response = finish_with(&state, jar, assertion).await;

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let cookies = set_cookies(&response);
        assert!(
            cookies
                .iter()
                .any(|c| c.starts_with(crate::SESSION_COOKIE_NAME))
        );
        assert!(
            cookies
                .iter()
                .any(|c| c.starts_with(&format!("{STATE_COOKIE_NAME}=;"))),
            "ceremony cookie cleared: {cookies:?}"
        );

        // The session belongs to the passkey account.
        let sessions: Vec<_> = match &state.sessions {
            crate::SessionStore::InMemory(inner) => inner.read().values().cloned().collect(),
            #[cfg(feature = "database")]
            crate::SessionStore::Database(_) => unreachable!("tests use the in-memory store"),
        };
        assert_eq!(sessions.len(), 1);
        assert!(matches!(
            &sessions[0].user,
            SessionUser::CentralElectoralCommittee {
                user: CsbUser::Passkey { account_id: id, .. },
                ..
            } if *id == account_id
        ));
    }

    #[tokio::test]
    async fn finish_records_the_login_and_the_moved_counter() {
        let (state, account_id, mut authenticator) = state_with_account("Jan de Vries").await;
        let login = state.passkeys.as_ref().unwrap();
        let before = login
            .store()
            .passkeys_for_account(account_id)
            .await
            .unwrap();

        let (jar, assertion) = started_ceremony(&state, &mut authenticator).await;
        finish_with(&state, jar, assertion).await;

        let main_store = state
            .csb_main_store(state.config.default_election)
            .await
            .unwrap();
        assert!(
            main_store
                .data
                .read()
                .events
                .iter()
                .any(|event| matches!(event.payload.action, crate::CsbMainAction::Login))
        );
        let after = login
            .store()
            .passkeys_for_account(account_id)
            .await
            .unwrap();
        assert_ne!(
            serde_json::to_value(&before[0].passkey).unwrap(),
            serde_json::to_value(&after[0].passkey).unwrap(),
            "the signature counter was persisted"
        );
    }

    #[tokio::test]
    async fn finish_rejects_a_missing_cookie_and_a_decoy_ceremony() {
        let (state, _, mut authenticator) = state_with_account("Jan de Vries").await;

        let (_, assertion) = started_ceremony(&state, &mut authenticator).await;
        let response = finish_with(&state, CookieJar::new(), assertion).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        // A decoy ceremony never logs in, whatever is posted with its cookie.
        let started = start_for(&state, "Nobody").await;
        let decoy_jar = jar_from(&started);
        assert!(
            authenticator
                .authenticate(&challenge_from(started).await)
                .is_none()
        );
        let (_, assertion) = started_ceremony(&state, &mut authenticator).await;
        let response = finish_with(&state, decoy_jar, assertion).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn finish_rejects_a_replayed_ceremony_and_a_revoked_passkey() {
        let (state, account_id, mut authenticator) = state_with_account("Jan de Vries").await;

        let (jar, assertion) = started_ceremony(&state, &mut authenticator).await;
        let first = finish_with(&state, jar.clone(), assertion.clone()).await;
        assert_eq!(first.status(), StatusCode::NO_CONTENT);
        let replay = finish_with(&state, jar, assertion).await;
        assert_eq!(replay.status(), StatusCode::BAD_REQUEST);

        let (jar, assertion) = started_ceremony(&state, &mut authenticator).await;
        state
            .passkeys
            .as_ref()
            .unwrap()
            .store()
            .delete_account(account_id)
            .await
            .unwrap();
        let response = finish_with(&state, jar, assertion).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
