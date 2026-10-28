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
    political_groups::{PoliticalGroupStep, PoliticalGroupSteps},
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
    let steps = PoliticalGroupSteps::new(
        &store,
        query.is_initial(),
        PoliticalGroupStep::ListDesignation,
    )?;
    Ok(HtmlTemplate(
        ListDesignationUpdateTemplate {
            steps,
            form: FormData::new_with_data(political_group.list_designation_kind().into()),
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
    let steps = PoliticalGroupSteps::new(
        &store,
        query.is_initial(),
        PoliticalGroupStep::ListDesignation,
    )?;

    match form.validate_update(&political_group.list_designation_kind().into()) {
        Err(form_data) => Ok(HtmlTemplate(
            ListDesignationUpdateTemplate {
                form: form_data,
                steps,
            },
            context,
        )
        .into_response()),
        Ok(target) => {
            let kind = target.list_designation_type;
            // The same list type keeps its registrations; another type starts
            // unbound.
            political_group.list_designation =
                Some(match political_group.list_designation.take() {
                    Some(current) if current.kind() == kind => current,
                    _ => kind.into(),
                });
            political_group.update(&store).await?;

            if political_group.is_blank() {
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
        AppError, Context, Form, PgStore, QueryParamState,
        structs::csb::RegisteredPoliticalGroupId, test_utils::response_body_string,
    };
    use axum::{
        http::{StatusCode, header},
        response::IntoResponse,
    };
    use axum_extra::routing::TypedPath;

    #[tokio::test]
    async fn update_list_designation_renders_existing_data() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let political_group = store.snapshot().get_political_group().clone();

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
        let political_group = store.snapshot().get_political_group().clone();

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
            store.snapshot().get_political_group().list_designation,
            Some(ListDesignation::default())
        );

        Ok(())
    }

    async fn submit_kind(store: &PgStore, kind: &str) -> Result<(), AppError> {
        let response = update_list_designation_submit(
            ListDesignationUpdatePath {},
            Context::new_test_without_db(),
            store.clone(),
            store.snapshot().get_political_group().clone(),
            Query(QueryParamState::default()),
            Form(ListDesignationForm {
                list_designation_type: kind.to_string(),
            }),
        )
        .await?;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        Ok(())
    }

    #[tokio::test]
    async fn resubmitting_the_same_kind_keeps_the_binding() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let bound = ListDesignation::Standalone {
            registration: Some(RegisteredPoliticalGroupId::new()),
        };
        PoliticalGroup {
            list_designation: Some(bound.clone()),
            ..store.snapshot().get_political_group().clone()
        }
        .update(&store)
        .await?;

        submit_kind(&store, "standalone").await?;
        assert_eq!(
            store.snapshot().get_political_group().list_designation,
            Some(bound)
        );

        Ok(())
    }

    #[tokio::test]
    async fn changing_the_kind_drops_the_binding() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        PoliticalGroup {
            list_designation: Some(ListDesignation::Standalone {
                registration: Some(RegisteredPoliticalGroupId::new()),
            }),
            ..store.snapshot().get_political_group().clone()
        }
        .update(&store)
        .await?;

        submit_kind(&store, "combined").await?;
        assert_eq!(
            store.snapshot().get_political_group().list_designation,
            Some(ListDesignation::Combined {
                registrations: Vec::new()
            })
        );

        submit_kind(&store, "standalone").await?;
        assert_eq!(
            store.snapshot().get_political_group().list_designation,
            Some(ListDesignation::Standalone { registration: None })
        );

        Ok(())
    }

    #[tokio::test]
    async fn update_list_designation_blank_redirects_to_list_submitter() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let political_group = store.snapshot().get_political_group().clone();

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
            store.snapshot().get_political_group().list_designation,
            Some(ListDesignation::Blank)
        );

        Ok(())
    }
}
