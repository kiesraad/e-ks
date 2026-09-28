use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, AppResponse, Context, Form, HtmlTemplate, Overlay, PgStore, QueryParamState, filters,
    form::FormData,
    persons::PersonalDataForm,
    structs::{
        candidate_lists::FullCandidateList,
        candidates::Candidate,
        common::{HasSeverity, Problematic},
    },
};

use super::CandidateListUpdatePersonPath;
#[derive(Template)]
#[template(path = "pg/candidates/pages/update.html")]
struct CandidateUpdateTemplate {
    full_list: FullCandidateList,
    candidate: Candidate,
    form: FormData<PersonalDataForm>,
    overlay: Overlay,
    locality_unknown: bool,
}

pub async fn update_person(
    _: CandidateListUpdatePersonPath,
    context: Context,
    full_list: FullCandidateList,
    candidate: Candidate,
    Query(query): Query<QueryParamState>,
) -> AppResponse<impl IntoResponse> {
    Ok(HtmlTemplate(
        CandidateUpdateTemplate {
            form: FormData::new_with_data(PersonalDataForm::from(candidate.person.clone())),
            overlay: Overlay::new(&query),
            locality_unknown: candidate
                .person
                .personal_data
                .show_unknown_place_of_residence_warning(),
            candidate,
            full_list,
        },
        context,
    ))
}

pub async fn update_person_submit(
    _: CandidateListUpdatePersonPath,
    context: Context,
    full_list: FullCandidateList,
    mut candidate: Candidate,
    store: PgStore,
    Query(query): Query<QueryParamState>,
    Form(form): Form<PersonalDataForm>,
) -> Result<Response, AppError> {
    match form.validate_update_with_checks(&candidate.person, &store) {
        Err(form_data) => Ok(HtmlTemplate(
            CandidateUpdateTemplate {
                full_list,
                locality_unknown: candidate
                    .person
                    .personal_data
                    .show_unknown_place_of_residence_warning(),
                candidate,
                form: *form_data,
                overlay: Overlay::new(&query),
            },
            context,
        )
        .into_response()),
        Ok(updated) => {
            candidate.person = candidate
                .person
                .update_personal_data(&store, updated.name, updated.personal_data)
                .await?;

            Ok(query.redirect_or(candidate.after_update_path()))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::{
        Context, Form, PgStore, QueryParamState,
        structs::{candidate_lists::CandidateListId, common::PlaceOfResidence, persons::PersonId},
        test_utils::{
            response_body_string, sample_candidate_list, sample_person, sample_person_form,
        },
    };
    use axum::{
        extract::Query,
        http::{StatusCode, header},
        response::IntoResponse,
    };

    #[tokio::test]
    async fn update_person_renders_candidate() -> Result<(), AppError> {
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

        let response = update_person(
            CandidateListUpdatePersonPath {
                list_id,
                person_id: person.id,
            },
            Context::new_test_without_db(),
            full_list,
            candidate,
            Query(QueryParamState::default()),
        )
        .await?
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Jansen"));

        Ok(())
    }

    #[tokio::test]
    async fn update_person_persists_and_redirects() -> Result<(), AppError> {
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

        let context = Context::new_test_without_db();
        let mut form = sample_person_form();
        form.name.last_name = "Updated".to_string();
        let expected_path = format!("{}?&success=true", candidate.after_update_path());

        let response = update_person_submit(
            CandidateListUpdatePersonPath {
                list_id,
                person_id: person.id,
            },
            context,
            full_list,
            candidate,
            store.clone(),
            Query(QueryParamState::default()),
            Form(form),
        )
        .await?;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get(header::LOCATION)
            .expect("location header")
            .to_str()
            .expect("location header value");
        assert_eq!(location, expected_path);

        let updated = store
            .get_persons()
            .into_iter()
            .find(|p| p.id == person.id)
            .expect("updated person");
        assert_eq!(updated.name.last_name.to_string(), "Updated");

        Ok(())
    }

    #[tokio::test]
    async fn update_person_invalid_form_renders_template() -> Result<(), AppError> {
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

        let context = Context::new_test_without_db();
        let mut form = sample_person_form();
        form.name.last_name = " ".to_string();

        let response = update_person_submit(
            CandidateListUpdatePersonPath {
                list_id,
                person_id: person.id,
            },
            context,
            full_list,
            candidate,
            store,
            Query(QueryParamState::default()),
            Form(form),
        )
        .await?
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("This field must not be empty."));

        Ok(())
    }

    #[tokio::test]
    async fn update_person_renders_warning_unknown_bag_locality() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        let mut person = sample_person(PersonId::new());

        person.personal_data.place_of_residence = PlaceOfResidence::from_str("Fantasy City").ok();

        list.create(&store).await?;
        person.create(&store).await?;
        list.clone().update_order(&store, &[person.id]).await?;

        let full_list = FullCandidateList::get(&store, list_id).expect("candidate list");
        let candidate = store
            .get_candidate_list(list_id)?
            .get_candidate(&store, person.id)
            .await?;

        let response = update_person(
            CandidateListUpdatePersonPath {
                list_id,
                person_id: person.id,
            },
            Context::new_test_without_db(),
            full_list,
            candidate,
            Query(QueryParamState::default()),
        )
        .await?
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Place of residence not found in the BAG"));

        Ok(())
    }
}
