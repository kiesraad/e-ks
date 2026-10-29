use crate::structs::name_authorisations::NameAuthorisation;
use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use super::NameAuthorisationCreatePath;
use crate::{
    AppError, Context, CsbMainStore, Form, HtmlTemplate, Overlay, PgStore, QueryParamState,
    filters, form::FormData, name_authorisations::NameAuthorisationForm,
};

#[derive(Template)]
#[template(path = "pg/name_authorisations/pages/create.html")]
struct NameAuthorisationCreateTemplate {
    form: FormData<NameAuthorisationForm>,
    overlay: Overlay,
}

pub async fn create_name_authorisation(
    _: NameAuthorisationCreatePath,
    context: Context,
    store: PgStore,
    main_store: CsbMainStore,
    Query(query): Query<QueryParamState>,
) -> Result<impl IntoResponse, AppError> {
    // Start from the statutory name of the registration the group is bound to.
    let legal_name = store
        .snapshot()
        .get_political_group()
        .registration_id()
        .and_then(|id| main_store.get_registered_political_group(id).ok())
        .map(|registration| registration.legal_name.to_string())
        .unwrap_or_default();
    Ok(HtmlTemplate(
        NameAuthorisationCreateTemplate {
            form: FormData::new_with_data(NameAuthorisationForm {
                legal_name,
                ..Default::default()
            }),
            overlay: Overlay::new_create(&query),
        },
        context,
    ))
}

pub async fn create_name_authorisation_submit(
    _: NameAuthorisationCreatePath,
    context: Context,
    store: PgStore,
    Query(query): Query<QueryParamState>,
    Form(form): Form<NameAuthorisationForm>,
) -> Result<Response, AppError> {
    match form.validate_create() {
        Err(form_data) => Ok(HtmlTemplate(
            NameAuthorisationCreateTemplate {
                form: form_data,
                overlay: Overlay::new_create(&query),
            },
            context,
        )
        .into_response()),
        Ok(authorisation) => {
            authorisation.create(&store).await?;

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
            csb::sample_registered_political_group, list_designation::ListDesignation,
            political_groups::PoliticalGroup,
        },
        test_utils::{response_body_string, sample_name_authorisation_form},
    };
    use axum::{
        extract::Query,
        http::{StatusCode, header},
        response::IntoResponse,
    };
    use axum_extra::routing::TypedPath;

    /// `body` with the line breaks between attributes collapsed to one space.
    fn single_spaced(body: &str) -> String {
        body.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    #[tokio::test]
    async fn create_name_authorisation_renders_csrf_field() -> Result<(), AppError> {
        let response = create_name_authorisation(
            NameAuthorisationCreatePath {},
            Context::new_test_without_db(),
            PgStore::new_for_test(),
            CsbMainStore::new_for_test(),
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert_eq!(
            // One for: logout, session-expiry warning, language selection and create name authorisation
            body.matches(r#"input type="hidden" name="csrf_token""#)
                .count(),
            4,
        );
        // Not registered: the statutory name starts empty.
        assert!(single_spaced(&body).contains("name=\"legal_name\" value=\"\""));

        Ok(())
    }

    #[tokio::test]
    async fn create_name_authorisation_starts_from_the_registered_legal_name()
    -> Result<(), AppError> {
        let registration = sample_registered_political_group("De Partij", 100, 3);
        let main_store = CsbMainStore::new_for_test();
        main_store
            .update(
                CsbMainAction::CreateRegisteredPoliticalGroup(registration.clone())
                    .by(CsbUser::new_test()),
            )
            .await?;
        let store = PgStore::new_for_test();
        PoliticalGroup {
            list_designation: Some(ListDesignation::Standalone {
                registration: Some(registration.id),
            }),
            ..store.snapshot().get_political_group().clone()
        }
        .update(&store)
        .await?;

        let response = create_name_authorisation(
            NameAuthorisationCreatePath {},
            Context::new_test_without_db(),
            store,
            main_store,
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        let body = response_body_string(response).await;
        assert!(
            single_spaced(&body).contains("name=\"legal_name\" value=\"Vereniging De Partij\"")
        );

        Ok(())
    }

    #[tokio::test]
    async fn submit_label_is_save_when_returning_to_finalise() -> Result<(), AppError> {
        async fn submit_label(query: QueryParamState) -> String {
            let response = create_name_authorisation(
                NameAuthorisationCreatePath {},
                Context::new_test_without_db(),
                Query(query),
            )
            .await
            .unwrap()
            .into_response();
            let body = response_body_string(response).await;
            let button = body.split(r#"value="save">"#).nth(1).unwrap();
            button.split("</button>").next().unwrap().trim().to_string()
        }

        assert_eq!(
            submit_label(QueryParamState::default()).await,
            "Save and close"
        );
        assert_eq!(
            submit_label(QueryParamState::redirect_to(
                crate::finalise::FinalisePath.to_string()
            ))
            .await,
            "Save"
        );

        Ok(())
    }

    #[tokio::test]
    async fn create_name_authorisation_persists_and_redirects() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        let context = Context::new_test_without_db();
        let form = sample_name_authorisation_form();

        let response = create_name_authorisation_submit(
            NameAuthorisationCreatePath {},
            context,
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
        let data = store.snapshot();
        let authorisations = data.get_name_authorisations();
        assert_eq!(authorisations.len(), 1);
        assert_eq!(
            location,
            NameAuthorisation::list_path()
                .with_query_params(QueryParamState::success())
                .to_string()
        );

        Ok(())
    }

    #[tokio::test]
    async fn create_name_authorisation_invalid_form_renders_template() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        let context = Context::new_test_without_db();
        let mut form = sample_name_authorisation_form();
        form.name.last_name = " ".to_string();

        let response = create_name_authorisation_submit(
            NameAuthorisationCreatePath {},
            context,
            store,
            Query(QueryParamState::default()),
            Form(form),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("This field must not be empty."));

        Ok(())
    }
}
