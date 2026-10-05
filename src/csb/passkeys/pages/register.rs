//! `POST /csb/passkeys/register/start` and `/finish`: the registration
//! ceremony, driven by the management page's script. Both need a committee
//! session and, as JSON posts, carry the CSRF token in a header.

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use axum_extra::extract::CookieJar;
use serde::Deserialize;
use tracing::warn;
use webauthn_rs::prelude::{CredentialID, Passkey, RegisterPublicKeyCredential};

use crate::{
    AppError, AppRequestState, CsbContext, CsbMainAction, CsbMainStore, CsbUser, PasskeyAccount,
    PasskeyAccountId, PasskeyAccountName, PasskeyLabel, PasskeyLogin, StoredPasskey,
    auth::passkey::{
        MAX_PASSKEYS_PER_ACCOUNT, Purpose, RegisterCeremony, RegisterTarget,
        build_state_removal_cookie, pending_register_id, require_passkey_login, user_error,
    },
    csb::passkeys::paths::{CsbPasskeyRegisterFinishPath, CsbPasskeyRegisterStartPath},
    form::{ValidationError, generate_csrf_token},
};

#[derive(Debug, Deserialize)]
pub struct RegisterStart {
    /// The new account's name; ignored for a passkey session, which adds to
    /// its own account.
    #[serde(default)]
    name: Option<String>,
    label: String,
}

/// Decides which account the passkey goes to, then returns the WebAuthn
/// creation options and seals the ceremony into the browser's cookie. A new
/// account is persisted only when the ceremony finishes.
pub async fn register_start<S: AppRequestState>(
    _: CsbPasskeyRegisterStartPath,
    context: CsbContext,
    State(state): State<S>,
    jar: CookieJar,
    payload: Result<Json<RegisterStart>, JsonRejection>,
) -> Result<Response, AppError> {
    // 404 for an unconfigured deployment, whatever was posted.
    let login = require_passkey_login(&state)?;
    let Json(payload) = payload?;
    let user = context.user()?;
    let label: PasskeyLabel = payload.label.parse().map_err(invalid)?;

    let (target, name, existing) = resolve_target(login, &user, payload.name.as_deref()).await?;
    let exclude: Vec<CredentialID> = existing
        .iter()
        .map(|stored| stored.credential_id().clone())
        .collect();
    let (challenge, registration) = login
        .webauthn()
        .start_passkey_registration(
            target.account_id().uuid(),
            name.as_str(),
            name.as_str(),
            Some(exclude),
        )
        .map_err(user_error)?;

    let nonce = generate_csrf_token().0;
    state
        .pending_requests()
        .register(pending_register_id(&nonce))
        .await;
    let ceremony = RegisterCeremony {
        nonce,
        label,
        created_by: user,
        target,
        registration,
    };
    let cookie = login.state().seal(&ceremony, Purpose::Register)?;

    Ok((jar.add(cookie), Json(challenge)).into_response())
}

/// A passkey session adds to its own account, up to the limit; any other
/// session names a new account, which must be free.
async fn resolve_target(
    login: &PasskeyLogin,
    user: &CsbUser,
    requested_name: Option<&str>,
) -> Result<(RegisterTarget, PasskeyAccountName, Vec<StoredPasskey>), AppError> {
    if let CsbUser::Passkey { account_id, name } = user {
        let existing = login.store().passkeys_for_account(*account_id).await?;
        if existing.len() >= MAX_PASSKEYS_PER_ACCOUNT {
            return Err(AppError::UserError(format!(
                "An account holds at most {MAX_PASSKEYS_PER_ACCOUNT} passkeys"
            )));
        }
        return Ok((
            RegisterTarget::Existing(*account_id),
            name.clone(),
            existing,
        ));
    }

    let name: PasskeyAccountName = requested_name
        .unwrap_or_default()
        .parse()
        .map_err(invalid)?;
    if login.store().find_account_by_name(&name).await?.is_some() {
        return Err(AppError::Conflict);
    }
    let target = RegisterTarget::New {
        id: PasskeyAccountId::new(),
        name: name.clone(),
    };
    Ok((target, name, Vec::new()))
}

/// Verifies the attestation against the sealed ceremony, stores the passkey
/// (creating the account first when it is new) and records it in the audit
/// log. Answers `204`; the script reloads the page.
pub async fn register_finish<S: AppRequestState>(
    _: CsbPasskeyRegisterFinishPath,
    context: CsbContext,
    State(state): State<S>,
    main_store: CsbMainStore,
    jar: CookieJar,
    credential: Result<Json<RegisterPublicKeyCredential>, JsonRejection>,
) -> Result<Response, AppError> {
    let login = require_passkey_login(&state)?;
    let Json(credential) = credential?;
    let user = context.user()?;

    let ceremony = open_ceremony(&state, login, &jar, &user).await?;
    let passkey = login
        .webauthn()
        .finish_passkey_registration(&credential, &ceremony.registration)
        .map_err(user_error)?;
    let account_name = store_passkey(login, &user, &ceremony, passkey).await?;

    main_store
        .update(
            CsbMainAction::RegisterPasskey {
                account_name,
                label: ceremony.label,
            }
            .by(user),
        )
        .await?;

    Ok((
        jar.remove(build_state_removal_cookie()),
        StatusCode::NO_CONTENT,
    )
        .into_response())
}

