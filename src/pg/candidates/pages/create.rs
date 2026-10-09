use crate::structs::persons::Person;
use askama::Template;
use axum::response::{IntoResponse, Redirect, Response};

use crate::{
    AppError, Context, Form, HtmlTemplate, Overlay, PgStore, QueryParamState, filters,
    form::FormData, persons::PersonalDataForm, structs::candidate_lists::FullCandidateList,
};

use super::CreateCandidatePath;
#[derive(Template)]
#[template(path = "pg/candidates/pages/create.html")]
struct PersonCreateTemplate {
    full_list: FullCandidateList,
    form: FormData<PersonalDataForm>,
    overlay: Overlay,
}

pub async fn create_person_candidate_list(
    _: CreateCandidatePath,
    context: Context,
    full_list: FullCandidateList,
) -> Result<impl IntoResponse, AppError> {
    Ok(HtmlTemplate(
        PersonCreateTemplate {
            full_list,
            form: FormData::new(),
            overlay: Overlay::new_create(&QueryParamState::default()),
        },
        context,
    )
    .into_response())
}

pub async fn create_person_candidate_list_submit(
    _: CreateCandidatePath,
    context: Context,
    full_list: FullCandidateList,
    store: PgStore,
    Form(form): Form<PersonalDataForm>,
) -> Result<Response, AppError> {
    match form.validate_create_with_checks(&store) {
        Err(form_data) => Ok(HtmlTemplate(
            PersonCreateTemplate {
                full_list,
                form: *form_data,
                overlay: Overlay::new_create(&QueryParamState::default()),
            },
            context,
        )
        .into_response()),
        Ok(person) => {
            if full_list.list.candidates.len() >= store.candidate_limit() {
                return Ok(
                    Redirect::to(&full_list.list.max_candidates_reached_path().to_string())
                        .into_response(),
                );
            }

            let person =
                Person::create_from_personal_data(&store, person.name, person.personal_data)
                    .await?;

            let mut list = full_list.list;
            list.append_candidate(&store, person.id).await?;
            let candidate = list.get_candidate(&store, person.id).await?;

            Ok(Redirect::to(&candidate.after_create_path()).into_response())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OrNotFound;

    use crate::{
        Context, Form, MAX_CANDIDATES, PgStore,
        structs::{candidate_lists::CandidateListId, persons::PersonId},
        test_utils::{
            paper_corrections_store, response_body_string, sample_candidate_list,
            sample_person_form, sample_person_with_last_name,
        },
    };
    use axum::{
        http::{StatusCode, header},
        response::IntoResponse,
    };

    #[tokio::test]
    async fn create_person_candidate_list_renders_form() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        list.create(&store).await?;

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");

        let response = create_person_candidate_list(
            CreateCandidatePath { list_id },
            Context::new_test_without_db(),
            full_list,
        )
        .await?
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains(&list.create_candidate_path().to_string()));
        assert!(body.contains("name=\"csrf_token\""));

        Ok(())
    }

    #[tokio::test]
    async fn create_person_candidate_list_persists_and_redirects() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        list.create(&store).await?;

        let context = Context::new_test_without_db();
        let form = sample_person_form();

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");

        let response = create_person_candidate_list_submit(
            CreateCandidatePath { list_id },
            context,
            full_list,
            store.clone(),
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

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");
        assert_eq!(full_list.candidates.len(), 1);
        let candidate = full_list.candidates.first().expect("candidate");
        assert_eq!(location, candidate.data.after_create_path());

        Ok(())
    }

    #[tokio::test]
    async fn create_person_candidate_list_redirects_when_full() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);

        let mut full = Vec::new();
        for index in 0..MAX_CANDIDATES {
            let person = sample_person_with_last_name(PersonId::new(), &format!("Bakker{index}"));
            person.create(&store).await?;
            full.push(person.id);
        }
        list.candidates = full;
        list.create(&store).await?;

        let context = Context::new_test_without_db();
        let form = sample_person_form();

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");

        let response = create_person_candidate_list_submit(
            CreateCandidatePath { list_id },
            context,
            full_list,
            store.clone(),
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
        assert!(location.contains("max_candidates_reached=true"));
        // No orphan person is created when the list is already full.
        assert_eq!(store.snapshot().person_count(), MAX_CANDIDATES);
        assert_eq!(
            store
                .snapshot()
                .candidate_list(list_id)
                .cloned()
                .or_not_found()?
                .candidates
                .len(),
            MAX_CANDIDATES
        );

        Ok(())
    }

    /// Paper corrections record the list as handed in, so an 81st candidate
    /// must be enterable there instead of bouncing off the hard maximum.
    #[tokio::test]
    async fn create_person_candidate_list_allows_extra_candidate_while_correcting()
    -> Result<(), AppError> {
        let store = paper_corrections_store().await?;
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);

        let mut full = Vec::new();
        for index in 0..MAX_CANDIDATES {
            let person = sample_person_with_last_name(PersonId::new(), &format!("Bakker{index}"));
            person.create(&store).await?;
            full.push(person.id);
        }
        list.candidates = full;
        list.create(&store).await?;

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");

        let response = create_person_candidate_list_submit(
            CreateCandidatePath { list_id },
            Context::new_test_from_store(&store),
            full_list,
            store.clone(),
            Form(sample_person_form()),
        )
        .await?;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get(header::LOCATION)
            .expect("location header")
            .to_str()
            .expect("location header value");
        assert!(!location.contains("max_candidates_reached=true"));
        assert_eq!(
            store
                .snapshot()
                .candidate_list(list_id)
                .cloned()
                .or_not_found()?
                .candidates
                .len(),
            MAX_CANDIDATES + 1
        );

        Ok(())
    }

    #[tokio::test]
    async fn create_person_candidate_list_invalid_form_renders_template() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        list.create(&store).await?;

        let context = Context::new_test_without_db();
        let mut form = sample_person_form();
        form.name.last_name = " ".to_string();

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");

        let response = create_person_candidate_list_submit(
            CreateCandidatePath { list_id },
            context,
            full_list,
            store,
            Form(form),
        )
        .await?
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("This field must not be empty."));

        Ok(())
    }
}
