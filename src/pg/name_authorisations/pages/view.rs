use super::NameAuthorisationsPath;
use crate::{
    AppError, Context, HtmlTemplate, PgStore, QueryParamState, filters,
    political_groups::PoliticalGroupSteps,
    structs::{
        common::{HasSeverity, PotentialProblems, Problematic},
        list_designation::ListDesignation,
        list_submitters::ListSubmitter,
        name_authorisations::NameAuthorisation,
        political_groups::PoliticalGroup,
    },
};
use askama::Template;
use axum::{extract::Query, response::IntoResponse};

#[derive(Template)]
#[template(path = "pg/name_authorisations/pages/view.html")]
struct NameAuthorisationTemplate {
    name_authorisations: Vec<NameAuthorisation>,
    size_problem: Option<PotentialProblems>,
    steps: PoliticalGroupSteps,
}

pub async fn list_name_authorisations(
    _: NameAuthorisationsPath,
    context: Context,
    store: PgStore,
    Query(query): Query<QueryParamState>,
) -> Result<impl IntoResponse, AppError> {
    let steps = PoliticalGroupSteps::new(&store.snapshot(), query.is_initial())?;
    Ok(HtmlTemplate(
        NameAuthorisationTemplate {
            name_authorisations: steps.name_authorisations.clone(),
            size_problem: NameAuthorisation::get_size_problems(
                steps.list_designation,
                steps.name_authorisations.len(),
            ),
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
        structs::name_authorisations::NameAuthorisationId,
        test_utils::{response_body_string, sample_name_authorisation},
    };
    use axum::{extract::Query, http::StatusCode, response::IntoResponse};

    #[tokio::test]
    async fn list_name_authorisations_shows_created_agent() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        let authorisation = sample_name_authorisation(NameAuthorisationId::new());
        authorisation.create(&store).await?;

        let response = list_name_authorisations(
            NameAuthorisationsPath {},
            Context::new_test_without_db(),
            store.clone(),
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains(&authorisation.legal_name.to_string()));
        assert!(body.contains(authorisation.name.last_name.as_str()));

        Ok(())
    }

    #[tokio::test]
    async fn list_name_authorisations_shows_edit_link() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        let authorisation = sample_name_authorisation(NameAuthorisationId::new());
        authorisation.create(&store).await?;

        let response = list_name_authorisations(
            NameAuthorisationsPath {},
            Context::new_test_without_db(),
            store.clone(),
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains(&authorisation.update_path().to_string()));

        Ok(())
    }
}
