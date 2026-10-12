use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, AppResponse, Context, Form, HtmlTemplate, Overlay, PgStore, QueryParamState, filters,
    form::FormData,
    persons::AddressForm,
    structs::{
        candidate_lists::FullCandidateList,
        candidates::Candidate,
        common::{HasSeverity, Problematic},
    },
};

use super::CandidateListUpdateAddressPath;
#[derive(Template)]
#[template(path = "pg/candidates/pages/update_address.html")]
struct CandidateAddressUpdateTemplate {
    should_warn: bool,
    address_unknown: bool,
    candidate: Candidate,
    form: FormData<AddressForm>,
    full_list: FullCandidateList,
    overlay: Overlay,
}

pub async fn update_person_address(
    _: CandidateListUpdateAddressPath,
    context: Context,
    full_list: FullCandidateList,
    candidate: Candidate,
    Query(query): Query<QueryParamState>,
) -> AppResponse<impl IntoResponse> {
    let form = FormData::new_with_data(AddressForm::from(candidate.person.clone()));

    Ok(HtmlTemplate(
        CandidateAddressUpdateTemplate {
            should_warn: query.should_warn(),
            address_unknown: candidate.person.address.is_unknown(),
            form,
            overlay: Overlay::new_edit(&query),
            candidate: candidate.clone(),
            full_list,
        },
        context,
    ))
}

pub async fn update_person_address_submit(
    _: CandidateListUpdateAddressPath,
    context: Context,
    full_list: FullCandidateList,
    candidate: Candidate,
    store: PgStore,
    Query(query): Query<QueryParamState>,
    Form(form): Form<AddressForm>,
) -> Result<Response, AppError> {
    match form.validate_update(&candidate.person) {
        Err(form_data) => Ok(HtmlTemplate(
            CandidateAddressUpdateTemplate {
                should_warn: query.should_warn(),
                address_unknown: candidate.person.address.is_unknown(),
                candidate,
                form: form_data,
                full_list,
                overlay: Overlay::new_edit(&query),
            },
            context,
        )
        .into_response()),
        Ok(mut person) => {
            person.save_address(&store).await?;

            Ok(query.redirect_or(full_list.list.highlight_success_path(candidate.person.id)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Context, Form, PgStore, QueryParamState,
        structs::{candidate_lists::CandidateListId, persons::PersonId},
        test_utils::{
            response_body_string, sample_address_form, sample_candidate_list,
            sample_person_with_last_name,
        },
    };
    use axum::{
        extract::Query,
        http::{StatusCode, header},
        response::IntoResponse,
    };

    #[tokio::test]
    async fn update_person_address_renders_candidate() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        let person = sample_person_with_last_name(PersonId::new(), "Jansen");

        list.create(&store).await?;
        person.create(&store).await?;
        list.clone().update_order(&store, &[person.id]).await?;

        let full_list = FullCandidateList::get(&store, list_id).expect("candidate list");
        let candidate = store
            .get_candidate_list(list_id)?
            .get_candidate(&store, person.id)
            .await?;

        let response = update_person_address(
            CandidateListUpdateAddressPath {
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
    async fn update_person_address_persists_and_redirects() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        let person = sample_person_with_last_name(PersonId::new(), "Jansen");

        person.create(&store).await?;
        list.candidates = vec![person.id];
        list.create(&store).await?;

        let full_list = FullCandidateList::get(&store, list_id).expect("candidate list");
        let candidate = store
            .get_candidate_list(list_id)?
            .get_candidate(&store, person.id)
            .await?;

        let context = Context::new_test_without_db();
        let mut form = sample_address_form();
        form.address.locality = "Rotterdam".to_string();
        let expected_path = full_list
            .list
            .highlight_success_path(candidate.person.id)
            .to_string();

        let response = update_person_address_submit(
            CandidateListUpdateAddressPath {
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
        assert_eq!(
            updated.address.locality.as_deref().map(ToString::to_string),
            Some("Rotterdam".to_string())
        );

        Ok(())
    }

    #[tokio::test]
    async fn update_person_address_invalid_form_renders_template() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        let person = sample_person_with_last_name(PersonId::new(), "Jansen");

        person.create(&store).await?;
        list.candidates = vec![person.id];
        list.create(&store).await?;

        let full_list = FullCandidateList::get(&store, list_id).expect("candidate list");
        let candidate = store
            .get_candidate_list(list_id)?
            .get_candidate(&store, person.id)
            .await?;

        let context = Context::new_test_without_db();
        let mut form = sample_address_form();
        form.address.postal_code = "a".to_string();

        let response = update_person_address_submit(
            CandidateListUpdateAddressPath {
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
        assert!(body.contains("The postal code is not valid"));

        Ok(())
    }
}
