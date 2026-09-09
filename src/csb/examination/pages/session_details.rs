use askama::Template;
use axum::response::{IntoResponse, Response};

use crate::{
    AppError, Context, CsbContext, HtmlTemplate, csb::examination::CsbSessionDetailsPath, filters,
};

#[derive(Template)]
#[template(path = "csb/examination/pages/session_details.html")]
struct CsbSessionDetailsTemplate;

pub async fn session_details(
    _: CsbSessionDetailsPath,
    context: CsbContext,
) -> Result<Response, AppError> {
    Ok(HtmlTemplate(CsbSessionDetailsTemplate, context).into_response())
}

pub async fn session_details_submit() {}
