use crate::structs::candidates::{AddPerson, AddPersonAction};
use askama::Template;
use axum::response::{IntoResponse, Redirect, Response};
use std::collections::HashMap;

use crate::{
    AppError, Context, Form, HtmlTemplate, Overlay, PgStore, QueryParamState,
    candidates::AddPersonForm,
    filters,
    form::FormData,
    structs::{
        candidate_lists::{CandidateListId, FullCandidateList},
        persons::{Person, PersonId},
    },
};

use super::AddCandidatePath;

#[derive(Template)]
#[template(path = "pg/candidates/pages/add.html")]
struct AddExistingPersonTemplate {
    close_action: String,
    persons: Vec<Person>,
    added_candidates: HashMap<PersonId, usize>,
    form: FormData<AddPersonForm>,
    allow_add: bool,
    show_add_all: bool,
    show_remove_all: bool,
    just_added: Option<PersonId>,
    overlay: Overlay,
}

impl AddExistingPersonTemplate {
    /// Creates a template for adding an existing person to the candidate list.
    /// If `added_position` is provided, the template will show all candidates from that position onward as already added to the list.
    fn from(
        list_id: CandidateListId,
        added_position: Option<usize>,
        store: &PgStore,
        form: FormData<AddPersonForm>,
        just_added: Option<PersonId>,
    ) -> Result<Self, AppError> {
        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)?;
        let added_candidates = match added_position {
            Some(pos) => full_list
                .candidates
                .iter()
                .filter(|candidate| candidate.data.position >= pos)
                .map(|candidate| (candidate.data.person.id, candidate.data.position))
                .collect::<HashMap<PersonId, usize>>(),
            None => HashMap::new(),
        };
        let candidate_ids = added_candidates.keys().cloned().collect::<Vec<_>>();
        let persons = full_list.list.persons_not_on_list(store, &candidate_ids)?;
        let allow_add = full_list.candidates.len() < store.candidate_limit();
        let show_add_all = persons.len() != candidate_ids.len() && allow_add;
        let close_action = if !added_candidates.is_empty() {
            full_list
                .list
                .highlight_last_success_path(added_candidates.len())
                .to_string()
        } else {
            full_list.list.view_path().to_string()
        };

        Ok(Self {
            close_action,
            show_add_all,
            allow_add,
            overlay: Overlay::new_create(&QueryParamState::default()),
            show_remove_all: !show_add_all && !candidate_ids.is_empty(),
            persons,
            added_candidates,
            just_added,
            form,
        })
    }
}

/// Handles the logic for adding a person to the candidate list based on the submitted form data.
async fn handle_add_candidate_form(
    add_person: &mut AddPerson,
    full_list: &mut FullCandidateList,
    store: &PgStore,
) -> Result<Option<PersonId>, AppError> {
    match add_person.action {
        AddPersonAction::None => {
            // No action, do nothing.
        }
        AddPersonAction::AddAll => {
            // Enable showing the newly added candidate as already added to the list in the template.
            if add_person.added_position.is_none() {
                add_person.added_position = Some(full_list.list.candidates.len() + 1);
            }

            let persons_not_on_list = full_list.list.persons_not_on_list(store, &[])?;
            let person_ids = persons_not_on_list
                .iter()
                .map(|person| person.id)
                .collect::<Vec<_>>();
            let mut all_persons = full_list.list.candidates.clone();
            all_persons.extend(person_ids);
            all_persons.truncate(store.candidate_limit());

            full_list.list.update_order(store, &all_persons).await?;
        }
        AddPersonAction::RemoveAll => {
            if let Some(position) = add_person.added_position {
                let remaining_candidates = full_list
                    .list
                    .candidates
                    .iter()
                    .take(position.saturating_sub(1))
                    .copied()
                    .collect::<Vec<_>>();
                full_list
                    .list
                    .update_order(store, &remaining_candidates)
                    .await?;
            }

            add_person.added_position = None;
        }
        AddPersonAction::TogglePerson(person_id) => {
            if full_list.list.candidates.contains(&person_id) {
                full_list.list.remove_candidate(store, person_id).await?;
            } else {
                full_list.list.append_candidate(store, person_id).await?;

                // Enable showing the newly added candidate as already added to the list in the template.
                if add_person.added_position.is_none() {
                    add_person.added_position = Some(full_list.list.candidates.len());
                }

                return Ok(Some(person_id));
            }
        }
    }

    Ok(None)
}

