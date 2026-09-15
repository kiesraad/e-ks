use askama::Template;
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::routing::TypedPath;

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
    structs::csb::Objection,
};

#[derive(Template)]
#[template(path = "csb/finalise/pages/objection.html")]
struct CsbObjectionTemplate {
    objection: Option<Objection>,
    close_action: String,
    overlay: Overlay,
    form: FormData<ObjectionForm>,
}

pub async fn add_objection(
    _: CsbAddObjectionPath,
    context: CsbContext,
) -> Result<Response, AppError> {
    Ok(HtmlTemplate(
        CsbObjectionTemplate {
            objection: None,
            close_action: CsbFinalisePath::PATH.to_owned(),
            overlay: Overlay::new(&QueryParamState::default()),

            form: FormData::new(),
        },
        context,
    )
    .into_response())
}

pub async fn add_objection_submit(
    _: CsbAddObjectionPath,
    context: CsbContext,
    main_store: CsbMainStore,
    Form(form): Form<ObjectionForm>,
) -> Result<Response, AppError> {
    match form.validate_create() {
        Err(form) => Ok(HtmlTemplate(
            CsbObjectionTemplate {
                objection: None,
                close_action: CsbFinalisePath::PATH.to_owned(),
                overlay: Overlay::new(&QueryParamState::default()),

                form,
            },
            context,
        )
        .into_response()),
        Ok(objection) => {
            main_store
                .update(CsbMainAction::AddObjection(objection).by(context.user()?))
                .await?;
            Ok(Redirect::to(&Objection::after_success_submit_path().to_string()).into_response())
        }
    }
}

pub async fn update_objection(
    CsbUpdateObjectionPath { id }: CsbUpdateObjectionPath,
    main_store: CsbMainStore,
    context: CsbContext,
) -> Result<Response, AppError> {
    Ok(HtmlTemplate(
        CsbObjectionTemplate {
            objection: main_store.get_objection(id),
            close_action: CsbFinalisePath::PATH.to_owned(),
            overlay: Overlay::new(&QueryParamState::default()),

            form: FormData::new_with_data(
                main_store
                    .get_objection(id)
                    .ok_or(AppError::GenericNotFound)?
                    .into(),
            ),
        },
        context,
    )
    .into_response())
}

pub async fn update_objection_submit(
    CsbUpdateObjectionPath { id }: CsbUpdateObjectionPath,
    context: CsbContext,
    main_store: CsbMainStore,
    Form(form): Form<ObjectionForm>,
) -> Result<Response, AppError> {
    let objection = main_store
        .get_objection(id)
        .ok_or(AppError::GenericNotFound)?;
    match form.validate_update(&objection) {
        Err(form) => Ok(HtmlTemplate(
            CsbObjectionTemplate {
                objection: Some(objection),
                close_action: CsbFinalisePath::PATH.to_owned(),
                overlay: Overlay::new(&QueryParamState::default()),
                form,
            },
            context,
        )
        .into_response()),
        Ok(objection) => {
            main_store
                .update(CsbMainAction::UpdateObjection(objection).by(context.user()?))
                .await?;
            Ok(Redirect::to(&Objection::after_success_submit_path().to_string()).into_response())
        }
    }
}

pub async fn delete_objection(
    CsbDeleteObjectionPath { id }: CsbDeleteObjectionPath,
    context: CsbContext,
    main_store: CsbMainStore,
) -> Result<Response, AppError> {
    main_store
        .update(CsbMainAction::DeleteObjection(id).by(context.user()?))
        .await?;
    Ok(Redirect::to(&Objection::after_success_submit_path().to_string()).into_response())
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Response};
    use reqwest::{StatusCode, header};

    use crate::{CsbUser, structs::csb::ObjectionId, test_utils::response_body_string};

    use super::*;

    async fn add_objection_to_store(store: &CsbMainStore, text: &str) -> ObjectionId {
        let id = ObjectionId::new();
        store
            .update(
                CsbMainAction::AddObjection(Objection {
                    id,
                    objection_text: text.to_string(),
                })
                .by(CsbUser::Developer),
            )
            .await
            .expect("Add objection");
        id
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
        let response = add_objection(CsbAddObjectionPath, CsbContext::new_test())
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
        let form = ObjectionForm {
            objection_text: "test bezwaar tekst".to_string(),
        };

        let response = add_objection_submit(
            CsbAddObjectionPath,
            CsbContext::new_test(),
            main_store.clone(),
            Form(form),
        )
        .await
        .expect("Form submit");

        assert_redirect(response).await;

        let objections = main_store.get_all_objections();
        assert_eq!(objections.len(), 1);
        assert_eq!(objections[0].objection_text, "test bezwaar tekst");
    }

    #[tokio::test]
    async fn add_objection_invalid_submit_returns_form() {
        let main_store = CsbMainStore::new_for_test();
        let form = ObjectionForm {
            objection_text: String::new(),
        };

        let response = add_objection_submit(
            CsbAddObjectionPath,
            CsbContext::new_test(),
            main_store.clone(),
            Form(form),
        )
        .await
        .expect("Form submit");

        assert_eq!(response.status(), StatusCode::OK);

        let body = response_body_string(response).await;
        assert!(body.contains("This field must not be empty."));

        let objections = main_store.get_all_objections();
        assert!(objections.is_empty());
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
            main_store,
            CsbContext::new_test(),
        )
        .await
        .expect("Page render response");

        let body = response_body_string(response).await;

        assert!(body.contains("Objection text"));
        assert!(body.contains("Update objection"));
        assert!(body.contains("<textarea"));
        assert!(body.contains("objection text that should appear in the textarea</textarea>"));
    }

    #[tokio::test]
    async fn update_objection_submit_redirects() {
        let main_store = CsbMainStore::new_for_test();
        let id = add_objection_to_store(&main_store, "old_text").await;

        let form = ObjectionForm {
            objection_text: "new text".to_string(),
        };

        let response = update_objection_submit(
            CsbUpdateObjectionPath { id },
            CsbContext::new_test(),
            main_store.clone(),
            Form(form),
        )
        .await
        .expect("Form submit");

        assert_redirect(response).await;

        let objections = main_store.get_all_objections();
        assert_eq!(objections.len(), 1);
        assert_eq!(objections[0].objection_text, "new text");
        assert_eq!(objections[0].id, id);
    }

    #[tokio::test]
    async fn update_objection_invalid_submit_returns_form() {
        let main_store = CsbMainStore::new_for_test();
        let id = add_objection_to_store(&main_store, "old text").await;

        let form = ObjectionForm {
            objection_text: String::new(),
        };

        let response = update_objection_submit(
            CsbUpdateObjectionPath { id },
            CsbContext::new_test(),
            main_store.clone(),
            Form(form),
        )
        .await
        .expect("Form submit");

        assert_eq!(response.status(), StatusCode::OK);

        let body = response_body_string(response).await;
        assert!(body.contains("This field must not be empty."));

        let objections = main_store.get_all_objections();
        assert_eq!(objections.len(), 1);
        assert_eq!(objections[0].objection_text, "old text");
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
}
