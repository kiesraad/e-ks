use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, AppResponse, Context, HtmlTemplate, Overlay, PgStore, QueryParamState, filters,
    structs::{
        common::{HasSeverity, Problematic},
        list_submitters::ListSubmitter,
    },
};

use super::SubstituteSubmitterDeletePath;

#[derive(Template)]
#[template(path = "pg/substitute_list_submitters/pages/delete.html")]
struct DeleteSubstituteSubmitterTemplate {
    substitute_submitter: ListSubmitter,
    overlay: Overlay,
}

pub async fn delete_substitute_submitter_confirm(
    _: SubstituteSubmitterDeletePath,
    context: Context,
    substitute_submitter: ListSubmitter,
    Query(query): Query<QueryParamState>,
) -> AppResponse<impl IntoResponse> {
    Ok(HtmlTemplate(
        DeleteSubstituteSubmitterTemplate {
            substitute_submitter,
            overlay: Overlay::new(&query),
        },
        context,
    ))
}

pub async fn delete_substitute_submitter(
    _: SubstituteSubmitterDeletePath,
    _context: Context,
    substitute_submitter: ListSubmitter,
    store: PgStore,
    Query(query): Query<QueryParamState>,
) -> Result<Response, AppError> {
    substitute_submitter.delete_substitute(&store).await?;

    Ok(query.redirect_or_preserving_initial(ListSubmitter::view_path()))
}

#[cfg(test)]
mod tests {
    use axum::extract::Query;
    use axum_extra::routing::TypedPath;

    use super::*;
    use crate::QueryParamState;

    use crate::{
        AppError, Context, PgStore,
        structs::list_submitters::ListSubmitterId,
        test_utils::{response_body_string, sample_list_submitter},
    };

    #[tokio::test]
    async fn delete_substitute_submitter_confirm_contains_delete_button() -> Result<(), AppError> {
        let sub_submitter_id = ListSubmitterId::new();
        let substitute_submitter = sample_list_submitter(sub_submitter_id);

        let response = delete_substitute_submitter_confirm(
            SubstituteSubmitterDeletePath { sub_submitter_id },
            Context::new_test_without_db(),
            substitute_submitter.clone(),
            Query(QueryParamState::default()),
        )
        .await?
        .into_response();

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains(&substitute_submitter.substitute_delete_path().to_string()));

        Ok(())
    }

    #[tokio::test]
    async fn delete_substitute_submitter_removes_and_redirects() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        let sub_submitter_id = ListSubmitterId::new();
        let substitute_submitter = sample_list_submitter(sub_submitter_id);
        substitute_submitter.create_substitute(&store).await?;

        let context = Context::new_test_without_db();

        let response = delete_substitute_submitter(
            SubstituteSubmitterDeletePath { sub_submitter_id },
            context,
            substitute_submitter.clone(),
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
            ListSubmitter::view_path()
                .with_query_params(QueryParamState::success())
                .to_string()
        );

        let submitters = store.get_substitute_submitters();
        assert!(submitters.is_empty());

        Ok(())
    }
}
