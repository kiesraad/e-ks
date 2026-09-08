use askama::Template;
use axum::response::{IntoResponse, Response};

use crate::{
    AppError, Context, CsbContext, HtmlTemplate,
    csb::examination::{
        extractors::{CsbPoliticalGroup, CsbPoliticalGroups},
        paths::CsbFinishExaminationPath,
    },
    filters,
};

#[derive(Template)]
#[template(path = "csb/examination/pages/finish_examination.html")]
struct CsbFinishExaminationTemplate {
    political_groups: Vec<CsbPoliticalGroup>,
}

pub async fn finish(
    _: CsbFinishExaminationPath,
    context: CsbContext,
    CsbPoliticalGroups(political_groups): CsbPoliticalGroups,
) -> Result<Response, AppError> {
    let finished_political_groups = political_groups
        .into_iter()
        .filter(|pg| pg.is_examination_finished)
        .collect();

    Ok(HtmlTemplate(
        CsbFinishExaminationTemplate {
            political_groups: finished_political_groups,
        },
        context,
    )
    .into_response())
}
