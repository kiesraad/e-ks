use askama::Template;
use axum::response::{IntoResponse, Response};

use crate::{
    AppError, Context, CsbContext, ElectionConfig, HtmlTemplate,
    csb::examination::CsbHearingDetailsPath, filters,
};

#[derive(Template)]
#[template(path = "csb/examination/pages/hearing_details.html")]
struct CsbHearingDetailsTemplate {
    election_config: ElectionConfig,
}

pub async fn hearing_details(
    _: CsbHearingDetailsPath,
    context: CsbContext,
) -> Result<Response, AppError> {
    Ok(HtmlTemplate(
        CsbHearingDetailsTemplate {
            election_config: context.election,
        },
        context,
    )
    .into_response())
}

pub async fn hearing_details_submit(_: CsbHearingDetailsPath) {}
// pub async fn hearing_details_submit(_: CsbHearingDetailsPath) -> Result<Response, AppError> {
//     Ok
// }
