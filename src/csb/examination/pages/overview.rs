use askama::Template;
use axum::response::{IntoResponse, Response};

use crate::{
    AppError, Context, CsbContext, HtmlTemplate,
    csb::{
        examination::{
            extractors::{CsbPoliticalGroup, CsbPoliticalGroups},
            pages::{CsbExaminationOverviewPath, CsbI1DownloadPath, CsbI4DownloadPath},
        },
        import::CsbImportPath,
    },
    filters,
};

#[derive(Template)]
#[template(path = "csb/examination/pages/overview.html")]
struct CsbExaminationOverviewTemplate {
    unfinished_political_groups: Vec<CsbPoliticalGroup>,
    finished_political_groups: Vec<CsbPoliticalGroup>,
}

/// Render the placeholder overview page.
pub async fn overview(
    _: CsbExaminationOverviewPath,
    context: CsbContext,
    CsbPoliticalGroups(political_groups): CsbPoliticalGroups,
) -> Result<Response, AppError> {
    let mut unfinished_political_groups = Vec::new();
    let mut finished_political_groups = Vec::new();
    for political_group in political_groups {
        if political_group.is_deleted {
            continue;
        }
        if political_group.is_examination_finished {
            finished_political_groups.push(political_group)
        } else {
            unfinished_political_groups.push(political_group);
        }
    }
    Ok(HtmlTemplate(
        CsbExaminationOverviewTemplate {
            unfinished_political_groups,
            finished_political_groups,
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
        csb::examination::structs::BrpCheckState,
        test_utils::{response_body_string, sample_political_group},
    };

    #[tokio::test]
    async fn overview_renders_imported_political_group_names() {
        let groups = CsbPoliticalGroups(vec![CsbPoliticalGroup {
            political_group: sample_political_group(),
            stream_id: StreamId::new(),
            brp: BrpCheckState::NotChecked,
            mode: crate::structs::csb::CsbPhase::Examination,
            is_examination_finished: false,
            is_deleted: false,
            restoration_count: 0,
            omission_count: 0,
            pending_omission_count: 0,
            actionable_omission_count: 0,
            first_candidate_name: None,
        }]);

        let response = overview(
            CsbExaminationOverviewPath {},
            CsbContext::new_test(),
            groups,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        // The seeded group's appellation is rendered in the "added" table.
        let body = response_body_string(response).await;
        assert!(body.contains("Kiesraad Demo"));
    }

    #[tokio::test]
    async fn the_brp_column_follows_the_group_rather_than_always_reading_correct() {
        let groups = CsbPoliticalGroups(vec![CsbPoliticalGroup {
            political_group: sample_political_group(),
            stream_id: StreamId::new(),
            brp: BrpCheckState::Errors { errors: 2 },
            mode: crate::structs::csb::CsbPhase::Examination,
            is_examination_finished: false,
            is_deleted: false,
            restoration_count: 0,
            omission_count: 0,
            pending_omission_count: 0,
            actionable_omission_count: 0,
            first_candidate_name: None,
        }]);

        let response = overview(
            CsbExaminationOverviewPath {},
            CsbContext::new_test(),
            groups,
        )
        .await
        .unwrap()
        .into_response();

        let body = response_body_string(response).await;
        assert!(body.contains("Errors"), "{body}");
        assert!(!body.contains("Correct"));
    }

    #[tokio::test]
    async fn a_check_that_did_not_finish_is_not_reported_as_a_verdict() {
        let groups = CsbPoliticalGroups(vec![CsbPoliticalGroup {
            political_group: sample_political_group(),
            stream_id: StreamId::new(),
            brp: BrpCheckState::Incomplete { errors: 2 },
            mode: crate::structs::csb::CsbPhase::Examination,
            is_examination_finished: false,
            is_deleted: false,
            restoration_count: 0,
            omission_count: 0,
            pending_omission_count: 0,
            actionable_omission_count: 0,
            first_candidate_name: None,
        }]);

        let response = overview(
            CsbExaminationOverviewPath {},
            CsbContext::new_test(),
            groups,
        )
        .await
        .unwrap()
        .into_response();

        // What it found so far is on the group page; one tag must not pass for
        // a verdict the check never reached.
        let body = response_body_string(response).await;
        assert!(body.contains("Check not finished"), "{body}");
        assert!(!body.contains(">Errors<"));
    }

    #[tokio::test]
    async fn overview_skips_deleted_political_group_names() {
        let groups = CsbPoliticalGroups(vec![CsbPoliticalGroup {
            political_group: sample_political_group(),
            stream_id: StreamId::new(),
            brp: BrpCheckState::NotChecked,
            mode: crate::structs::csb::CsbPhase::Examination,
            pending_omission_count: 0,
            actionable_omission_count: 0,
            is_examination_finished: false,
            is_deleted: true,
            restoration_count: 0,
            omission_count: 0,
            first_candidate_name: None,
        }]);

        let response = overview(
            CsbExaminationOverviewPath {},
            CsbContext::new_test(),
            groups,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        // The seeded group's appellation is not rendered as it is deleted.
        let body = response_body_string(response).await;
        assert!(!body.contains("Kiesraad Demo"));
    }

    #[tokio::test]
    async fn overview_renders_omission_count_badge() {
        let groups = CsbPoliticalGroups(vec![CsbPoliticalGroup {
            political_group: sample_political_group(),
            stream_id: StreamId::new(),
            brp: BrpCheckState::NotChecked,
            mode: crate::structs::csb::CsbPhase::Examination,
            pending_omission_count: 0,
            actionable_omission_count: 0,
            is_examination_finished: false,
            is_deleted: false,
            restoration_count: 0, /* omission count should be used and > 0 */
            omission_count: 3,
            first_candidate_name: None,
        }]);

        let response = overview(
            CsbExaminationOverviewPath {},
            CsbContext::new_test(),
            groups,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Omissions added"));
    }

    #[tokio::test]
    async fn overview_renders_without_political_groups() {
        let response = overview(
            CsbExaminationOverviewPath {},
            CsbContext::new_test(),
            CsbPoliticalGroups(vec![]),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(!body.contains("<table"));
    }
}