pub async fn add_existing_person(
    AddCandidatePath { list_id }: AddCandidatePath,
    context: Context,
    store: PgStore,
) -> Result<impl IntoResponse, AppError> {
    Ok(HtmlTemplate(
        AddExistingPersonTemplate::from(list_id, None, &store, FormData::new(), None)?,
        context,
    ))
}

pub async fn add_person_to_candidate_list(
    _: AddCandidatePath,
    mut full_list: FullCandidateList,
    store: PgStore,
    mut context: Context,
    Form(form): Form<AddPersonForm>,
) -> Result<Response, AppError> {
    context.show_success_alert = true;

    match form.validate_create() {
        Err(form_data) => Ok(HtmlTemplate(
            AddExistingPersonTemplate::from(
                full_list.list.id,
                form_data.data.added_position.parse().ok(),
                &store,
                form_data,
                None,
            )?,
            context,
        )
        .into_response()),
        Ok(mut add_person) => {
            let just_added =
                match handle_add_candidate_form(&mut add_person, &mut full_list, &store).await {
                    Ok(just_added) => just_added,
                    Err(AppError::TooManyCandidates { .. }) => {
                        return Ok(Redirect::to(
                            &full_list.list.max_candidates_reached_path().to_string(),
                        )
                        .into_response());
                    }
                    Err(error) => return Err(error),
                };

            Ok(HtmlTemplate(
                AddExistingPersonTemplate::from(
                    full_list.list.id,
                    add_person.added_position,
                    &store,
                    FormData::new_with_data(add_person.into()),
                    just_added,
                )?,
                context,
            )
            .into_response())
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
            paper_corrections_store, response_body_string, sample_candidate_list, sample_person,
            sample_person_with_last_name,
        },
    };
    use axum::{http::StatusCode, response::IntoResponse};

    #[tokio::test]
    async fn view_candidate_list_renders_persons() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        let person = sample_person(PersonId::new());

        list.create(&store).await?;
        person.create(&store).await?;

        let response = add_existing_person(
            AddCandidatePath { list_id },
            Context::new_test_without_db(),
            store,
        )
        .await?
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Jansen"));
        // Sticky footer with Done button
        assert!(body.contains("<footer>"));

        Ok(())
    }

    #[tokio::test]
    async fn add_person_to_candidate_list_adds_and_renders() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        let person = sample_person_with_last_name(PersonId::new(), "Bakker");

        list.create(&store).await?;
        person.create(&store).await?;

        let context = Context::new_test_without_db();
        let form = AddPersonForm {
            action: person.id.to_string(),
            added_position: String::new(),
        };

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");

        let response = add_person_to_candidate_list(
            AddCandidatePath { list_id },
            full_list,
            store.clone(),
            context,
            Form(form),
        )
        .await?;

        assert_eq!(response.status(), StatusCode::OK);

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");
        assert_eq!(full_list.candidates.len(), 1);
        assert_eq!(full_list.candidates[0].data.person.id, person.id);

        let body = response_body_string(response).await;
        assert!(body.contains("highlight_last=1"));
        assert!(body.contains("success=true"));
        // The just-added person is marked so the frontend can scroll to and highlight it.
        assert!(body.contains(&format!("data-highlight=\"{}\"", person.id)));

        Ok(())
    }

    #[tokio::test]
    async fn add_person_to_candidate_list_adds_when_person_not_on_list() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        let existing_person = sample_person_with_last_name(PersonId::new(), "Jansen");
        let new_person = sample_person_with_last_name(PersonId::new(), "Bakker");

        existing_person.create(&store).await?;
        list.candidates = vec![existing_person.id];
        list.create(&store).await?;
        new_person.create(&store).await?;

        let context = Context::new_test_without_db();
        let form = AddPersonForm {
            action: new_person.id.to_string(),
            added_position: String::new(),
        };

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");

        let response = add_person_to_candidate_list(
            AddCandidatePath { list_id },
            full_list,
            store.clone(),
            context,
            Form(form),
        )
        .await?;

        assert_eq!(response.status(), StatusCode::OK);

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");
        assert_eq!(full_list.candidates.len(), 2);
        assert_eq!(full_list.candidates[0].data.person.id, existing_person.id);
        assert_eq!(full_list.candidates[1].data.person.id, new_person.id);

        let body = response_body_string(response).await;
        assert!(body.contains("highlight_last=1"));
        assert!(body.contains("success=true"));

        Ok(())
    }

    #[tokio::test]
    async fn add_person_to_candidate_list_add_all_adds_missing_persons() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        let existing_person = sample_person_with_last_name(PersonId::new(), "Adams");
        let person_one = sample_person_with_last_name(PersonId::new(), "Bakker");
        let person_two = sample_person_with_last_name(PersonId::new(), "Jansen");

        existing_person.create(&store).await?;
        person_one.create(&store).await?;
        person_two.create(&store).await?;
        list.candidates = vec![existing_person.id];
        list.create(&store).await?;

        let context = Context::new_test_without_db();
        let form = AddPersonForm {
            action: AddPersonAction::AddAll.to_string(),
            added_position: String::new(),
        };

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");

        let response = add_person_to_candidate_list(
            AddCandidatePath { list_id },
            full_list,
            store.clone(),
            context,
            Form(form),
        )
        .await?;

        assert_eq!(response.status(), StatusCode::OK);

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");
        assert_eq!(full_list.candidates.len(), 3);
        assert!(full_list.contains(existing_person.id));
        assert!(full_list.contains(person_one.id));
        assert!(full_list.contains(person_two.id));

        let body = response_body_string(response).await;
        assert!(body.contains("highlight_last=2"));
        assert!(body.contains("success=true"));
        // Bulk add has no single target, so no row is singled out for highlighting.
        assert!(!body.contains("data-highlight"));

        Ok(())
    }

    #[tokio::test]
    async fn add_person_to_candidate_list_add_all_caps_at_max() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        list.create(&store).await?;

        for index in 0..(MAX_CANDIDATES + 5) {
            sample_person_with_last_name(PersonId::new(), &format!("Bakker{index}"))
                .create(&store)
                .await?;
        }

        let context = Context::new_test_without_db();
        let form = AddPersonForm {
            action: AddPersonAction::AddAll.to_string(),
            added_position: String::new(),
        };

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");

        let response = add_person_to_candidate_list(
            AddCandidatePath { list_id },
            full_list,
            store.clone(),
            context,
            Form(form),
        )
        .await?;

        // Bulk add silently caps: no error, just the maximum number of candidates.
        assert_eq!(response.status(), StatusCode::OK);
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

    #[tokio::test]
    async fn add_person_to_candidate_list_toggle_over_max_redirects() -> Result<(), AppError> {
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

        let overflow = sample_person_with_last_name(PersonId::new(), "Zeeman");
        overflow.create(&store).await?;

        let context = Context::new_test_without_db();
        let form = AddPersonForm {
            action: overflow.id.to_string(),
            added_position: String::new(),
        };

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");

        let response = add_person_to_candidate_list(
            AddCandidatePath { list_id },
            full_list,
            store.clone(),
            context,
            Form(form),
        )
        .await?;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get(axum::http::header::LOCATION)
            .expect("location header")
            .to_str()
            .expect("location header value");
        assert!(location.contains("max_candidates_reached=true"));
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

    /// While correcting paper documents the hard maximum does not apply: a
    /// bulk add keeps every person and the 81st toggle is accepted.
    #[tokio::test]
    async fn add_person_to_candidate_list_ignores_max_while_correcting() -> Result<(), AppError> {
        let store = paper_corrections_store().await?;
        let list_id = CandidateListId::new();
        sample_candidate_list(list_id).create(&store).await?;

        for index in 0..(MAX_CANDIDATES + 5) {
            sample_person_with_last_name(PersonId::new(), &format!("Bakker{index}"))
                .create(&store)
                .await?;
        }

        let form = AddPersonForm {
            action: AddPersonAction::AddAll.to_string(),
            added_position: String::new(),
        };
        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");

        let response = add_person_to_candidate_list(
            AddCandidatePath { list_id },
            full_list,
            store.clone(),
            Context::new_test_from_store(&store),
            Form(form),
        )
        .await?;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            store
                .snapshot()
                .candidate_list(list_id)
                .cloned()
                .or_not_found()?
                .candidates
                .len(),
            MAX_CANDIDATES + 5
        );

        Ok(())
    }

    #[tokio::test]
    async fn add_person_to_candidate_list_remove_all_removes_recently_added_persons()
    -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        let existing_person = sample_person_with_last_name(PersonId::new(), "Adams");
        let person_one = sample_person_with_last_name(PersonId::new(), "Bakker");
        let person_two = sample_person_with_last_name(PersonId::new(), "Jansen");

        existing_person.create(&store).await?;
        person_one.create(&store).await?;
        person_two.create(&store).await?;
        list.candidates = vec![existing_person.id];
        list.create(&store).await?;

        let add_all_context = Context::new_test_without_db();
        let add_all_form = AddPersonForm {
            action: AddPersonAction::AddAll.to_string(),
            added_position: String::new(),
        };

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");

        let response = add_person_to_candidate_list(
            AddCandidatePath { list_id },
            full_list,
            store.clone(),
            add_all_context,
            Form(add_all_form),
        )
        .await?;

        assert_eq!(response.status(), StatusCode::OK);

        let remove_all_context = Context::new_test_without_db();
        let remove_all_form = AddPersonForm {
            action: AddPersonAction::RemoveAll.to_string(),
            added_position: "2".into(),
        };

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");

        let response = add_person_to_candidate_list(
            AddCandidatePath { list_id },
            full_list,
            store.clone(),
            remove_all_context,
            Form(remove_all_form),
        )
        .await?;

        assert_eq!(response.status(), StatusCode::OK);

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");
        assert_eq!(full_list.candidates.len(), 1);
        assert_eq!(full_list.candidates[0].data.person.id, existing_person.id);

        Ok(())
    }

    #[tokio::test]
    async fn add_person_to_candidate_list_renders_remove_all_after_adding_everyone()
    -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        let person = sample_person_with_last_name(PersonId::new(), "Bakker");

        list.create(&store).await?;
        person.create(&store).await?;

        let context = Context::new_test_without_db();
        let form = AddPersonForm {
            action: person.id.to_string(),
            added_position: String::new(),
        };

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");

        let response = add_person_to_candidate_list(
            AddCandidatePath { list_id },
            full_list,
            store,
            context,
            Form(form),
        )
        .await?;

        assert_eq!(response.status(), StatusCode::OK);

        let body = response_body_string(response).await;
        assert!(body.contains("Remove all"));
        assert!(!body.contains("Add all candidates"));

        Ok(())
    }

    #[tokio::test]
    async fn add_person_to_candidate_list_removes_candidate() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        let remove_person = sample_person_with_last_name(PersonId::new(), "Jansen");
        let keep_person = sample_person_with_last_name(PersonId::new(), "Bakker");

        remove_person.create(&store).await?;
        keep_person.create(&store).await?;
        list.candidates = vec![remove_person.id, keep_person.id];
        list.create(&store).await?;

        let context = Context::new_test_without_db();
        let form = AddPersonForm {
            action: remove_person.id.to_string(),
            added_position: String::new(),
        };

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");

        let response = add_person_to_candidate_list(
            AddCandidatePath { list_id },
            full_list,
            store.clone(),
            context,
            Form(form),
        )
        .await?;

        assert_eq!(response.status(), StatusCode::OK);

        let full_list = FullCandidateList::get(&store.snapshot(), store.election, list_id)
            .expect("candidate list");
        assert_eq!(full_list.candidates.len(), 1);
        assert_eq!(full_list.candidates[0].data.person.id, keep_person.id);

        Ok(())
    }
}
