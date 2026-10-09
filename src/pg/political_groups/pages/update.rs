use crate::structs::political_groups::PoliticalGroup;
use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, Context, HtmlTemplate, PgStore, QueryParamState, filters,
    form::{Form, FormData},
    political_groups::{PoliticalGroupForm, PoliticalGroupSteps},
    structs::{
        list_designation::ListDesignation, list_submitters::ListSubmitter,
        name_authorisations::NameAuthorisation,
    },
};

use crate::political_groups::PoliticalGroupUpdatePath;

#[derive(Template)]
#[template(path = "pg/political_groups/pages/update.html")]
struct PoliticalGroupUpdateTemplate {
    form: FormData<PoliticalGroupForm>,
    steps: PoliticalGroupSteps,
}

pub async fn update_political_group(
    _: PoliticalGroupUpdatePath,
    context: Context,
    store: PgStore,
    political_group: PoliticalGroup,
    Query(query): Query<QueryParamState>,
) -> Result<Response, AppError> {
    let steps = PoliticalGroupSteps::new(&store.snapshot(), query.is_initial())?;

    Ok(HtmlTemplate(
        PoliticalGroupUpdateTemplate {
            form: FormData::new_with_data(political_group.clone().into()),
            steps,
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
    Query(query): Query<QueryParamState>,
    Form(form): Form<PoliticalGroupForm>,
) -> Result<Response, AppError> {
    let steps = PoliticalGroupSteps::new(&store.snapshot(), query.is_initial())?;

    match form.validate_update(&political_group) {
        Err(form_data) => Ok(HtmlTemplate(
            PoliticalGroupUpdateTemplate {
                form: form_data,
                steps,
            },
            context,
        )
        .into_response()),
        Ok(political_group) => {
            political_group.update(&store).await?;

            Ok(query.redirect_or_preserving_initial(NameAuthorisation::list_path()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppError, Context, Form, PgStore, QueryParamState,
        structs::{
            common::{Appellation, PreviousElectionResults},
            name_authorisations::NameAuthorisationId,
        },
        test_utils::{
            response_body_string, sample_name_authorisation, sample_political_group_form,
        },
    };
    use axum::{
        http::{StatusCode, header},
        response::IntoResponse,
    };
    use axum_extra::routing::TypedPath;

    #[tokio::test]
    async fn update_political_group_renders_existing_data() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let political_group = store.snapshot().political_group().clone();

        let response = update_political_group(
            PoliticalGroupUpdatePath {},
            Context::new_test_without_db(),
            store,
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
        let political_group = store.snapshot().political_group().clone();

        let response = update_political_group(
            PoliticalGroupUpdatePath {},
            context,
            store,
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
        let political_group = store.snapshot().political_group().clone();

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

        let updated = store.snapshot().political_group().clone();
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
            store.snapshot().political_group().clone(),
            store,
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
