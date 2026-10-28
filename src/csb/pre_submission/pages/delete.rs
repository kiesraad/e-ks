use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Redirect, Response},
};

use crate::{
    AppError, Context, CsbAction, CsbContext, HtmlTemplate, Overlay, QueryParamState,
    csb::pre_submission::{
        CsbPreSubmissionOverviewPath,
        extractors::{PreSubmissionGroup, PreSubmissionStore},
        paths::CsbPreSubmissionDeleteGroupPath,
    },
    filters,
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
    PreSubmissionStore(csb_store): PreSubmissionStore,
) -> Result<Response, AppError> {
    csb_store.update(CsbAction::Delete).await?;
    Ok(Redirect::to(&CsbPreSubmissionOverviewPath.to_string()).into_response())
}
#[cfg(test)]
mod tests {
    use reqwest::StatusCode;

    use crate::{
        CsbStore,
        test_utils::{response_body_string, sample_political_group},
    };

    use super::*;

    /// A store holding the sample group with one candidate.
    fn store_with_appellation(appellation: &str) -> PreSubmissionStore {
        let store = CsbStore::new_for_test();
        let mut pg = sample_political_group();
        pg.appellation = Some(appellation.parse().expect("valid appellation"));
        store.set_political_group(pg);
        PreSubmissionStore(store)
    }

    #[tokio::test]
    async fn delete_overlay_mentions_appellation() {
        let appellation = "Test Partij";
        let store = store_with_appellation(appellation);
        let group = PreSubmissionGroup::from_store(&store);

        let response = delete(
            group.delete_path(),
            CsbContext::new_test(),
            Query::default(),
            store,
        )
        .await
        .expect("valid response");

        let body = response_body_string(response).await;

        assert!(body.contains(&format!("“{appellation}“")))
    }

    #[tokio::test]
    async fn submit_delete_marks_group_as_deleted() {
        let store = store_with_appellation("Test Partij");
        let group = PreSubmissionGroup::from_store(&store);
        assert!(!store.0.snapshot().is_deleted());

        let response = submit_delete(group.delete_path(), PreSubmissionStore(store.clone()))
            .await
            .expect("successful delete submission");

        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        assert!(store.0.snapshot().is_deleted());
    }
}
