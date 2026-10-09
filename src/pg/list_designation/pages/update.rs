use crate::structs::{list_designation::ListDesignation, political_groups::PoliticalGroup};
use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, Context, Form, HtmlTemplate, PgStore, QueryParamState, filters,
    form::FormData,
    list_designation::{
        ListDesignationUpdatePath, forms::list_designation_form::ListDesignationForm,
    },
    political_groups::PoliticalGroupSteps,
    structs::{list_submitters::ListSubmitter, name_authorisations::NameAuthorisation},
};

#[derive(Template)]
#[template(path = "pg/list_designation/pages/update.html")]
struct ListDesignationUpdateTemplate {
    form: FormData<ListDesignationForm>,
    steps: PoliticalGroupSteps,
}

pub async fn update_list_designation(
    _: ListDesignationUpdatePath,
    context: Context,
    store: PgStore,
    political_group: PoliticalGroup,
    Query(query): Query<QueryParamState>,
) -> Result<Response, AppError> {
    let steps = PoliticalGroupSteps::new(&store.snapshot(), query.is_initial())?;
    Ok(HtmlTemplate(
        ListDesignationUpdateTemplate {
            steps,
            form: FormData::new_with_data(political_group.list_designation.into()),
        },
        context,
    )
    .into_response())
}

pub async fn update_list_designation_submit(
    _: ListDesignationUpdatePath,
    context: Context,
    store: PgStore,
    mut political_group: PoliticalGroup,
    Query(query): Query<QueryParamState>,
    Form(form): Form<ListDesignationForm>,
) -> Result<Response, AppError> {
    let steps = PoliticalGroupSteps::new(&store.snapshot(), query.is_initial())?;

    match form.validate_update(&political_group.list_designation.into()) {
        Err(form_data) => Ok(HtmlTemplate(
            ListDesignationUpdateTemplate {
                form: form_data,
                steps,
            },
            context,
        )
        .into_response()),
        Ok(list_designation) => {
            political_group.list_designation = Some(list_designation.list_designation_type);
            political_group.update(&store).await?;

            if political_group.list_designation == Some(ListDesignation::Blank) {
                Ok(query.redirect_or_preserving_initial(ListSubmitter::view_path()))
            } else {
                Ok(query.redirect_or_preserving_initial(PoliticalGroup::update_path()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppError, Context, Form, PgStore, QueryParamState, test_utils::response_body_string,
    };
    use axum::{
        http::{StatusCode, header},
        response::IntoResponse,
    };
    use axum_extra::routing::TypedPath;

    #[tokio::test]
    async fn update_list_designation_renders_existing_data() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let political_group = store.snapshot().political_group().clone();

        let response = update_list_designation(
            ListDesignationUpdatePath {},
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
        assert!(body.contains("id=\"standalone\""));

        Ok(())
    }

    #[tokio::test]
    async fn update_list_designation_persists_and_redirects_to_basic_info() -> Result<(), AppError>
    {
        let store = PgStore::new_for_test();
        let political_group = store.snapshot().political_group().clone();

        let context = Context::new_test_without_db();
        let form = ListDesignationForm {
            list_designation_type: "standalone".to_string(),
        };

        let response = update_list_designation_submit(
            ListDesignationUpdatePath {},
            context,
            store.clone(),
            political_group,
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
            PoliticalGroup::update_path()
                .with_query_params(QueryParamState::success())
                .to_string()
        );
        assert_eq!(
            store.snapshot().political_group().clone().list_designation,
            Some(ListDesignation::Standalone)
        );

        Ok(())
    }

    #[tokio::test]
    async fn update_list_designation_blank_redirects_to_list_submitter() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let political_group = store.snapshot().political_group().clone();

        let context = Context::new_test_without_db();
        let form = ListDesignationForm {
            list_designation_type: "blank".to_string(),
        };

        let response = update_list_designation_submit(
            ListDesignationUpdatePath {},
            context,
            store.clone(),
            political_group,
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
        assert_eq!(
            store.snapshot().political_group().clone().list_designation,
            Some(ListDesignation::Blank)
        );

        Ok(())
    }
}
