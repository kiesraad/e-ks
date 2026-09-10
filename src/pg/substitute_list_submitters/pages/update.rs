use crate::structs::list_submitters::{ListSubmitter, ListSubmitterData};
use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, Context, Form, HtmlTemplate, Overlay, PgStore, QueryParamState, filters,
    form::FormData,
    list_submitters::ListSubmitterForm,
    structs::common::{HasSeverity, Problematic},
};

use super::SubstituteSubmitterUpdatePath;

#[derive(Template)]
#[template(path = "pg/substitute_list_submitters/pages/update.html")]
struct SubstituteSubmitterUpdateTemplate {
    substitute_submitter: ListSubmitter,
    form: FormData<ListSubmitterForm>,
    address_unknown: bool,
    overlay: Overlay,
}

pub async fn update_substitute_submitter(
    _: SubstituteSubmitterUpdatePath,
    context: Context,
    substitute_submitter: ListSubmitter,
    Query(query): Query<QueryParamState>,
) -> Result<Response, AppError> {
    Ok(HtmlTemplate(
        SubstituteSubmitterUpdateTemplate {
            form: FormData::new_with_data(substitute_submitter.clone().into()),
            address_unknown: substitute_submitter.address.is_unknown(),
            substitute_submitter,
            overlay: Overlay::new(&query),
        },
        context,
    )
    .into_response())
}

pub async fn update_substitute_submitter_submit(
    _: SubstituteSubmitterUpdatePath,
    context: Context,
    substitute_submitter: ListSubmitter,
    store: PgStore,
    Query(query): Query<QueryParamState>,
    Form(form): Form<ListSubmitterForm>,
) -> Result<Response, AppError> {
    match form.validate_update_with_checks(&ListSubmitterData::from(substitute_submitter.clone())) {
        Err(form_data) => Ok(HtmlTemplate(
            SubstituteSubmitterUpdateTemplate {
                address_unknown: substitute_submitter.address.is_unknown(),
                substitute_submitter,
                form: *form_data,
                overlay: Overlay::new(&query),
            },
            context,
        )
        .into_response()),
        Ok(substitute_submitter_data) => {
            let updated = substitute_submitter.updated_from(substitute_submitter_data);
            updated.update_substitute(&store).await?;

            Ok(query.redirect_or_preserving_initial(ListSubmitter::view_path()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        QueryParamState,
        structs::list_submitters::ListSubmitterId,
        test_utils::{sample_list_submitter, sample_list_submitter_form},
    };
    use axum::{
        http::{StatusCode, header},
        response::IntoResponse,
    };
    use axum_extra::routing::TypedPath;

    use crate::{AppError, Context, PgStore, test_utils::response_body_string};

    #[tokio::test]
    async fn update_substitute_submitter_renders_existing_submitter() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        let sub_submitter_id = ListSubmitterId::new();
        let substitute_submitter = sample_list_submitter(sub_submitter_id);
        substitute_submitter.create_substitute(&store).await?;

        let response = update_substitute_submitter(
            SubstituteSubmitterUpdatePath { sub_submitter_id },
            Context::new_test_without_db(),
            substitute_submitter.clone(),
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains(substitute_submitter.name.last_name.as_str()));

        Ok(())
    }

    #[tokio::test]
    async fn update_substitute_submitter_persists_and_redirects() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        let sub_submitter_id = ListSubmitterId::new();
        let substitute_submitter = sample_list_submitter(sub_submitter_id);
        substitute_submitter.create_substitute(&store).await?;

        let context = Context::new_test_without_db();
        let mut form = sample_list_submitter_form();
        form.name.last_name = "Updated".to_string();

        let response = update_substitute_submitter_submit(
            SubstituteSubmitterUpdatePath { sub_submitter_id },
            context,
            substitute_submitter.clone(),
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
            ListSubmitter::view_path()
                .with_query_params(QueryParamState::success())
                .to_string()
        );

        let updated = store.get_substitute_submitter(sub_submitter_id)?;
        assert_eq!(updated.name.last_name.to_string(), "Updated");

        Ok(())
    }

    #[tokio::test]
    async fn update_substitute_submitter_invalid_form_renders_template() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        let sub_submitter_id = ListSubmitterId::new();
        let substitute_submitter = sample_list_submitter(sub_submitter_id);
        substitute_submitter.create_substitute(&store).await?;

        let context = Context::new_test_without_db();
        let mut form = sample_list_submitter_form();
        form.name.last_name = " ".to_string();

        let response = update_substitute_submitter_submit(
            SubstituteSubmitterUpdatePath { sub_submitter_id },
            context,
            substitute_submitter.clone(),
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
