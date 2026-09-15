use crate::structs::name_authorisations::NameAuthorisation;
use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use super::NameAuthorisationCreatePath;
use crate::{
    AppError, Context, Form, HtmlTemplate, Overlay, PgStore, QueryParamState, filters,
    form::FormData, name_authorisations::NameAuthorisationForm,
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
    Query(query): Query<QueryParamState>,
) -> Result<impl IntoResponse, AppError> {
    Ok(HtmlTemplate(
        NameAuthorisationCreateTemplate {
            form: FormData::new(),
            overlay: Overlay::new(&query),
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
                overlay: Overlay::new(&query),
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
        AppError, Context, Form, PgStore, QueryParamState,
        test_utils::{response_body_string, sample_name_authorisation_form},
    };
    use axum::{
        extract::Query,
        http::{StatusCode, header},
        response::IntoResponse,
    };
    use axum_extra::routing::TypedPath;

    #[tokio::test]
    async fn create_name_authorisation_renders_csrf_field() -> Result<(), AppError> {
        let response = create_name_authorisation(
            NameAuthorisationCreatePath {},
            Context::new_test_without_db(),
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = dbg!(response_body_string(response).await);
        assert!(body.contains("name=\"csrf_token\""));

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
        let authorisations = store.get_name_authorisations();
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
