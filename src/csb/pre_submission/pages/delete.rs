use askama::Template;
use axum::{
    extract::Query, response::{IntoResponse, Redirect, Response},
};

use crate::{
    AppError, Context, CsbAction, CsbContext, HtmlTemplate, Overlay, QueryParamState, csb::pre_submission::{CsbPreSubmissionOverviewPath, extractors::{PreSubmissionGroup, PreSubmissionStore}, paths::CsbPreSubmissionDeleteGroupPath}, filters,
};

#[derive(Template)]
#[template(path = "csb/pre_submission/pages/delete.html")]
struct CsbPoliticalGroupDeleteTemplate {
    political_group: PreSubmissionGroup,
    overlay: Overlay,
    close_action: String,
}

pub async fn delete(
    _: CsbPreSubmissionDeleteGroupPath,
    context: CsbContext,
    Query(query): Query<QueryParamState>,
    store: PreSubmissionStore,
) -> Result<Response, AppError> {
    let political_group = PreSubmissionGroup::from_store(&store);
    let close_action = political_group.path().to_string();
    Ok(HtmlTemplate(
        CsbPoliticalGroupDeleteTemplate {
            close_action,
            political_group,
            overlay: Overlay::new_edit(&query),
        },
        context,
    )
    .into_response())
}

pub async fn submit_delete(
    _: CsbPreSubmissionDeleteGroupPath,
    PreSubmissionStore(csb_store): PreSubmissionStore
) -> Result<Response, AppError> {
    csb_store.update(CsbAction::Delete).await?;
    Ok(Redirect::to(&CsbPreSubmissionOverviewPath.to_string()).into_response())
}
