use askama::Template;
use axum::response::{IntoResponse, Response};

use crate::{
    AppError, Context, CsbContext, HtmlTemplate,
    csb::{
        examination::extractors::{CsbPoliticalGroup, CsbPoliticalGroups},
        recovery::paths::CsbRecoveryOverviewPath,
    },
    filters,
    structs::csb::CsbPhase,
};

#[derive(Template)]
#[template(path = "csb/recovery/pages/overview.html")]
struct CsbRecoveryOverviewTemplate {
    political_groups: Vec<CsbPoliticalGroup>,
}

/// The recovery overview: every political group with its assessment progress.
pub async fn overview(
    _: CsbRecoveryOverviewPath,
    context: CsbContext,
    CsbPoliticalGroups(political_groups): CsbPoliticalGroups,
) -> Result<Response, AppError> {
    let political_groups = political_groups
        .into_iter()
        .filter(|political_group| !political_group.is_deleted)
        .map(|political_group| political_group.with_mode(CsbPhase::Recovery))
        .collect();

    Ok(HtmlTemplate(CsbRecoveryOverviewTemplate { political_groups }, context).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    use crate::{
        StreamId,
        test_utils::{response_body_string, sample_political_group},
    };

    fn group(pending: usize, actionable: usize) -> CsbPoliticalGroup {
        CsbPoliticalGroup {
            political_group: sample_political_group(),
            stream_id: StreamId::new(),
            brp: crate::csb::examination::structs::BrpCheckState::NotChecked,
            mode: CsbPhase::Examination,
            is_examination_finished: false,
            is_deleted: false,
            restoration_count: 0,
            omission_count: actionable,
            pending_omission_count: pending,
            actionable_omission_count: actionable,
            first_candidate_name: None,
        }
    }

    #[tokio::test]
    async fn overview_lists_groups_with_recovery_links_and_progress() {
        let groups = CsbPoliticalGroups(vec![group(1, 3)]);
        let stream_id = groups.0[0].stream_id;

        let response = overview(CsbRecoveryOverviewPath {}, CsbContext::new_test(), groups)
            .await
            .unwrap()
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Kiesraad Demo"));
        // Rows link to the recovery detail page, not the examination page.
        assert!(body.contains(&format!("/csb/recovery/{stream_id}")));
        assert!(!body.contains("/csb/examination/"));
        // The progress column shows assessed vs. assessable omissions.
        assert!(body.contains("2 of 3 assessed"));
    }

    #[tokio::test]
    async fn overview_skips_deleted_groups() {
        let mut deleted = group(0, 0);
        deleted.is_deleted = true;
        let groups = CsbPoliticalGroups(vec![deleted]);

        let response = overview(CsbRecoveryOverviewPath {}, CsbContext::new_test(), groups)
            .await
            .unwrap()
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(!body.contains("Kiesraad Demo"));
    }
}
