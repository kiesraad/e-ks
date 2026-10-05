use askama::Template;
use axum::{
    extract::{Query, State},
    response::{IntoResponse, Response},
};

use crate::{
    AppError, AppRequestState, Context, CsbContext, CsbMainAction, CsbMainStore, CsbUser,
    HtmlTemplate, Overlay, PasskeyAccount, QueryParamState,
    auth::passkey::require_passkey_login,
    csb::passkeys::paths::{CsbDeletePasskeyAccountPath, CsbPasskeysPath},
    filters, redirect_success,
};

#[derive(Template)]
#[template(path = "csb/passkeys/pages/delete_account.html")]
struct DeletePasskeyAccountTemplate {
    account: PasskeyAccount,
    passkey_count: String,
    /// Whether the session itself was logged in with this account.
    own_account: bool,
    overlay: Overlay,
    close_action: String,
}

async fn load<S: AppRequestState>(
    state: &S,
    id: crate::PasskeyAccountId,
) -> Result<(PasskeyAccount, usize), AppError> {
    let login = require_passkey_login(state)?;
    let account = login
        .store()
        .find_account(id)
        .await?
        .ok_or(AppError::GenericNotFound)?;
    let passkeys = login.store().passkeys_for_account(id).await?;
    Ok((account, passkeys.len()))
}

/// Render the revoke confirmation dialog.
pub async fn delete_account<S: AppRequestState>(
    CsbDeletePasskeyAccountPath { id }: CsbDeletePasskeyAccountPath,
    context: CsbContext,
    State(state): State<S>,
    Query(query): Query<QueryParamState>,
) -> Result<Response, AppError> {
    let (account, passkey_count) = load(&state, id).await?;
    let own_account = matches!(
        context.user()?,
        CsbUser::Passkey { account_id, .. } if account_id == id
    );
    Ok(HtmlTemplate(
        DeletePasskeyAccountTemplate {
            account,
            passkey_count: passkey_count.to_string(),
            own_account,
            overlay: Overlay::new_edit(&query),
            close_action: CsbPasskeysPath.to_string(),
        },
        context,
    )
    .into_response())
}

/// Revokes the account with all its passkeys. Allowed on the session's own
/// account too; that session simply lasts until it expires.
pub async fn delete_account_submit<S: AppRequestState>(
    CsbDeletePasskeyAccountPath { id }: CsbDeletePasskeyAccountPath,
    context: CsbContext,
    State(state): State<S>,
    main_store: CsbMainStore,
) -> Result<Response, AppError> {
    let (account, _) = load(&state, id).await?;

    let login = require_passkey_login(&state)?;
    login.store().delete_account(id).await?;
    main_store
        .update(
            CsbMainAction::DeletePasskeyAccount {
                account_name: account.name,
            }
            .by(context.user()?),
        )
        .await?;

    Ok(redirect_success(CsbPasskeysPath))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    use crate::{
        AppState, ElectionConfig, Locale, Session, StoredPasskey,
        auth::passkey::test_support::{test_account, test_passkey},
        csb::login::test_support,
        test_utils::response_body_string,
    };

    async fn state_with_account() -> (AppState, PasskeyAccount, StoredPasskey) {
        let state = AppState::new_for_tests_with_config(test_support::passkey_test_config()).await;
        let store = state.passkeys.as_ref().unwrap().store();
        let account = test_account("Jan");
        store.create_account(&account).await.unwrap();
        let passkey = StoredPasskey::new(account.id, "Key".parse().unwrap(), test_passkey(1));
        store.insert_passkey(&passkey).await.unwrap();
        (state, account, passkey)
    }

    #[tokio::test]
    async fn confirmation_warns_when_revoking_the_own_account() {
        let (state, account, _) = state_with_account().await;
        let user = CsbUser::Passkey {
            account_id: account.id,
            name: account.name.clone(),
        };
        let context = CsbContext::new(
            Session::for_committee(user, ElectionConfig::EK27, Locale::En),
            ElectionConfig::EK27,
        );

        let response = delete_account(
            CsbDeletePasskeyAccountPath { id: account.id },
            context,
            State(state.clone()),
            Query(QueryParamState::default()),
        )
        .await
        .expect("page");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("signed in with"));

        let response = delete_account(
            CsbDeletePasskeyAccountPath { id: account.id },
            CsbContext::new_test(),
            State(state),
            Query(QueryParamState::default()),
        )
        .await
        .expect("page");
        let body = response_body_string(response).await;
        assert!(!body.contains("signed in with"));
    }

    #[tokio::test]
    async fn revokes_the_account_with_its_passkeys_and_logs_it() {
        let (state, account, passkey) = state_with_account().await;
        let main_store = state.csb_main_store(ElectionConfig::EK27).await.unwrap();

        let response = delete_account_submit(
            CsbDeletePasskeyAccountPath { id: account.id },
            CsbContext::new_test(),
            State(state.clone()),
            main_store.clone(),
        )
        .await
        .expect("deleted");
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let store = state.passkeys.as_ref().unwrap().store();
        assert!(store.find_account(account.id).await.unwrap().is_none());
        assert!(store.find_passkey(passkey.id).await.unwrap().is_none());
        assert!(main_store.data.read().events.iter().any(|event| matches!(
            &event.payload.action,
            CsbMainAction::DeletePasskeyAccount { account_name } if account_name.as_str() == "Jan"
        )));

        // Already gone is a stale form, not a change.
        let again = delete_account_submit(
            CsbDeletePasskeyAccountPath { id: account.id },
            CsbContext::new_test(),
            State(state),
            main_store,
        )
        .await;
        assert!(matches!(again, Err(AppError::GenericNotFound)));
    }
}
