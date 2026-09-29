use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, AppResponse, Context, HtmlTemplate, Overlay, PgStore, QueryParamState, filters,
    structs::{
        candidate_lists::{CandidateList, FullCandidateList},
        candidates::Candidate,
        common::{HasSeverity, Problematic},
    },
};

use super::CandidateListDeletePersonPath;

#[derive(Template)]
#[template(path = "pg/candidates/pages/delete.html")]
struct DeleteCandidateTemplate {
    candidate: Candidate,
    full_list: FullCandidateList,
    on_candidate_lists: usize,
    overlay: Overlay,
}

pub async fn delete_person_confirm(
    _: CandidateListDeletePersonPath,
    context: Context,
    store: PgStore,
    full_list: FullCandidateList,
    candidate: Candidate,
    Query(query): Query<QueryParamState>,
) -> AppResponse<impl IntoResponse> {
    Ok(HtmlTemplate(
        DeleteCandidateTemplate {
            on_candidate_lists: store.count_candidate_lists(candidate.person.id),
            candidate,
            full_list,
            overlay: Overlay::new_edit(&query),
        },
        context,
    ))
}

pub async fn delete_person(
    _: CandidateListDeletePersonPath,
    candidate: Candidate,
    candidate_list: CandidateList,
    _context: Context,
    store: PgStore,
    Query(query): Query<QueryParamState>,
) -> Result<Response, AppError> {
    candidate.person.delete(&store).await?;

    Ok(query.redirect_or(candidate_list.view_path()))
}

#[cfg(test)]
mod tests {
    use axum::extract::Query;

    use super::*;
    use crate::{
        PgStore, QueryParamState,
        structs::{
            candidate_lists::{CandidateListId, FullCandidateList},
            persons::PersonId,
        },
        test_utils::{
            response_body_string, sample_candidate_list, sample_person,
            sample_person_with_last_name,
        },
    };
    use axum::http::{StatusCode, header};
    use axum_extra::routing::TypedPath;

    #[tokio::test]
    async fn delete_person_confirm_contains_delete_button() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        let person = sample_person(PersonId::new());

        list.create(&store).await?;
        person.create(&store).await?;
        list.clone().update_order(&store, &[person.id]).await?;

        let full_list = FullCandidateList::get(&store, list_id).expect("candidate list");
        let candidate = store
            .get_candidate_list(list_id)?
            .get_candidate(&store, person.id)
            .await?;

        let response = delete_person_confirm(
            CandidateListDeletePersonPath {
                list_id,
                person_id: person.id,
            },
            Context::new_test_without_db(),
            store,
            full_list,
            candidate.clone(),
            Query(QueryParamState::default()),
        )
        .await?
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;

        // Delete from application button
        assert!(body.contains(&candidate.delete_path().to_string()));

        // Remove from list button
        let formaction = format!("formaction=\"{}\"", candidate.update_position_path());
        let after_formaction = body
            .find(&formaction)
            .and_then(|pos| body[pos..].find('>').map(|end| &body[pos..pos + end]))
            .expect("formaction not found");
        assert!(after_formaction.contains("value=\"remove\""));

        Ok(())
    }

    #[tokio::test]
    async fn delete_person_removes_from_list_and_redirects() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        let person = sample_person(PersonId::new());
        let other_person = sample_person_with_last_name(PersonId::new(), "Other");

        person.create(&store).await?;
        other_person.create(&store).await?;
        list.candidates = vec![person.id, other_person.id];
        list.create(&store).await?;

        let candidate = store
            .get_candidate_list(list_id)?
            .get_candidate(&store, person.id)
            .await?;

        let context = Context::new_test_without_db();

        let response = delete_person(
            CandidateListDeletePersonPath {
                list_id,
                person_id: person.id,
            },
            candidate,
            list.clone(),
            context,
            store.clone(),
            Query(QueryParamState::default()),
        )
        .await?;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get(header::LOCATION)
            .expect("location header")
            .to_str()
            .expect("location header value");
        assert_eq!(
            location,
            list.view_path()
                .with_query_params(QueryParamState::success())
                .to_string()
        );

        let updated_list = FullCandidateList::get(&store, list_id).expect("candidate list");
        assert_eq!(updated_list.candidates.len(), 1);
        assert_eq!(updated_list.candidates[0].data.person.id, other_person.id);

        let removed = store.get_person(person.id);
        assert!(removed.is_err());

        Ok(())
    }
}
