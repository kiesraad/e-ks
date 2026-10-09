use crate::structs::{
    candidates::{Candidate, CandidatePosition},
    political_groups::PoliticalGroup,
};
use askama::Template;
use axum::{extract::Query, response::IntoResponse};

use crate::{
    AppError, Context, Form, HtmlTemplate, Overlay, PgStore, QueryParamState,
    candidates::CandidatePositionForm,
    filters,
    form::FormData,
    redirect_success,
    structs::{
        candidate_lists::FullCandidateList,
        common::{FormAction, HasSeverity, Problematic},
    },
};

use super::UpdateCandidatePositionPath;

#[derive(Template)]
#[template(path = "pg/candidates/pages/update_position.html")]
struct UpdateCandidatePositionTemplate {
    full_list: FullCandidateList,
    candidate: Candidate,
    form: FormData<CandidatePositionForm>,
    overlay: Overlay,
    position_warning: Option<PositionWarning>,
}

struct PositionWarning {
    max_candidates: usize,
}

impl PositionWarning {
    fn new_opt(pg: &PoliticalGroup, candidate: &Candidate) -> Option<Self> {
        let max_candidates = pg.get_max_candidates();
        if max_candidates < candidate.position {
            Some(PositionWarning { max_candidates })
        } else {
            None
        }
    }
}

pub async fn update_candidate_position(
    _: UpdateCandidatePositionPath,
    context: Context,
    full_list: FullCandidateList,
    political_group: PoliticalGroup,
    candidate: Candidate,
    Query(query): Query<QueryParamState>,
) -> Result<impl IntoResponse, AppError> {
    let candidate_position = CandidatePosition {
        position: candidate.position,
        action: FormAction::Save,
    };
    let form = FormData::new_with_data(CandidatePositionForm::from(candidate_position.clone()));

    Ok(HtmlTemplate(
        UpdateCandidatePositionTemplate {
            position_warning: PositionWarning::new_opt(&political_group, &candidate),
            candidate,
            full_list,
            form,
            overlay: Overlay::new_edit(&query),
        },
        context,
    ))
}

