use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, AppResponse, Context, HtmlTemplate, Overlay, PgStore, QueryParamState, filters,
    structs::{
        common::{HasSeverity, Problematic},
        name_authorisations::NameAuthorisation,
    },
};

use super::NameAuthorisationDeletePath;

#[derive(Template)]
#[template(path = "pg/name_authorisations/pages/delete.html")]
struct DeleteNameAuthorisationTemplate {
    name_authorisation: NameAuthorisation,
    overlay: Overlay,
}

pub async fn delete_name_authorisation_confirm(
    _: NameAuthorisationDeletePath,
    context: Context,
    name_authorisation: NameAuthorisation,
    Query(query): Query<QueryParamState>,
) -> AppResponse<impl IntoResponse> {
    Ok(HtmlTemplate(
        DeleteNameAuthorisationTemplate {
            name_authorisation,
            overlay: Overlay::new_edit(&query),
        },
        context,
    ))
}

pub async fn delete_name_authorisation(
    _: NameAuthorisationDeletePath,
    name_authorisation: NameAuthorisation,
    _context: Context,
    store: PgStore,
    Query(query): Query<QueryParamState>,
) -> Result<Response, AppError> {
    name_authorisation.delete(&store).await?;

    Ok(query.redirect_or_preserving_initial(NameAuthorisation::list_path()))
}

#[cfg(test)]
mod tests {
    use axum::extract::Query;
    use axum_extra::routing::TypedPath;

    use super::*;
    use crate::{
        AppError, Context, PgStore, QueryParamState,
        structs::name_authorisations::NameAuthorisationId,
        test_utils::{response_body_string, sample_name_authorisation},
    };

    #[tokio::test]
    async fn delete_name_authorisation_confirm_contains_delete_button() -> Result<(), AppError> {
        let authorisation_id = NameAuthorisationId::new();
        let name_authorisation = sample_name_authorisation(authorisation_id);

        let response = delete_name_authorisation_confirm(
            NameAuthorisationDeletePath { authorisation_id },
            Context::new_test_without_db(),
            name_authorisation.clone(),
            Query(QueryParamState::default()),
        )
        .await?
        .into_response();

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains(&name_authorisation.delete_path().to_string()));

        Ok(())
    }

    #[tokio::test]
    async fn delete_name_authorisation_removes_and_redirects() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let authorisation_id = NameAuthorisationId::new();
        let name_authorisation = sample_name_authorisation(authorisation_id);

        name_authorisation.create(&store).await?;

        let context = Context::new_test_without_db();

        let response = delete_name_authorisation(
            NameAuthorisationDeletePath { authorisation_id },
            name_authorisation,
            context,
            store.clone(),
            Query(QueryParamState::default()),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get(axum::http::header::LOCATION)
            .expect("location header")
            .to_str()
            .expect("location header value");
        assert_eq!(
            location,
            NameAuthorisation::list_path()
                .with_query_params(QueryParamState::success())
                .to_string()
        );

        let name_authorisations = store
            .snapshot()
            .name_authorisations()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        assert!(name_authorisations.is_empty());

        Ok(())
    }
}
