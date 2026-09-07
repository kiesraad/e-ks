use askama::Template;
use axum::response::{IntoResponse, Response};

use crate::{
    AppError, Context, CsbContext, HtmlTemplate, csb::examination::paths::CsbFinishExaminationPath,
    filters,
};

#[derive(Template)]
#[template(path = "csb/examination/pages/finish_examination.html")]
struct CsbFinishExaminationTemplate {
    name: String,
}

pub async fn finish(
    _: CsbFinishExaminationPath,
    context: CsbContext,
) -> Result<Response, AppError> {
    Ok(HtmlTemplate(
        CsbFinishExaminationTemplate {
            name: "World".to_string(),
        },
        context,
    )
    .into_response())
}