pub async fn update_candidate_position_submit(
    _: UpdateCandidatePositionPath,
    context: Context,
    full_list: FullCandidateList,
    candidate: Candidate,
    store: PgStore,
    Query(query): Query<QueryParamState>,
    Form(form): Form<CandidatePositionForm>,
) -> Result<impl IntoResponse, AppError> {
    let candidate_position = CandidatePosition {
        position: candidate.position,
        action: FormAction::Save,
    };

    match form.validate_update(&candidate_position) {
        Err(form_data) => Ok(HtmlTemplate(
            UpdateCandidatePositionTemplate {
                position_warning: PositionWarning::new_opt(
                    &store.snapshot().political_group().clone(),
                    &candidate,
                ),
                candidate,
                full_list,
                form: form_data,
                overlay: Overlay::new_edit(&query),
            },
            context,
        )
        .into_response()),
        Ok(position_form) => match position_form.action {
            FormAction::Remove => {
                let mut list = full_list.list;
                list.remove_candidate(&store, candidate.person.id).await?;

                Ok(redirect_success(list.view_path()))
            }
            FormAction::Save => {
                let mut list = full_list.list;
                list.update_position(&store, candidate.person.id, position_form.position)
                    .await?;

                Ok(query.redirect_or(candidate.update_path()))
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OrNotFound;
    use crate::{
        Context, Form, PgStore, QueryParamState,
        structs::{candidate_lists::CandidateListId, persons::PersonId},
        test_utils::{
            response_body_string, sample_candidate_list, sample_person,
            sample_person_with_last_name,
        },
    };
    use axum::{extract::Query, http::StatusCode, response::IntoResponse};

    fn sample_position_form(position: usize, action: &str) -> CandidatePositionForm {
        CandidatePositionForm {
            position: position.to_string(),
            action: action.to_string(),
        }
    }

    #[tokio::test]
    async fn update_candidate_position_renders_form() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        let person = sample_person(PersonId::new());

        person.create(&store).await?;
        list.candidates = vec![person.id];
        list.create(&store).await?;

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");
        let political_group = store.snapshot().political_group().clone();
        let candidate = list.get_candidate(&store, person.id).await?;

        let response = update_candidate_position(
            UpdateCandidatePositionPath {
                list_id,
                person_id: person.id,
            },
            Context::new_test_without_db(),
            full_list,
            political_group,
            candidate.clone(),
            Query(QueryParamState::default()),
        )
        .await?
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains(&candidate.update_position_path().to_string()));
        assert!(body.contains("Jansen"));

        Ok(())
    }

    #[tokio::test]
    async fn update_candidate_position_renders_form_with_warning() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        let mut candidates = Vec::new();

        for _ in 0..100 {
            let person = sample_person(PersonId::new());
            person.create(&store).await?;
            candidates.push(person.id);
        }
        list.candidates = candidates.clone();
        list.create(&store).await?;

        let last_candidate_id = candidates.last().unwrap().to_owned();

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");
        let political_group = store.snapshot().political_group().clone();
        let candidate = list.get_candidate(&store, last_candidate_id).await?;

        let response = update_candidate_position(
            UpdateCandidatePositionPath {
                list_id,
                person_id: last_candidate_id,
            },
            Context::new_test_without_db(),
            full_list,
            political_group,
            candidate.clone(),
            Query(QueryParamState::default()),
        )
        .await?
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains(&candidate.update_position_path().to_string()));
        assert!(body.contains("Jansen"));
        assert!(body.contains("Warning: More than 50 candidates on this list"));

        Ok(())
    }

    #[tokio::test]
    async fn update_candidate_position_moves_candidate() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        let person_a = sample_person_with_last_name(PersonId::new(), "Jansen");
        let person_b = sample_person_with_last_name(PersonId::new(), "Bakker");

        list.create(&store).await?;
        person_a.create(&store).await?;
        person_b.create(&store).await?;
        list.clone()
            .update_order(&store, &[person_a.id, person_b.id])
            .await?;

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");
        let candidate = store
            .snapshot()
            .candidate_list(list_id)
            .cloned()
            .or_not_found()?
            .get_candidate(&store, person_a.id)
            .await?;

        let context = Context::new_test_without_db();
        let form = sample_position_form(2, "save");

        let response = update_candidate_position_submit(
            UpdateCandidatePositionPath {
                list_id,
                person_id: person_a.id,
            },
            context,
            full_list,
            candidate,
            store.clone(),
            Query(QueryParamState::default()),
            Form(form),
        )
        .await?
        .into_response();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");
        assert_eq!(full_list.candidates.len(), 2);
        assert_eq!(full_list.candidates[0].data.person.id, person_b.id);
        assert_eq!(full_list.candidates[1].data.person.id, person_a.id);

        Ok(())
    }

    #[tokio::test]
    async fn update_candidate_position_removes_candidate() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        let person_a = sample_person_with_last_name(PersonId::new(), "Jansen");
        let person_b = sample_person_with_last_name(PersonId::new(), "Bakker");

        list.create(&store).await?;
        person_a.create(&store).await?;
        person_b.create(&store).await?;
        list.clone()
            .update_order(&store, &[person_a.id, person_b.id])
            .await?;

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");
        let candidate = store
            .snapshot()
            .candidate_list(list_id)
            .cloned()
            .or_not_found()?
            .get_candidate(&store, person_a.id)
            .await?;

        let context = Context::new_test_without_db();
        let form = sample_position_form(1, "remove");

        let response = update_candidate_position_submit(
            UpdateCandidatePositionPath {
                list_id,
                person_id: person_a.id,
            },
            context,
            full_list,
            candidate,
            store.clone(),
            Query(QueryParamState::default()),
            Form(form),
        )
        .await?
        .into_response();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");
        assert_eq!(full_list.candidates.len(), 1);
        assert_eq!(full_list.candidates[0].data.person.id, person_b.id);

        Ok(())
    }
}
