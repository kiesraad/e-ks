use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, Context, CsbContext, CsbMainAction, CsbMainStore, Form, HtmlTemplate, Overlay,
    QueryParamState,
    csb::finalise::{
        CsbFinalisePath,
        forms::ObjectionForm,
        paths::{CsbAddObjectionPath, CsbDeleteObjectionPath, CsbUpdateObjectionPath},
    },
    filters,
    form::FormData,
    redirect_success,
    structs::csb::Objection,
};

#[derive(Template)]
#[template(path = "csb/finalise/pages/objection.html")]
struct CsbObjectionTemplate {
    /// The objection being edited; `None` when adding a new one.
    objection: Option<Objection>,
    close_action: String,
    overlay: Overlay,
    form: FormData<ObjectionForm>,
}

fn render(
    context: CsbContext,
    query: &QueryParamState,
    form: FormData<ObjectionForm>,
    objection: Option<Objection>,
) -> Response {
    HtmlTemplate(
        CsbObjectionTemplate {
            objection,
            close_action: CsbFinalisePath.to_string(),
            overlay: Overlay::new(query),
            form,
        },
        context,
    )
    .into_response()
}

pub async fn add_objection(
    _: CsbAddObjectionPath,
    context: CsbContext,
    Query(query): Query<QueryParamState>,
) -> Result<Response, AppError> {
    Ok(render(context, &query, FormData::new(), None))
}

pub async fn add_objection_submit(
    _: CsbAddObjectionPath,
    context: CsbContext,
    main_store: CsbMainStore,
    Query(query): Query<QueryParamState>,
    Form(form): Form<ObjectionForm>,
) -> Result<Response, AppError> {
    let objection = match form.validate_create() {
        Ok(objection) => objection,
        Err(form) => return Ok(render(context, &query, form, None)),
    };
    main_store
        .update(CsbMainAction::AddObjection(objection).by(context.user()?))
        .await?;
    Ok(redirect_success(CsbFinalisePath))
}

pub async fn update_objection(
    CsbUpdateObjectionPath { id }: CsbUpdateObjectionPath,
    context: CsbContext,
    main_store: CsbMainStore,
    Query(query): Query<QueryParamState>,
) -> Result<Response, AppError> {
    let objection = main_store.get_objection(id)?;
    Ok(render(
        context,
        &query,
        FormData::new_with_data(objection.clone().into()),
        Some(objection),
    ))
}

pub async fn update_objection_submit(
    CsbUpdateObjectionPath { id }: CsbUpdateObjectionPath,
    context: CsbContext,
    main_store: CsbMainStore,
    Query(query): Query<QueryParamState>,
    Form(form): Form<ObjectionForm>,
) -> Result<Response, AppError> {
    let current = main_store.get_objection(id)?;
    let objection = match form.validate_update(&current) {
        Ok(objection) => objection,
        Err(form) => return Ok(render(context, &query, form, Some(current))),
    };
    main_store
        .update(CsbMainAction::UpdateObjection(objection).by(context.user()?))
        .await?;
    Ok(redirect_success(CsbFinalisePath))
}

