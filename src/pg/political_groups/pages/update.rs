use crate::structs::political_groups::PoliticalGroup;
use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, Context, CsbMainStore, HtmlTemplate, PgStore, QueryParamState, filters,
    form::{Form, FormData, ValidationError},
    political_groups::{PoliticalGroupForm, PoliticalGroupSteps},
    structs::{
        csb::RegisteredPoliticalGroup, list_designation::ListDesignation,
        list_submitters::ListSubmitter, name_authorisations::NameAuthorisation,
    },
};

use crate::political_groups::PoliticalGroupUpdatePath;

#[derive(Template)]
#[template(path = "pg/political_groups/pages/update.html")]
struct PoliticalGroupUpdateTemplate {
    form: FormData<PoliticalGroupForm>,
    steps: PoliticalGroupSteps,
    /// The committee's registrations the group can take its information
    /// from, most votes first.
    registered_political_groups: Vec<RegisteredPoliticalGroup>,
    /// The registration picked, whose values the form shows read-only.
    registration: Option<RegisteredPoliticalGroup>,
}

/// The registration `form` picked among `registered`: `None` when the group
/// enters its information by hand, an error when the pick is unknown.
fn picked_registration(
    registered: &[RegisteredPoliticalGroup],
    form: &PoliticalGroupForm,
) -> Result<Option<RegisteredPoliticalGroup>, ValidationError> {
    let picked = form.registered_political_group.trim();
    if picked.is_empty() {
        return Ok(None);
    }
    registered
        .iter()
        .find(|group| group.id.to_string() == picked)
        .cloned()
        .map(Some)
        .ok_or(ValidationError::InvalidValue)
}

pub async fn update_political_group(
    _: PoliticalGroupUpdatePath,
    context: Context,
    store: PgStore,
    main_store: CsbMainStore,
    political_group: PoliticalGroup,
    Query(query): Query<QueryParamState>,
) -> Result<Response, AppError> {
    let steps = PoliticalGroupSteps::new(&store, query.is_initial())?;
    let registered_political_groups = main_store.registered_political_groups();
    // A registration the committee deleted since leaves the fields unlocked.
    let registration = political_group
        .registration_id()
        .and_then(|id| main_store.get_registered_political_group(id).ok());
    let mut form_data = PoliticalGroupForm::from(political_group);
    if let Some(registration) = &registration {
        form_data = form_data.with_registration(registration);
    }
    let form = FormData::new_with_data(form_data);

    Ok(HtmlTemplate(
        PoliticalGroupUpdateTemplate {
            form,
            steps,
            registered_political_groups,
            registration,
        },
        context,
    )
    .into_response())
}

