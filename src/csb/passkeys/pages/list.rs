use askama::Template;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, AppRequestState, Context, CsbContext, CsbUser, HtmlTemplate, PasskeyAccount,
    StoredPasskey,
    auth::passkey::{MAX_PASSKEYS_PER_ACCOUNT, require_passkey_login},
    csb::passkeys::paths::{
        CsbPasskeyRegisterFinishPath, CsbPasskeyRegisterStartPath, CsbPasskeysPath,
    },
    filters,
};

#[derive(Template)]
#[template(path = "csb/passkeys/pages/list.html")]
struct PasskeysTemplate {
    /// Every account with its passkeys, by name.
    accounts: Vec<(PasskeyAccount, Vec<StoredPasskey>)>,
    /// The account behind the session when it is a passkey login: new
    /// passkeys are added to it, and it gets no name field.
    own_account: Option<PasskeyAccount>,
    /// Whether the session's own account is full.
    own_account_full: bool,
    register_start_path: CsbPasskeyRegisterStartPath,
    register_finish_path: CsbPasskeyRegisterFinishPath,
}

/// The registered passkeys of every committee member, and the form to add one.
pub async fn list<S: AppRequestState>(
    _: CsbPasskeysPath,
    context: CsbContext,
    State(state): State<S>,
) -> Result<Response, AppError> {
    let login = require_passkey_login(&state)?;
    let accounts = login.store().list_accounts().await?;

    let (own_account, own_account_full) = match context.user()? {
        CsbUser::Passkey { account_id, .. } => accounts
            .iter()
            .find(|(account, _)| account.id == account_id)
            .map(|(account, passkeys)| {
                (
                    Some(account.clone()),
                    passkeys.len() >= MAX_PASSKEYS_PER_ACCOUNT,
                )
            })
            .unwrap_or((None, false)),
        _ => (None, false),
    };

    Ok(HtmlTemplate(
        PasskeysTemplate {
            accounts,
            own_account,
            own_account_full,
            register_start_path: CsbPasskeyRegisterStartPath,
            register_finish_path: CsbPasskeyRegisterFinishPath,
        },
        context,
    )
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    use crate::{
        AppState, ElectionConfig, Locale, Session,
        auth::passkey::test_support::{test_account, test_passkey},
        csb::login::test_support,
        test_utils::response_body_string,
    };

    async fn passkey_state() -> AppState {
        AppState::new_for_tests_with_config(test_support::passkey_test_config()).await
    }

    #[tokio::test]
    async fn is_not_found_without_passkey_config() {
        let state = AppState::new_for_tests().await;

        let err = list(CsbPasskeysPath, CsbContext::new_test(), State(state))
            .await
            .expect_err("404 without config");

        assert!(matches!(err, AppError::GenericNotFound));
    }

    #[tokio::test]
    async fn empty_list_shows_placeholder_and_a_form_with_a_name_field() {
        let state = passkey_state().await;

        let response = list(CsbPasskeysPath, CsbContext::new_test(), State(state))
            .await
            .expect("page");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("No passkeys have been registered yet."));
        assert!(body.contains("data-passkey-register-start=\"/csb/passkeys/register/start\""));
        assert!(body.contains("data-passkey-register-finish=\"/csb/passkeys/register/finish\""));
        assert!(body.contains("name=\"name\""));
        assert!(body.contains("name=\"label\""));
    }

    #[tokio::test]
    async fn lists_accounts_with_their_passkeys_and_delete_links() {
        let state = passkey_state().await;
        let store = state.passkeys.as_ref().unwrap().store();
        let account = test_account("Jan de Vries");
        store.create_account(&account).await.unwrap();
        let passkey = StoredPasskey::new(account.id, "YubiKey".parse().unwrap(), test_passkey(1));
        store.insert_passkey(&passkey).await.unwrap();

        let response = list(CsbPasskeysPath, CsbContext::new_test(), State(state))
            .await
            .expect("page");

        let body = response_body_string(response).await;
        assert!(body.contains("Jan de Vries"));
        assert!(body.contains("YubiKey"));
        assert!(body.contains(&format!("/csb/passkeys/{}/delete", passkey.id)));
        assert!(body.contains(&format!("/csb/passkeys/accounts/{}/delete", account.id)));
        assert!(body.contains("Developer"), "who registered the account");
    }

    #[tokio::test]
    async fn a_passkey_session_adds_to_its_own_account_without_a_name_field() {
        let state = passkey_state().await;
        let store = state.passkeys.as_ref().unwrap().store();
        let account = test_account("Jan de Vries");
        store.create_account(&account).await.unwrap();
        let user = CsbUser::Passkey {
            account_id: account.id,
            name: account.name.clone(),
        };
        let session = Session::for_committee(user, ElectionConfig::EK27, Locale::En);
        let context = CsbContext::new(session, ElectionConfig::EK27);

        let response = list(CsbPasskeysPath, context, State(state))
            .await
            .expect("page");

        let body = response_body_string(response).await;
        assert!(!body.contains("name=\"name\""));
        assert!(body.contains("added to your account"));
    }
}
