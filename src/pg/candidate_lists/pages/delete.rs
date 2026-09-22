use crate::structs::candidate_lists::CandidateList;
use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, AppResponse, Context, HtmlTemplate, Overlay, PgStore, QueryParamState,
    candidate_lists::pages::CandidateListsDeletePath, filters,
};

#[derive(Template)]
#[template(path = "pg/candidate_lists/pages/delete.html")]
struct DeleteCandidateListTemplate {
    candidate_list: CandidateList,
    overlay: Overlay,
}

pub async fn delete_candidate_list_confirm(
    _: CandidateListsDeletePath,
    context: Context,
    candidate_list: CandidateList,
    Query(query): Query<QueryParamState>,
) -> AppResponse<impl IntoResponse> {
    Ok(HtmlTemplate(
        DeleteCandidateListTemplate {
            overlay: Overlay::new(&query),
            candidate_list,
        },
        context,
    ))
}

pub async fn delete_candidate_list(
    _: CandidateListsDeletePath,
    _context: Context,
    candidate_list: CandidateList,
    store: PgStore,
    Query(query): Query<QueryParamState>,
) -> Result<Response, AppError> {
    candidate_list.delete(&store).await?;

    Ok(query.redirect_or(CandidateList::list_path()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ElectoralDistrict, PgStore, QueryParamState,
        structs::candidate_lists::CandidateListSummary, test_utils::response_body_string,
    };
    use axum::{
        extract::Query,
        http::{StatusCode, header},
    };
    use axum_extra::routing::TypedPath;
    use std::collections::BTreeSet;

    #[tokio::test]
    async fn delete_candidate_list_confirm_contains_delete_button() -> Result<(), AppError> {
        let candidate_list = CandidateList {
            electoral_districts: BTreeSet::from([ElectoralDistrict::Utrecht]),
            ..Default::default()
        };

        let response = delete_candidate_list_confirm(
            CandidateListsDeletePath {
                list_id: candidate_list.id,
            },
            Context::new_test_without_db(),
            candidate_list.clone(),
            Query(QueryParamState::default()),
        )
        .await?
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains(&candidate_list.delete_path().to_string()));

        Ok(())
    }

    #[tokio::test]
    async fn delete_candidate_list_and_redirect() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let context = Context::new_test_without_db();
        let candidate_list = CandidateList {
            electoral_districts: BTreeSet::from([ElectoralDistrict::Utrecht]),
            ..Default::default()
        };
        candidate_list.create(&store).await?;

        let response = delete_candidate_list(
            CandidateListsDeletePath {
                list_id: candidate_list.id,
            },
            context,
            candidate_list.clone(),
            store.clone(),
            Query(QueryParamState::default()),
        )
        .await?;

        // verify redirect
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get(header::LOCATION)
            .expect("location header")
            .to_str()
            .expect("location header value");

        assert_eq!(
            location,
            CandidateList::list_path()
                .with_query_params(QueryParamState::success())
                .to_string()
        );

        // verify deletion (i.e. no lists in database left)
        let lists = CandidateListSummary::list(&store);
        assert_eq!(lists.len(), 0);

        Ok(())
    }
}
