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
    let political_groups: Vec<CsbPoliticalGroup> = political_groups
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

    use crate::{structs::csb::RecoveryProgress, test_utils::response_body_string};

    fn group(pending: usize, total: usize) -> CsbPoliticalGroup {
        CsbPoliticalGroup {
            omission_count: total,
            recovery: RecoveryProgress { pending, total },
            ..CsbPoliticalGroup::sample("Kiesraad Demo")
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
        // The page leads back to the CSB overview.
        assert!(body.contains(r#"href="/csb" class="button secondary">Back<"#));
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

    /// The hearing is recorded in phase 4, on the finalise page; the recovery
    /// overview only offers the downloads.
    #[tokio::test]
    async fn overview_does_not_offer_the_hearing_details() {
        let body = render(vec![group(0, 3)]).await;
        assert!(!body.contains("/csb/examination/hearing-details"));
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