/// The ceremony behind the browser's cookie, once its nonce has been
/// consumed; it must have been started by the same session user.
async fn open_ceremony<S: AppRequestState>(
    state: &S,
    login: &PasskeyLogin,
    jar: &CookieJar,
    user: &CsbUser,
) -> Result<RegisterCeremony, AppError> {
    let Some(ceremony) = login
        .state()
        .open::<RegisterCeremony>(jar, Purpose::Register)
    else {
        return Err(stale());
    };
    if !state
        .pending_requests()
        .consume_if_pending(&pending_register_id(&ceremony.nonce))
        .await
    {
        warn!("passkey registration ceremony is unknown, expired, or replayed");
        return Err(stale());
    }
    if ceremony.created_by != *user {
        warn!("passkey registration finished by another session user than it was started by");
        return Err(stale());
    }
    Ok(ceremony)
}

/// Stores the verified passkey under the ceremony's account, creating a new
/// account first and removing it again should the passkey not store, so no
/// empty account is left behind. Returns the account's name for the log.
async fn store_passkey(
    login: &PasskeyLogin,
    user: &CsbUser,
    ceremony: &RegisterCeremony,
    passkey: Passkey,
) -> Result<PasskeyAccountName, AppError> {
    let account_name = match &ceremony.target {
        RegisterTarget::New { id, name } => {
            let account = PasskeyAccount::new(*id, name.clone(), user.clone());
            login.store().create_account(&account).await?;
            name.clone()
        }
        RegisterTarget::Existing(id) => {
            login
                .store()
                .find_account(*id)
                .await?
                .ok_or(AppError::GenericNotFound)?
                .name
        }
    };

    let stored = StoredPasskey::new(
        ceremony.target.account_id(),
        ceremony.label.clone(),
        passkey,
    );
    if let Err(err) = login.store().insert_passkey(&stored).await {
        if let RegisterTarget::New { id, .. } = ceremony.target {
            login.store().delete_account(id).await?;
        }
        return Err(err);
    }
    Ok(account_name)
}

fn invalid(err: ValidationError) -> AppError {
    AppError::UserError(err.to_string())
}

