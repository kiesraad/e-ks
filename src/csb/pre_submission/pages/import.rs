use askama::Template;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, AppRequestState, Context, CsbContext, Form, HtmlTemplate,
    csb::{
        import::{ImportForm, ImportResult, import_package},
        pre_submission::pages::{CsbPreSubmissionGroupPath, CsbPreSubmissionImportPath},
    },
    filters, redirect_success,
};

#[derive(Template)]
#[template(path = "csb/pre_submission/pages/import.html")]
struct PreSubmissionImportTemplate {
    hash: String,
    error: Option<String>,
    warning: Option<String>,
}

fn render_import(
    context: CsbContext,
    hash: String,
    error: Option<String>,
    warning: Option<String>,
) -> Response {
    HtmlTemplate(
        PreSubmissionImportTemplate {
            hash,
            error,
            warning,
        },
        context,
    )
    .into_response()
}

pub async fn import(
    _: CsbPreSubmissionImportPath,
    context: CsbContext,
) -> Result<Response, AppError> {
    Ok(render_import(context, String::new(), None, None))
}

/// Import the package identified by the submitted chain hash for the
/// pre-submission check.
pub async fn import_submit<S: AppRequestState>(
    _: CsbPreSubmissionImportPath,
    State(state): State<S>,
    context: CsbContext,
    Form(form): Form<ImportForm>,
) -> Result<Response, AppError> {
    let hash = form.hash.clone();
    let result = import_package(
        &state,
        state.pre_submission_store_registry(),
        form,
        context.user()?,
        context.election,
        context.session.locale,
    )
    .await?;

    Ok(match result {
        ImportResult::Imported(stream_id) => {
            redirect_success(CsbPreSubmissionGroupPath { stream_id })
        }
        ImportResult::Retry { error, warning } => render_import(context, hash, error, warning),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    use crate::{
        AppState, CsbAction, ElectionConfig, PgEvent, StreamId, test_utils::response_body_string,
        utils::format_hash,
    };

    /// A political-group stream with one event; returns its formatted hash.
    async fn seed_source_event(state: &AppState) -> Result<String, AppError> {
        let source_store = state
            .store_for_stream(StreamId::new(), ElectionConfig::EK27, false)
            .await?;
        source_store.update(PgEvent::HideDownloadWarning).await?;

        let hash = source_store.data.read().events[0].hash;
        Ok(format_hash(&hash, false))
    }

    async fn submit(
        state: &AppState,
        hash: &str,
        confirmed_hash: Option<&str>,
    ) -> Result<Response, AppError> {
        import_submit(
            CsbPreSubmissionImportPath,
            State(state.clone()),
            CsbContext::new_test(),
            Form(ImportForm {
                hash: hash.to_string(),
                confirmed_hash: confirmed_hash.map(str::to_string),
            }),
        )
        .await
    }

    #[tokio::test]
    async fn renders_the_import_form() {
        let response = import(CsbPreSubmissionImportPath, CsbContext::new_test())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(
            body.contains("action=\"/csb/pre-submission/import\""),
            "{body}"
        );
        // No empty groups here: the check is about a handed-in package.
        assert!(!body.contains("create-empty"));
    }

    #[tokio::test]
    async fn imports_into_the_pre_submission_registry_only() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let hash = seed_source_event(&state).await?;

        let response = submit(&state, &hash, None).await?;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response.headers()["Location"].to_str().unwrap();
        assert!(location.starts_with("/csb/pre-submission/"), "{location}");

        let stores = state
            .pre_submission_store_registry()
            .stores_by_scope()
            .await?;
        assert_eq!(stores.len(), 1);
        assert!(matches!(
            stores[0].data.read().events[0].payload.action,
            CsbAction::Import { .. }
        ));
        assert!(
            state
                .csb_store_registry()
                .stores_by_scope()
                .await?
                .is_empty()
        );

        Ok(())
    }

    #[tokio::test]
    async fn an_unknown_hash_re_renders_the_form_with_an_error() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;

        let response = submit(&state, "F381 3DE7 96D3 8033", None).await?;

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("<span class=\"error\">"), "{body}");
        assert!(
            state
                .pre_submission_store_registry()
                .stores_by_scope()
                .await?
                .is_empty()
        );

        Ok(())
    }

    #[tokio::test]
    async fn a_second_import_warns_until_confirmed() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let hash = seed_source_event(&state).await?;

        assert_eq!(
            submit(&state, &hash, None).await?.status(),
            StatusCode::SEE_OTHER
        );

        let response = submit(&state, &hash, None).await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response_body_string(response)
                .await
                .contains("alert-warning")
        );
        assert_eq!(
            state
                .pre_submission_store_registry()
                .stores_by_scope()
                .await?
                .len(),
            1
        );

        let response = submit(&state, &hash, Some(&hash)).await?;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            state
                .pre_submission_store_registry()
                .stores_by_scope()
                .await?
                .len(),
            2
        );

        Ok(())
    }
}