pub async fn update_political_group_submit(
    _: PoliticalGroupUpdatePath,
    context: Context,
    political_group: PoliticalGroup,
    store: PgStore,
    main_store: CsbMainStore,
    Query(query): Query<QueryParamState>,
    Form(form): Form<PoliticalGroupForm>,
) -> Result<Response, AppError> {
    let steps = PoliticalGroupSteps::new(&store, query.is_initial())?;
    let registered_political_groups = main_store.registered_political_groups();

    let (form_data, registration) = match picked_registration(&registered_political_groups, &form) {
        Err(error) => (
            Err(FormData::new_with_errors(
                form,
                vec![("registered_political_group".to_string(), error)],
            )),
            None,
        ),
        Ok(Some(registration)) => (
            form.with_registration(&registration)
                .validate_update(&political_group),
            Some(registration),
        ),
        Ok(None) => (form.validate_update(&political_group), None),
    };

    match form_data {
        Err(form_data) => Ok(HtmlTemplate(
            PoliticalGroupUpdateTemplate {
                form: form_data,
                steps,
                registered_political_groups,
                registration,
            },
            context,
        )
        .into_response()),
        Ok(mut political_group) => {
            political_group.bind_registration(registration.as_ref().map(|r| r.id));
            political_group.update(&store).await?;

            Ok(query.redirect_or_preserving_initial(NameAuthorisation::list_path()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppError, Context, CsbMainAction, CsbUser, Form, PgStore, QueryParamState,
        structs::{
            common::{Appellation, PreviousElectionResults},
            csb::{RegisteredPoliticalGroupId, sample_registered_political_group},
            list_designation::ListDesignation,
            name_authorisations::NameAuthorisationId,
        },
        test_utils::{
            response_body_string, sample_name_authorisation, sample_political_group_form,
        },
    };

    /// `body` with every run of whitespace, including the line breaks between
    /// attributes, collapsed to one space.
    fn single_spaced(body: String) -> String {
        body.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    async fn main_store_with(
        groups: &[crate::structs::csb::RegisteredPoliticalGroup],
    ) -> CsbMainStore {
        let store = CsbMainStore::new_for_test();
        for group in groups {
            store
                .update(
                    CsbMainAction::CreateRegisteredPoliticalGroup(group.clone())
                        .by(CsbUser::new_test()),
                )
                .await
                .unwrap();
        }
        store
    }
    use axum::{
        http::{StatusCode, header},
        response::IntoResponse,
    };
    use axum_extra::routing::TypedPath;

    #[tokio::test]
    async fn update_political_group_renders_existing_data() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let political_group = store.snapshot().get_political_group().clone();

        let response = update_political_group(
            PoliticalGroupUpdatePath {},
            Context::new_test_without_db(),
            store,
            CsbMainStore::new_for_test(),
            political_group,
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("name=\"csrf_token\""));
        assert!(body.contains("Kiesraad Demo"));
        // Without registrations there is nothing to pick from.
        assert!(!body.contains("name=\"registered_political_group\""));
        assert!(!body.contains("readonly"));

        Ok(())
    }

    #[tokio::test]
    async fn update_political_group_offers_the_registrations() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let political_group = store.snapshot().get_political_group().clone();
        let main_store =
            main_store_with(&[sample_registered_political_group("De Partij", 100, 3)]).await;

        let response = update_political_group(
            PoliticalGroupUpdatePath {},
            Context::new_test_without_db(),
            store,
            main_store.clone(),
            political_group,
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        let body = single_spaced(response_body_string(response).await);
        let registration = &main_store.registered_political_groups()[0];
        assert!(body.contains("name=\"registered_political_group\""));
        assert!(body.contains(&format!(
            "value=\"{}\" data-appellation=\"De Partij\" data-previous-election-results=\"one_to_fifteen_seats\" >",
            registration.id
        )));
        // Nothing picked yet: the fields are entered by hand.
        assert!(!body.contains("selected"));
        assert!(!body.contains("readonly"));
        assert!(!body.contains("disabled"));

        Ok(())
    }

    #[tokio::test]
    async fn a_bound_registration_locks_its_fields() -> Result<(), AppError> {
        let registration = sample_registered_political_group("De Partij", 100, 16);
        let main_store = main_store_with(std::slice::from_ref(&registration)).await;
        let store = PgStore::new_for_test();
        // Bound by id: the stored appellation does not decide the match.
        let political_group = PoliticalGroup {
            list_designation: Some(ListDesignation::Standalone {
                registration: Some(registration.id),
            }),
            appellation: "Oude Naam".parse().ok(),
            previous_election_results: Some(PreviousElectionResults::ZeroSeats),
        };
        political_group.update(&store).await?;

        let response = update_political_group(
            PoliticalGroupUpdatePath {},
            Context::new_test_without_db(),
            store,
            main_store,
            political_group,
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        let body = single_spaced(response_body_string(response).await);
        assert!(body.contains(&format!(
            "value=\"{}\" data-appellation=\"De Partij\" data-previous-election-results=\"sixteen_or_more_seats\" selected>",
            registration.id
        )));
        assert!(body.contains("value=\"De Partij\" id=\"appellation\" readonly"));
        assert!(body.contains("id=\"previous_election_results_16\" checked disabled"));
        assert!(body.contains("id=\"previous_election_results_0\" disabled"));

        Ok(())
    }

    #[tokio::test]
    async fn a_deleted_registration_unlocks_the_fields() -> Result<(), AppError> {
        let main_store =
            main_store_with(&[sample_registered_political_group("De Partij", 100, 3)]).await;
        let store = PgStore::new_for_test();
        let political_group = PoliticalGroup {
            list_designation: Some(ListDesignation::Standalone {
                registration: Some(RegisteredPoliticalGroupId::new()),
            }),
            ..store.snapshot().get_political_group().clone()
        };
        political_group.update(&store).await?;

        let response = update_political_group(
            PoliticalGroupUpdatePath {},
            Context::new_test_without_db(),
            store,
            main_store,
            political_group,
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        let body = single_spaced(response_body_string(response).await);
        assert!(!body.contains("selected"));
        assert!(body.contains("value=\"Kiesraad Demo\" id=\"appellation\" />"));

        Ok(())
    }

    #[tokio::test]
    async fn submitting_a_registration_takes_over_its_values() -> Result<(), AppError> {
        let registration = sample_registered_political_group("De Partij", 100, 3);
        let main_store = main_store_with(std::slice::from_ref(&registration)).await;
        let store = PgStore::new_for_test();

        // The locked fields are submitted as whatever the browser sent, and
        // not trusted.
        let form = PoliticalGroupForm {
            registered_political_group: registration.id.to_string(),
            previous_election_results: "sixteen_or_more_seats".to_string(),
            appellation: "Gemanipuleerd".to_string(),
        };
        let response = update_political_group_submit(
            PoliticalGroupUpdatePath {},
            Context::new_test_without_db(),
            store.snapshot().get_political_group().clone(),
            store.clone(),
            main_store,
            Query(QueryParamState::default()),
            Form(form),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let updated = store.snapshot().get_political_group().clone();
        assert_eq!(updated.registration_id(), Some(registration.id));
        assert_eq!(
            updated.appellation.as_deref().map(ToString::to_string),
            Some("De Partij".to_string())
        );
        assert_eq!(
            updated.previous_election_results,
            Some(PreviousElectionResults::OneToFifteenSeats)
        );

        Ok(())
    }

    #[tokio::test]
    async fn submitting_by_hand_unbinds_the_registration() -> Result<(), AppError> {
        let registration = sample_registered_political_group("De Partij", 100, 3);
        let main_store = main_store_with(std::slice::from_ref(&registration)).await;
        let store = PgStore::new_for_test();
        PoliticalGroup {
            list_designation: Some(ListDesignation::Standalone {
                registration: Some(registration.id),
            }),
            ..store.snapshot().get_political_group().clone()
        }
        .update(&store)
        .await?;

        let response = update_political_group_submit(
            PoliticalGroupUpdatePath {},
            Context::new_test_without_db(),
            store.snapshot().get_political_group().clone(),
            store.clone(),
            main_store,
            Query(QueryParamState::default()),
            Form(sample_political_group_form()),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let updated = store.snapshot().get_political_group().clone();
        assert_eq!(updated.registration_id(), None);
        assert_eq!(updated.list_designation, Some(ListDesignation::default()));
        assert_eq!(
            updated.appellation.as_deref().map(ToString::to_string),
            Some("Updated Appellation".to_string())
        );

        Ok(())
    }

    #[tokio::test]
    async fn a_combined_list_keeps_its_designation_on_submit() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let combined = ListDesignation::Combined {
            registrations: vec![RegisteredPoliticalGroupId::new()],
        };
        PoliticalGroup {
            list_designation: Some(combined.clone()),
            ..store.snapshot().get_political_group().clone()
        }
        .update(&store)
        .await?;

        let response = update_political_group_submit(
            PoliticalGroupUpdatePath {},
            Context::new_test_without_db(),
            store.snapshot().get_political_group().clone(),
            store.clone(),
            CsbMainStore::new_for_test(),
            Query(QueryParamState::default()),
            Form(sample_political_group_form()),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            store
                .snapshot()
                .get_political_group()
                .clone()
                .list_designation,
            Some(combined)
        );

        Ok(())
    }

    #[tokio::test]
    async fn submitting_an_unknown_registration_is_refused() -> Result<(), AppError> {
        let registration = sample_registered_political_group("De Partij", 100, 3);
        let main_store = main_store_with(std::slice::from_ref(&registration)).await;
        let store = PgStore::new_for_test();
        PoliticalGroup {
            list_designation: Some(ListDesignation::Standalone {
                registration: Some(registration.id),
            }),
            ..store.snapshot().get_political_group().clone()
        }
        .update(&store)
        .await?;

        let mut form = sample_political_group_form();
        form.registered_political_group = RegisteredPoliticalGroupId::new().to_string();
        let response = update_political_group_submit(
            PoliticalGroupUpdatePath {},
            Context::new_test_without_db(),
            store.snapshot().get_political_group().clone(),
            store.clone(),
            main_store,
            Query(QueryParamState::default()),
            Form(form),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("The provided value is not valid."));
        let unchanged = store.snapshot().get_political_group().clone();
        assert_eq!(unchanged.registration_id(), Some(registration.id));
        assert_eq!(
            unchanged.appellation.as_deref().map(ToString::to_string),
            Some("Kiesraad Demo".to_string())
        );

        Ok(())
    }

    /// In paper-corrections mode the app layout shows the warning banner
    /// (with the group name and an exit form) and hides the finalise menu.
    #[tokio::test]
    async fn renders_corrections_banner_and_hides_finalise_in_corrections_mode()
    -> Result<(), AppError> {
        let csb_store = crate::CsbStore::new_for_test();
        csb_store.set_political_group(crate::test_utils::sample_political_group());
        let store = csb_store.paper_corrections();
        let context = Context::new(
            &store,
            crate::Session::new_test_with_locale(crate::Locale::En),
        );
        let political_group = store.snapshot().get_political_group().clone();

        let response = update_political_group(
            PoliticalGroupUpdatePath {},
            context,
            store,
            CsbMainStore::new_for_test(),
            political_group,
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("paper-corrections-banner"));
        // The banner names the group being corrected and warns to only enter
        // handwritten corrections from the submitted paper documents.
        assert!(body.contains("You are correcting “Kiesraad Demo”."));
        assert!(body.contains("submitted paper documents"));
        // Leaving corrections mode posts to the stop route of the CSB stream.
        assert!(body.contains(&format!(
            "/csb/examination/{}/paper-corrections/stop",
            csb_store.stream_id
        )));
        // The finalise menu item is hidden.
        assert!(!body.contains("/finalise"));

        Ok(())
    }

    #[tokio::test]
    async fn update_political_group_persists_and_redirects() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let political_group = store.snapshot().get_political_group().clone();

        sample_name_authorisation(NameAuthorisationId::new())
            .create(&store)
            .await?;

        let context = Context::new_test_without_db();
        let mut form = sample_political_group_form();
        form.appellation = "a".repeat(Appellation::MAX_CHAR_COUNT); // max length

        let response = update_political_group_submit(
            PoliticalGroupUpdatePath {},
            context,
            political_group,
            store.clone(),
            CsbMainStore::new_for_test(),
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
        assert_eq!(
            location,
            NameAuthorisation::list_path()
                .with_query_params(QueryParamState::success())
                .to_string()
        );

        let updated = store.snapshot().get_political_group().clone();
        assert_eq!(
            updated.previous_election_results,
            Some(PreviousElectionResults::OneToFifteenSeats)
        );
        assert_eq!(
            updated.appellation.as_deref().map(ToString::to_string),
            Some("a".repeat(Appellation::MAX_CHAR_COUNT))
        );

        Ok(())
    }

    #[tokio::test]
    async fn update_political_group_invalid_form_renders_template() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        sample_name_authorisation(NameAuthorisationId::new())
            .create(&store)
            .await?;

        let context = Context::new_test_without_db();
        let mut form = sample_political_group_form();

        form.appellation = "a".repeat(Appellation::MAX_CHAR_COUNT + 1); // Invalid value (too long)

        let response = update_political_group_submit(
            PoliticalGroupUpdatePath {},
            context,
            store.snapshot().get_political_group().clone(),
            store,
            CsbMainStore::new_for_test(),
            Query(QueryParamState::default()),
            Form(form),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("The value is too long"));

        Ok(())
    }
}
