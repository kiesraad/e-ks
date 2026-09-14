use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Redirect, Response},
};
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
    Query(query): Query<QueryParamState>,
) -> Result<Response, AppError> {
    Ok(HtmlTemplate(
        CsbObjectionTemplate {
            objection: None,
            close_action: CsbFinalisePath::PATH.to_owned(),
            overlay: Overlay::new(&query),
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
    Query(query): Query<QueryParamState>,
    Form(form): Form<ObjectionForm>,
) -> Result<Response, AppError> {
    match form.validate_create() {
        Err(form) => Ok(HtmlTemplate(
            CsbObjectionTemplate {
                objection: None,
                close_action: CsbFinalisePath::PATH.to_owned(),
                overlay: Overlay::new(&query),
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
    Query(query): Query<QueryParamState>,
) -> Result<Response, AppError> {
    Ok(HtmlTemplate(
        CsbObjectionTemplate {
            objection: main_store.get_objection(id),
            close_action: CsbFinalisePath::PATH.to_owned(),
            overlay: Overlay::new(&query),
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
    Query(query): Query<QueryParamState>,
    Form(form): Form<ObjectionForm>,
) -> Result<Response, AppError> {
    dbg!("update");
    let objection = main_store
        .get_objection(id)
        .ok_or(AppError::GenericNotFound)?;
    match form.validate_update(&objection) {
        Err(form) => Ok(HtmlTemplate(
            CsbObjectionTemplate {
                objection: Some(objection),
                close_action: CsbFinalisePath::PATH.to_owned(),
                overlay: Overlay::new(&query),
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
