use crate::structs::{list_designation::ListDesignation, political_groups::PoliticalGroup};
use askama::Template;
use axum::{extract::Query, response::IntoResponse};

use crate::{
    AppError, Context, HtmlTemplate, PgStore, QueryParamState, filters,
    political_groups::PoliticalGroupSteps,
    structs::{
        common::{HasSeverity, Problematic},
        list_submitters::ListSubmitter,
        name_authorisations::NameAuthorisation,
    },
};

use super::ListSubmitterViewPath;

#[derive(Template)]
#[template(path = "pg/list_submitters/pages/view.html")]
struct ListSubmitterViewTemplate {
    list_submitter: ListSubmitter,
    substitute_submitters: Vec<ListSubmitter>,
    steps: PoliticalGroupSteps,
}

pub async fn view_list_submitter(
    _: ListSubmitterViewPath,
    context: Context,
    store: PgStore,
    Query(query): Query<QueryParamState>,
) -> Result<impl IntoResponse, AppError> {
    let steps = PoliticalGroupSteps::new(&store, query.is_initial())?;
    let list_submitter = steps.list_submitter.clone();
    let substitute_submitters = steps.substitute_submitters.clone();
    Ok(HtmlTemplate(
        ListSubmitterViewTemplate {
            list_submitter,
            substitute_submitters,
            steps,
        },
        context,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppError, Context, PgStore, QueryParamState,
        structs::list_submitters::ListSubmitterId,
        test_utils::{response_body_string, sample_list_submitter},
    };
    use axum::{extract::Query, http::StatusCode, response::IntoResponse};

    #[tokio::test]
    async fn view_list_submitter_shows_current_submitter() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        let submitter = sample_list_submitter(ListSubmitterId::new());
        submitter.update(&store).await?;

        let response = view_list_submitter(
            ListSubmitterViewPath {},
            Context::new_test_without_db(),
            store,
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains(submitter.name.last_name.as_str()));

        Ok(())
    }

    #[tokio::test]
    async fn view_list_submitter_shows_edit_link() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        let submitter = sample_list_submitter(ListSubmitterId::new());
        submitter.update(&store).await?;

        let response = view_list_submitter(
            ListSubmitterViewPath {},
            Context::new_test_without_db(),
            store,
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains(&ListSubmitter::update_path().to_string()));

        Ok(())
    }

    /// Opening an overlay from this page must not lose `initial=true`: without
    /// it, closing the overlay lands back here with warnings turned on for
    /// steps the user has not reached yet.
    #[tokio::test]
    async fn view_list_submitter_keeps_initial_in_overlay_links() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        let submitter = sample_list_submitter(ListSubmitterId::new());
        submitter.update(&store).await?;
        submitter.create_substitute(&store).await?;

        let response = view_list_submitter(
            ListSubmitterViewPath {},
            Context::new_test_without_db(),
            store.clone(),
            Query(QueryParamState::initial()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;

        for path in [
            ListSubmitter::update_path().to_string(),
            ListSubmitter::substitute_create_path().to_string(),
            submitter.substitute_update_path().to_string(),
        ] {
            // Askama escapes the `&` that `with_query_params` emits.
            let expected = format!("\"{path}?&#38;initial=true\"");
            assert!(body.contains(&expected), "{path} must keep initial=true");
        }

        Ok(())
    }

    #[tokio::test]
    async fn view_list_submitter_hides_add_button_when_submitter_exists() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        let submitter = sample_list_submitter(ListSubmitterId::new());
        submitter.update(&store).await?;

        let response = view_list_submitter(
            ListSubmitterViewPath {},
            Context::new_test_without_db(),
            store,
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(!body.contains("Add list submitter"));

        Ok(())
    }
}
