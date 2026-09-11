use askama::Template;
use axum::response::{IntoResponse, Response};

use crate::{
    AppError, Context, CsbContext, HtmlTemplate,
    csb::{
        examination::{
            CsbI4DownloadPath,
            extractors::{CsbPoliticalGroup, CsbPoliticalGroups},
        },
        recovery::paths::CsbRecoveryOverviewPath,
    },
    filters,
    structs::csb::CsbPhase,
};

#[derive(Template)]
#[template(path = "csb/recovery/pages/overview.html")]
struct CsbRecoveryOverviewTemplate {
    political_groups: Vec<CsbPoliticalGroup>,
    /// Every imported group has all of its omissions assessed, which opens
    /// the lock on the download card. The I 4 can be downloaded either way;
    /// the lock only tells whether the assessment behind it is done.
    all_omissions_assessed: bool,
}

/// The recovery overview: every political group with its assessment progress.
pub async fn overview(
    _: CsbRecoveryOverviewPath,
    context: CsbContext,
    CsbPoliticalGroups(political_groups): CsbPoliticalGroups,
) -> Result<Response, AppError> {
    let political_groups: Vec<CsbPoliticalGroup> = political_groups
        .into_iter()
        .filter(|political_group| !political_group.is_deleted)
        .map(|political_group| political_group.with_mode(CsbPhase::Recovery))
        .collect();

    let all_omissions_assessed = !political_groups.is_empty()
        && political_groups
            .iter()
            .all(|political_group| political_group.recovery.is_complete());

    Ok(HtmlTemplate(
        CsbRecoveryOverviewTemplate {
            political_groups,
            all_omissions_assessed,
        },
        context,
    )
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    use crate::{
        StreamId,
        structs::csb::RecoveryProgress,
        test_utils::{response_body_string, sample_political_group},
    };

    fn group(pending: usize, total: usize) -> CsbPoliticalGroup {
        CsbPoliticalGroup {
            political_group: sample_political_group(),
            stream_id: StreamId::new(),
            brp: crate::csb::examination::structs::BrpCheckState::NotChecked,
            mode: CsbPhase::Examination,
            is_examination_finished: false,
            is_deleted: false,
            restoration_count: 0,
            omission_count: total,
            recovery: RecoveryProgress { pending, total },
            first_candidate_name: None,
            first_non_scrapped_candidate_name: None,
            scrapped: Default::default(),
            candidate_list_districts: Default::default(),
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
        assert!(!body.contains(&format!("/csb/examination/{stream_id}")));
        // The progress column shows assessed vs. assessable omissions.
        assert!(body.contains("2 of 3 assessed"));
    }

    async fn render(groups: Vec<CsbPoliticalGroup>) -> String {
        let response = overview(
            CsbRecoveryOverviewPath {},
            CsbContext::new_test(),
            CsbPoliticalGroups(groups),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        response_body_string(response).await
    }

    /// The I 4 can be drawn up at any point; the card only shows whether the
    /// assessment behind it is done.
    #[tokio::test]
    async fn overview_always_offers_the_i4_download() {
        for groups in [vec![group(1, 3)], vec![group(0, 3)], vec![]] {
            let body = render(groups).await;
            assert!(body.contains("Download I 4"));
            assert!(body.contains("/csb/examination/i4.pdf"));
        }
    }

    /// The lock stays closed and the button muted until every group has
    /// assessed its omissions; without any group there is nothing to assess.
    #[tokio::test]
    async fn overview_unlocks_the_card_once_everything_is_assessed() {
        let body = render(vec![group(1, 3), group(0, 3)]).await;
        assert!(body.contains("badge locked"));
        assert!(body.contains("button secondary icon-download"));

        let body = render(vec![group(0, 3), group(0, 0)]).await;
        assert!(body.contains("badge unlocked"));
        assert!(body.contains("button primary icon-download"));

        let body = render(vec![]).await;
        assert!(body.contains("badge locked"));
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
