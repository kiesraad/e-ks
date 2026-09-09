use askama::Template;
use axum::response::{IntoResponse, Response};

use crate::{
    AppError, Context, CsbContext, HtmlTemplate,
    csb::pre_submission::{
        extractors::{PreSubmissionGroup, PreSubmissionGroups},
        pages::CsbPreSubmissionOverviewPath,
    },
    filters,
};

#[derive(Template)]
#[template(path = "csb/pre_submission/pages/overview.html")]
struct PreSubmissionOverviewTemplate {
    groups: Vec<PreSubmissionGroup>,
}

/// Every political group imported for the pre-submission check.
pub async fn overview(
    _: CsbPreSubmissionOverviewPath,
    context: CsbContext,
    PreSubmissionGroups(groups): PreSubmissionGroups,
) -> Result<Response, AppError> {
    Ok(HtmlTemplate(PreSubmissionOverviewTemplate { groups }, context).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    use crate::{
        StreamId, csb::examination::structs::BrpCheckState, test_utils::response_body_string,
    };

    fn group(brp: BrpCheckState) -> PreSubmissionGroup {
        PreSubmissionGroup {
            stream_id: StreamId::new(),
            appellation: "Kiesraad Demo".to_string(),
            brp,
        }
    }

    async fn render(groups: Vec<PreSubmissionGroup>) -> String {
        let response = overview(
            CsbPreSubmissionOverviewPath,
            CsbContext::new_test(),
            PreSubmissionGroups(groups),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        response_body_string(response).await
    }

    #[tokio::test]
    async fn lists_imported_groups_with_their_brp_state_and_page() {
        let group = group(BrpCheckState::Errors { errors: 2 });
        let stream_id = group.stream_id;

        let body = render(vec![group]).await;

        assert!(body.contains("Kiesraad Demo"), "{body}");
        assert!(body.contains("Errors"));
        assert!(body.contains(&format!("href=\"/csb/pre-submission/{stream_id}\"")));
        assert!(body.contains("href=\"/csb/pre-submission/import\""));
        assert!(!body.contains("/csb/examination"));
    }

    #[tokio::test]
    async fn a_check_that_never_ran_is_not_reported_as_a_verdict() {
        let body = render(vec![group(BrpCheckState::NotChecked)]).await;

        assert!(body.contains("Not checked"), "{body}");
        assert!(!body.contains("Correct"));
    }

    #[tokio::test]
    async fn renders_a_placeholder_without_groups() {
        let body = render(Vec::new()).await;

        assert!(!body.contains("<table"));
        assert!(
            body.contains("No political groups have been imported"),
            "{body}"
        );
    }
}