fn stale() -> AppError {
    AppError::UserError("The passkey registration expired, start again".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, header};
    use webauthn_rs::prelude::CreationChallengeResponse;

    use crate::{
        AppState, ElectionConfig, Locale, Session,
        auth::passkey::{
            STATE_COOKIE_NAME,
            test_support::{SoftAuthenticator, test_account, test_passkey},
        },
        csb::login::test_support,
        test_utils::response_body_string,
    };

    async fn passkey_state() -> AppState {
        AppState::new_for_tests_with_config(test_support::passkey_test_config()).await
    }

    fn passkey_context(account: &PasskeyAccount) -> CsbContext {
        let user = CsbUser::Passkey {
            account_id: account.id,
            name: account.name.clone(),
        };
        CsbContext::new(
            Session::for_committee(user, ElectionConfig::EK27, Locale::En),
            ElectionConfig::EK27,
        )
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

    async fn start(
        state: &AppState,
        context: CsbContext,
        name: Option<&str>,
        label: &str,
    ) -> Result<Response, AppError> {
        register_start(
            CsbPasskeyRegisterStartPath,
            context,
            State(state.clone()),
            CookieJar::new(),
            Ok(Json(RegisterStart {
                name: name.map(str::to_string),
                label: label.to_string(),
            })),
        )
        .await
    }

    async fn challenge_from(response: Response) -> CreationChallengeResponse {
        serde_json::from_str(&response_body_string(response).await).expect("challenge json")
    }

    async fn finish(
        state: &AppState,
        context: CsbContext,
        jar: CookieJar,
        credential: RegisterPublicKeyCredential,
    ) -> Result<Response, AppError> {
        let main_store = state.csb_main_store(ElectionConfig::EK27).await.unwrap();
        register_finish(
            CsbPasskeyRegisterFinishPath,
            context,
            State(state.clone()),
            main_store,
            jar,
            Ok(Json(credential)),
        )
        .await
    }

    /// A started ceremony for a new account, answered by the authenticator.
    async fn started_ceremony(
        state: &AppState,
        authenticator: &mut SoftAuthenticator,
        name: &str,
        label: &str,
    ) -> (CookieJar, RegisterPublicKeyCredential) {
        let started = start(state, CsbContext::new_test(), Some(name), label)
            .await
            .unwrap();
        let jar = jar_from(&started);
        let credential = authenticator.register(&challenge_from(started).await);
        (jar, credential)
    }

    #[tokio::test]
    async fn start_validates_name_and_label_and_refuses_taken_names() {
        let state = passkey_state().await;
        let store = state.passkeys.as_ref().unwrap().store();
        store.create_account(&test_account("Taken")).await.unwrap();

        let no_name = start(&state, CsbContext::new_test(), None, "Key").await;
        assert!(matches!(no_name, Err(AppError::UserError(_))));

        let bad_label = start(&state, CsbContext::new_test(), Some("Jan"), " ").await;
        assert!(matches!(bad_label, Err(AppError::UserError(_))));

        let taken = start(&state, CsbContext::new_test(), Some("taken"), "Key").await;
        assert!(matches!(taken, Err(AppError::Conflict)));

        let ok = start(&state, CsbContext::new_test(), Some("Jan"), "Key")
            .await
            .expect("started");
        assert_eq!(ok.status(), StatusCode::OK);
        assert!(ok.headers().contains_key(header::SET_COOKIE));
        let challenge = challenge_from(ok).await;
        assert_eq!(challenge.public_key.user.name, "Jan");
        // Nothing is persisted until the ceremony finishes.
        assert_eq!(store.list_accounts().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_passkey_session_adds_to_its_own_account_up_to_the_limit() {
        let state = passkey_state().await;
        let store = state.passkeys.as_ref().unwrap().store();
        let account = test_account("Jan");
        store.create_account(&account).await.unwrap();

        let response = start(&state, passkey_context(&account), Some("Other"), "Key")
            .await
            .expect("started for own account, the name is ignored");
        let challenge = challenge_from(response).await;
        assert_eq!(challenge.public_key.user.name, "Jan");
        assert_eq!(
            challenge.public_key.user.id,
            account.id.uuid().as_bytes().to_vec()
        );

        for seed in 1..=MAX_PASSKEYS_PER_ACCOUNT as u8 {
            let passkey =
                StoredPasskey::new(account.id, "Key".parse().unwrap(), test_passkey(seed));
            store.insert_passkey(&passkey).await.unwrap();
        }
        let full = start(&state, passkey_context(&account), None, "One more").await;
        assert!(matches!(full, Err(AppError::UserError(_))));
    }

    #[tokio::test]
    async fn finish_creates_the_account_stores_the_passkey_and_logs_it() {
        let state = passkey_state().await;
        let mut authenticator = SoftAuthenticator::new(3);
        let (jar, credential) =
            started_ceremony(&state, &mut authenticator, "Jan de Vries", "YubiKey").await;

        let response = finish(&state, CsbContext::new_test(), jar, credential)
            .await
            .expect("finished");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let store = state.passkeys.as_ref().unwrap().store();
        let accounts = store.list_accounts().await.unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].0.name.as_str(), "Jan de Vries");
        assert_eq!(accounts[0].0.created_by, CsbUser::Developer);
        assert_eq!(accounts[0].1.len(), 1);
        assert_eq!(accounts[0].1[0].label.as_str(), "YubiKey");
        assert_eq!(
            accounts[0].1[0].credential_id().as_slice(),
            authenticator.credential_id()
        );
        let main_store = state.csb_main_store(ElectionConfig::EK27).await.unwrap();
        assert!(main_store.data.read().events.iter().any(|event| matches!(
            &event.payload.action,
            CsbMainAction::RegisterPasskey { account_name, label }
                if account_name.as_str() == "Jan de Vries" && label.as_str() == "YubiKey"
        )));
    }

    #[tokio::test]
    async fn finish_refuses_a_missing_cookie_a_replay_and_another_user() {
        let state = passkey_state().await;
        let mut authenticator = SoftAuthenticator::new(4);

        let (jar, credential) = started_ceremony(&state, &mut authenticator, "Jan", "Key").await;
        let no_cookie = finish(
            &state,
            CsbContext::new_test(),
            CookieJar::new(),
            credential.clone(),
        )
        .await;
        assert!(matches!(no_cookie, Err(AppError::UserError(_))));

        let first = finish(
            &state,
            CsbContext::new_test(),
            jar.clone(),
            credential.clone(),
        )
        .await
        .expect("finished");
        assert_eq!(first.status(), StatusCode::NO_CONTENT);
        let replay = finish(&state, CsbContext::new_test(), jar, credential).await;
        assert!(matches!(replay, Err(AppError::UserError(_))));

        // A ceremony started by one session user cannot be finished by another.
        let mut other_authenticator = SoftAuthenticator::new(5);
        let (jar, credential) =
            started_ceremony(&state, &mut other_authenticator, "Piet", "Key").await;
        let other_user = finish(
            &state,
            passkey_context(&test_account("Someone")),
            jar,
            credential,
        )
        .await;
        assert!(matches!(other_user, Err(AppError::UserError(_))));
        let store = state.passkeys.as_ref().unwrap().store();
        assert!(
            store
                .find_account_by_name(&"Piet".parse().unwrap())
                .await
                .unwrap()
                .is_none()
        );
    }
}
