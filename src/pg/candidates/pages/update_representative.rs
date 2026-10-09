use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, AppResponse, Context, Form, HtmlTemplate, Overlay, PgStore, QueryParamState, filters,
    form::FormData,
    persons::RepresentativeForm,
    structs::{
        candidate_lists::FullCandidateList,
        candidates::Candidate,
        common::{HasSeverity, Problematic},
    },
};

use super::UpdateRepresentativePath;
#[derive(Template)]
#[template(path = "pg/candidates/pages/update_representative.html")]
struct UpdateRepresentativeTemplate {
    should_warn: bool,
    address_unknown: bool,
    full_list: FullCandidateList,
    candidate: Candidate,
    form: FormData<RepresentativeForm>,
    overlay: Overlay,
}

pub async fn update_representative(
    _: UpdateRepresentativePath,
    context: Context,
    full_list: FullCandidateList,
    candidate: Candidate,
    Query(query): Query<QueryParamState>,
) -> AppResponse<impl IntoResponse> {
    let form = FormData::new_with_data(RepresentativeForm::from(
        candidate.person.clone().representative.unwrap_or_default(),
    ));

    Ok(HtmlTemplate(
        UpdateRepresentativeTemplate {
            should_warn: query.should_warn(),
            address_unknown: candidate.person.representative_address_unknown(),
            candidate: candidate.clone(),
            full_list,
            form,
            overlay: Overlay::new_edit(&query),
        },
        context,
    ))
}

pub async fn update_representative_submit(
    _: UpdateRepresentativePath,
    context: Context,
    full_list: FullCandidateList,
    candidate: Candidate,
    store: PgStore,
    Query(query): Query<QueryParamState>,
    Form(form): Form<RepresentativeForm>,
) -> Result<Response, AppError> {
    let representative = candidate.person.clone().representative.unwrap_or_default();
    match form.validate_update(&representative) {
        Err(form_data) => Ok(HtmlTemplate(
            UpdateRepresentativeTemplate {
                should_warn: query.should_warn(),
                address_unknown: candidate.person.representative_address_unknown(),
                candidate,
                full_list,
                form: form_data,
                overlay: Overlay::new_edit(&query),
            },
            context,
        )
        .into_response()),
        Ok(representative) => {
            candidate
                .person
                .save_representative(&store, representative)
                .await?;

            Ok(query.redirect_or(full_list.list.highlight_success_path(candidate.person.id)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppError, Context, Form, PgStore, QueryParamState,
        structs::{candidate_lists::CandidateListId, persons::PersonId},
        test_utils::{
            extract_csrf_token, response_body_string, sample_candidate_list, sample_person,
            sample_representative_form,
        },
    };
    use axum::{
        extract::Query,
        http::{StatusCode, header},
        response::IntoResponse,
    };

    #[tokio::test]
    async fn update_representative_renders_candidate() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        let person = sample_person(PersonId::new());

        list.create(&store).await?;
        person.create(&store).await?;
        list.clone().update_order(&store, &[person.id]).await?;

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");
        let candidate = store
            .get_candidate_list(list_id)?
            .get_candidate(&store, person.id)
            .await?;

        let response = update_representative(
            UpdateRepresentativePath {
                list_id,
                person_id: person.id,
            },
            Context::new_test_without_db(),
            full_list,
            candidate,
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Jansen"));

        Ok(())
    }

    #[tokio::test]
    async fn update_representative_renders_valid_csrf_token() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        let person = sample_person(PersonId::new());

        list.create(&store).await?;
        person.create(&store).await?;
        list.clone().update_order(&store, &[person.id]).await?;

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");
        let candidate = store
            .get_candidate_list(list_id)?
            .get_candidate(&store, person.id)
            .await?;

        let context = Context::new_test_without_db();
        let expected_csrf = context.session.csrf_token().clone();

        let response = update_representative(
            UpdateRepresentativePath {
                list_id,
                person_id: person.id,
            },
            context,
            full_list,
            candidate,
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        let csrf_token = extract_csrf_token(&body).expect("csrf token");
        assert_eq!(csrf_token, expected_csrf);

        Ok(())
    }

    #[tokio::test]
    async fn update_representative_persists_and_redirects() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        let person = sample_person(PersonId::new());

        list.create(&store).await?;
        person.create(&store).await?;
        list.clone().update_order(&store, &[person.id]).await?;

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");
        let candidate = store
            .get_candidate_list(list_id)?
            .get_candidate(&store, person.id)
            .await?;

        let context = Context::new_test_without_db();
        let mut form = sample_representative_form();
        form.name.last_name = "Smit".to_string();

        let expected_path = full_list
            .list
            .highlight_success_path(candidate.person.id)
            .to_string();
        let response = update_representative_submit(
            UpdateRepresentativePath {
                list_id,
                person_id: person.id,
            },
            context,
            full_list,
            candidate.clone(),
            store.clone(),
            Query(QueryParamState::default()),
            Form(form),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get(header::LOCATION)
            .expect("location header")
            .to_str()
            .expect("location header value");
        assert_eq!(location, expected_path);

        let updated = store.get_person(person.id)?;
        assert_eq!(
            updated.representative.unwrap().name.last_name.to_string(),
            "Smit"
        );

        Ok(())
    }

    #[tokio::test]
    async fn update_representative_invalid_form_renders_template() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        let person = sample_person(PersonId::new());

        list.create(&store).await?;
        person.create(&store).await?;
        list.clone().update_order(&store, &[person.id]).await?;

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");
        let candidate = store
            .get_candidate_list(list_id)?
            .get_candidate(&store, person.id)
            .await?;

        let context = Context::new_test_without_db();
        let mut form = sample_representative_form();
        form.address.postal_code = "a".to_string();

        let response = update_representative_submit(
            UpdateRepresentativePath {
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
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("The postal code is not valid"));

        Ok(())
    }
}
