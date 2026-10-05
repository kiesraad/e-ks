use askama::Template;
use axum::{
    extract::{Query, State},
    response::{IntoResponse, Response},
};

use crate::{
    AppError, AppRequestState, Context, CsbContext, CsbMainAction, CsbMainStore, HtmlTemplate,
    Overlay, PasskeyAccount, QueryParamState, StoredPasskey,
    auth::passkey::require_passkey_login,
    csb::passkeys::paths::{CsbDeletePasskeyPath, CsbPasskeysPath},
    filters, redirect_success,
};

#[derive(Template)]
#[template(path = "csb/passkeys/pages/delete.html")]
struct DeletePasskeyTemplate {
    account: PasskeyAccount,
    passkey: StoredPasskey,
    /// Whether this is the account's only passkey, which cannot be revoked
    /// on its own: revoke the account instead.
    last_passkey: bool,
    overlay: Overlay,
    close_action: String,
}

async fn load<S: AppRequestState>(
    state: &S,
    id: crate::PasskeyId,
) -> Result<(PasskeyAccount, StoredPasskey, bool), AppError> {
    let login = require_passkey_login(state)?;
    let passkey = login
        .store()
        .find_passkey(id)
        .await?
        .ok_or(AppError::GenericNotFound)?;
    let account = login
        .store()
        .find_account(passkey.account_id)
        .await?
        .ok_or(AppError::GenericNotFound)?;
    let siblings = login.store().passkeys_for_account(account.id).await?;
    Ok((account, passkey, siblings.len() <= 1))
}

/// Render the revoke confirmation dialog.
pub async fn delete<S: AppRequestState>(
    CsbDeletePasskeyPath { id }: CsbDeletePasskeyPath,
    context: CsbContext,
    State(state): State<S>,
    Query(query): Query<QueryParamState>,
) -> Result<Response, AppError> {
    let (account, passkey, last_passkey) = load(&state, id).await?;
    Ok(HtmlTemplate(
        DeletePasskeyTemplate {
            account,
            passkey,
            last_passkey,
            overlay: Overlay::new_edit(&query),
            close_action: CsbPasskeysPath.to_string(),
        },
        context,
    )
    .into_response())
}

pub async fn delete_submit<S: AppRequestState>(
    CsbDeletePasskeyPath { id }: CsbDeletePasskeyPath,
    context: CsbContext,
    State(state): State<S>,
    main_store: CsbMainStore,
) -> Result<Response, AppError> {
    let (account, passkey, last_passkey) = load(&state, id).await?;
    if last_passkey {
        return Err(AppError::UserError(
            "The only passkey of an account cannot be revoked on its own; revoke the account"
                .to_string(),
        ));
    }

    let login = require_passkey_login(&state)?;
    login.store().delete_passkey(id).await?;
    main_store
        .update(
            CsbMainAction::DeletePasskey {
                account_name: account.name,
                label: passkey.label,
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
        AppState, ElectionConfig,
        auth::passkey::test_support::{test_account, test_passkey},
        csb::login::test_support,
        test_utils::response_body_string,
    };

    async fn state_with_two_passkeys() -> (AppState, PasskeyAccount, StoredPasskey, StoredPasskey) {
        let state = AppState::new_for_tests_with_config(test_support::passkey_test_config()).await;
        let store = state.passkeys.as_ref().unwrap().store();
        let account = test_account("Jan");
        store.create_account(&account).await.unwrap();
        let first = StoredPasskey::new(account.id, "First".parse().unwrap(), test_passkey(1));
        let second = StoredPasskey::new(account.id, "Second".parse().unwrap(), test_passkey(2));
        store.insert_passkey(&first).await.unwrap();
        store.insert_passkey(&second).await.unwrap();
        (state, account, first, second)
    }

    #[tokio::test]
    async fn confirmation_names_the_passkey_and_its_account() {
        let (state, _, first, _) = state_with_two_passkeys().await;

        let response = delete(
            CsbDeletePasskeyPath { id: first.id },
            CsbContext::new_test(),
            State(state),
            Query(QueryParamState::default()),
        )
        .await
        .expect("page");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("First"));
        assert!(body.contains("Jan"));
    }

    #[tokio::test]
    async fn revokes_a_passkey_but_never_the_last_one() {
        let (state, account, first, second) = state_with_two_passkeys().await;
        let main_store = state.csb_main_store(ElectionConfig::EK27).await.unwrap();

        let response = delete_submit(
            CsbDeletePasskeyPath { id: first.id },
            CsbContext::new_test(),
            State(state.clone()),
            main_store.clone(),
        )
        .await
        .expect("deleted");
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let store = state.passkeys.as_ref().unwrap().store();
        assert!(store.find_passkey(first.id).await.unwrap().is_none());
        assert!(main_store.data.read().events.iter().any(|event| matches!(
            &event.payload.action,
            CsbMainAction::DeletePasskey { label, .. } if label.as_str() == "First"
        )));

        let last = delete_submit(
            CsbDeletePasskeyPath { id: second.id },
            CsbContext::new_test(),
            State(state.clone()),
            main_store,
        )
        .await;
        assert!(matches!(last, Err(AppError::UserError(_))));
        assert_eq!(
            store.passkeys_for_account(account.id).await.unwrap().len(),
            1
        );
    }

    #[tokio::test]
    async fn unknown_passkey_is_not_found() {
        let (state, ..) = state_with_two_passkeys().await;

        let err = delete(
            CsbDeletePasskeyPath {
                id: crate::PasskeyId::new(),
            },
            CsbContext::new_test(),
            State(state),
            Query(QueryParamState::default()),
        )
        .await
        .expect_err("404");

        assert!(matches!(err, AppError::GenericNotFound));
    }
}
