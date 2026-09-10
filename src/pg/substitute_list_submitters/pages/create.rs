use crate::structs::list_submitters::ListSubmitter;
use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use super::SubstituteSubmitterCreatePath;
use crate::{
    AppError, Context, Form, HtmlTemplate, Overlay, PgStore, QueryParamState, filters,
    form::FormData, list_submitters::ListSubmitterForm,
};

#[derive(Template)]
#[template(path = "pg/substitute_list_submitters/pages/create.html")]
struct SubstituteSubmitterCreateTemplate {
    form: FormData<ListSubmitterForm>,
    overlay: Overlay,
}

pub async fn create_substitute_submitter(
    _: SubstituteSubmitterCreatePath,
    context: Context,
    Query(query): Query<QueryParamState>,
) -> Result<impl IntoResponse, AppError> {
    Ok(HtmlTemplate(
        SubstituteSubmitterCreateTemplate {
            form: FormData::new(),
            overlay: Overlay::new(&query),
        },
        context,
    ))
}

pub async fn create_substitute_submitter_submit(
    _: SubstituteSubmitterCreatePath,
    context: Context,
    store: PgStore,
    Query(query): Query<QueryParamState>,
    Form(form): Form<ListSubmitterForm>,
) -> Result<Response, AppError> {
    match form.validate_create_with_checks() {
        Err(form_data) => Ok(HtmlTemplate(
            SubstituteSubmitterCreateTemplate {
                form: *form_data,
                overlay: Overlay::new(&query),
            },
            context,
        )
        .into_response()),
        Ok(substitute_submitter_data) => {
            let mut substitute_submitter: ListSubmitter = substitute_submitter_data.into();
            substitute_submitter.address.update_is_known_in_bag();
            substitute_submitter.create_substitute(&store).await?;

            Ok(query.redirect_or_preserving_initial(ListSubmitter::view_path()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QueryParamState;
    use axum::{
        http::{StatusCode, header},
        response::IntoResponse,
    };

    use crate::{
        AppError, Context, PgStore,
        test_utils::{response_body_string, sample_list_submitter_form},
    };

    #[tokio::test]
    async fn create_substitute_submitter_renders_csrf_field() {
        let context = Context::new_test_without_db();

        let response = create_substitute_submitter(
            SubstituteSubmitterCreatePath {},
            context,
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("name=\"csrf_token\""));
    }

    #[tokio::test]
    async fn create_substitute_submitter_persists_and_redirects() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let context = Context::new_test_without_db();
        let form = sample_list_submitter_form();

        let response = create_substitute_submitter_submit(
            SubstituteSubmitterCreatePath {},
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
        let submitters = store.get_substitute_submitters();
        assert_eq!(submitters.len(), 1);
        assert_eq!(
            location,
            format!("{}?&success=true", ListSubmitter::view_path())
        );

        Ok(())
    }

    #[tokio::test]
    async fn create_substitute_submitter_invalid_form_renders_template() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        let context = Context::new_test_without_db();
        let mut form = sample_list_submitter_form();
        form.name.last_name = " ".to_string();

        let response = create_substitute_submitter_submit(
            SubstituteSubmitterCreatePath {},
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