pub async fn delete_objection(
    CsbDeleteObjectionPath { id }: CsbDeleteObjectionPath,
    context: CsbContext,
    main_store: CsbMainStore,
) -> Result<Response, AppError> {
    // Deleting an objection that is already gone is a stale form, not a change.
    main_store.get_objection(id)?;
    main_store
        .update(CsbMainAction::DeleteObjection(id).by(context.user()?))
        .await?;
    Ok(redirect_success(CsbFinalisePath))
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Response};
    use axum_extra::routing::TypedPath;
    use reqwest::{StatusCode, header};

    use crate::{CsbUser, structs::csb::ObjectionId, test_utils::response_body_string};

    use super::*;

    async fn add_objection_to_store(store: &CsbMainStore, text: &str) -> ObjectionId {
        let id = ObjectionId::new();
        store
            .update(
                CsbMainAction::AddObjection(Objection {
                    id,
                    objection_text: text.parse().expect("Valid objection text"),
                })
                .by(CsbUser::Developer),
            )
            .await
            .expect("Add objection");
        id
    }

    fn form(text: &str) -> Form<ObjectionForm> {
        Form(ObjectionForm {
            objection_text: text.to_string(),
        })
    }

    fn query() -> Query<QueryParamState> {
        Query(QueryParamState::default())
    }

    async fn assert_redirect(response: Response<Body>) {
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let location = response
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .expect("Location header value");
        assert!(location.contains(CsbFinalisePath::PATH));
        assert!(location.contains("success=true"));
    }

    #[tokio::test]
    async fn add_objection_renders_page() {
        let response = add_objection(CsbAddObjectionPath, CsbContext::new_test(), query())
            .await
            .expect("Page render response");

        let body = response_body_string(response).await;

        assert!(body.contains("Objection text"));
        assert!(body.contains("Add objection"));
        assert!(body.contains("<textarea"))
    }

    #[tokio::test]
    async fn add_objection_submit_redirects() {
        let main_store = CsbMainStore::new_for_test();

        let response = add_objection_submit(
            CsbAddObjectionPath,
            CsbContext::new_test(),
            main_store.clone(),
            query(),
            form("test bezwaar tekst"),
        )
        .await
        .expect("Form submit");

        assert_redirect(response).await;

        let objections = main_store.get_all_objections();
        assert_eq!(objections.len(), 1);
        assert_eq!(
            objections[0].objection_text.to_string(),
            "test bezwaar tekst"
        );
    }

    #[tokio::test]
    async fn add_objection_submit_trims_and_normalises_line_endings() {
        let main_store = CsbMainStore::new_for_test();

        let response = add_objection_submit(
            CsbAddObjectionPath,
            CsbContext::new_test(),
            main_store.clone(),
            query(),
            form("  eerste regel\r\ntweede regel \r\n"),
        )
        .await
        .expect("Form submit");

        assert_redirect(response).await;

        let objections = main_store.get_all_objections();
        assert_eq!(
            objections[0].objection_text.to_string(),
            "eerste regel\ntweede regel"
        );
    }

    #[tokio::test]
    async fn add_objection_invalid_submit_returns_form() {
        let main_store = CsbMainStore::new_for_test();

        for text in ["", "  \r\n "] {
            let response = add_objection_submit(
                CsbAddObjectionPath,
                CsbContext::new_test(),
                main_store.clone(),
                query(),
                form(text),
            )
            .await
            .expect("Form submit");

            assert_eq!(response.status(), StatusCode::OK);

            let body = response_body_string(response).await;
            assert!(body.contains("This field must not be empty."));
        }

        assert!(main_store.get_all_objections().is_empty());
    }

    #[tokio::test]
    async fn update_objection_renders_page() {
        let main_store = CsbMainStore::new_for_test();
        let id = add_objection_to_store(
            &main_store,
            "objection text that should appear in the textarea",
        )
        .await;

        let response = update_objection(
            CsbUpdateObjectionPath { id },
            CsbContext::new_test(),
            main_store,
            query(),
        )
        .await
        .expect("Page render response");

        let body = response_body_string(response).await;

        assert!(body.contains("Objection text"));
        assert!(body.contains("Update objection"));
        assert!(body.contains("<textarea"));
        assert!(body.contains("objection text that should appear in the textarea</textarea>"));
        assert!(body.contains(&CsbDeleteObjectionPath { id }.to_string()));
    }

    #[tokio::test]
    async fn update_of_an_unknown_objection_is_not_found() {
        let result = update_objection(
            CsbUpdateObjectionPath {
                id: ObjectionId::new(),
            },
            CsbContext::new_test(),
            CsbMainStore::new_for_test(),
            query(),
        )
        .await;

        assert!(matches!(result, Err(AppError::GenericNotFound)));
    }

    #[tokio::test]
    async fn update_objection_submit_redirects() {
        let main_store = CsbMainStore::new_for_test();
        let id = add_objection_to_store(&main_store, "old_text").await;

        let response = update_objection_submit(
            CsbUpdateObjectionPath { id },
            CsbContext::new_test(),
            main_store.clone(),
            query(),
            form("new text"),
        )
        .await
        .expect("Form submit");

        assert_redirect(response).await;

        let objections = main_store.get_all_objections();
        assert_eq!(objections.len(), 1);
        assert_eq!(objections[0].objection_text.to_string(), "new text");
        assert_eq!(objections[0].id, id);
    }

    #[tokio::test]
    async fn update_objection_invalid_submit_returns_form() {
        let main_store = CsbMainStore::new_for_test();
        let id = add_objection_to_store(&main_store, "old text").await;

        let response = update_objection_submit(
            CsbUpdateObjectionPath { id },
            CsbContext::new_test(),
            main_store.clone(),
            query(),
            form(""),
        )
        .await
        .expect("Form submit");

        assert_eq!(response.status(), StatusCode::OK);

        let body = response_body_string(response).await;
        assert!(body.contains("This field must not be empty."));

        let objections = main_store.get_all_objections();
        assert_eq!(objections.len(), 1);
        assert_eq!(objections[0].objection_text.to_string(), "old text");
        assert_eq!(objections[0].id, id);
    }

    #[tokio::test]
    async fn delete_objection_deletes() {
        let main_store = CsbMainStore::new_for_test();
        let id = add_objection_to_store(&main_store, "objection text").await;

        let response = delete_objection(
            CsbDeleteObjectionPath { id },
            CsbContext::new_test(),
            main_store.clone(),
        )
        .await
        .expect("Delete objection response");

        assert_redirect(response).await;

        assert!(main_store.get_all_objections().is_empty());
    }

    #[tokio::test]
    async fn delete_of_an_unknown_objection_is_not_found() {
        let result = delete_objection(
            CsbDeleteObjectionPath {
                id: ObjectionId::new(),
            },
            CsbContext::new_test(),
            CsbMainStore::new_for_test(),
        )
        .await;

        assert!(matches!(result, Err(AppError::GenericNotFound)));
    }
}
