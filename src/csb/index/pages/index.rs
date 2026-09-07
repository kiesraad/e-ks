use askama::Template;
use axum::response::{IntoResponse, Response};
use chrono::NaiveDate;

use crate::{
    AppError, Context, CsbContext, ElectionConfig, HtmlTemplate, csb::index::CsbIndexPath, filters,
};

#[derive(Template)]
#[template(path = "csb/index/pages/index.html")]
struct CsbIndexTemplate {
    /// The phase whose date window covers today; the homepage highlights its
    /// card and mutes the others. Every implemented phase stays reachable
    /// regardless of the date.
    current_phase: u8,
}

/// The phase whose window covers `today`, by the closing dates shown on the
/// phase cards.
fn current_phase(election: &ElectionConfig, today: NaiveDate) -> u8 {
    if today <= election.nomination_day_date() {
        1
    } else if today <= election.document_review_date() {
        2
    } else if today <= election.public_session().datetime.date() {
        3
    } else {
        4
    }
}

pub async fn index(_: CsbIndexPath, context: CsbContext) -> Result<Response, AppError> {
    let current_phase = current_phase(&context.election, chrono::Utc::now().date_naive());
    Ok(HtmlTemplate(CsbIndexTemplate { current_phase }, context).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    use crate::test_utils::response_body_string;

    #[tokio::test]
    async fn index_renders_all_phase_titles() {
        let response = index(CsbIndexPath {}, CsbContext::new_test())
            .await
            .unwrap()
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Registration"));
        assert!(body.contains("Pre-submission"));
        assert!(body.contains("Examination"));
        assert!(body.contains("Rectified lists"));
        assert!(body.contains("Finalise candidate lists"));
        assert!(!body.contains("Phase 5"));
    }

    #[tokio::test]
    async fn index_links_registered_political_groups_as_phase_0() {
        let response = index(CsbIndexPath {}, CsbContext::new_test())
            .await
            .unwrap()
            .into_response();

        let body = response_body_string(response).await;
        assert!(body.contains("Phase 0"));
        assert!(body.contains("href=\"/csb/registered-political-groups\""));
        assert!(body.contains("Go to registered political groups"));
    }

    #[tokio::test]
    async fn index_links_examination_and_recovery_phases() {
        let response = index(CsbIndexPath {}, CsbContext::new_test())
            .await
            .unwrap()
            .into_response();

        let body = response_body_string(response).await;
        // Both implemented phases are always reachable from the homepage,
        // regardless of the current date.
        assert!(body.contains("href=\"/csb/examination\""));
        assert!(body.contains("Go to examination"));
        assert!(body.contains("href=\"/csb/recovery\""));
    }

    #[tokio::test]
    async fn index_links_i4_download_as_phase_4() {
        let response = index(CsbIndexPath {}, CsbContext::new_test())
            .await
            .unwrap()
            .into_response();

        let body = response_body_string(response).await;
        assert!(body.contains("Phase 4"));
        assert!(body.contains("href=\"/csb/examination/i4.pdf\""));
        assert!(body.contains("Download I 4"));
    }

    #[test]
    fn current_phase_follows_the_election_dates() {
        let election = ElectionConfig::EK27;
        let day_before = |date: NaiveDate| date.pred_opt().unwrap();
        let day_after = |date: NaiveDate| date.succ_opt().unwrap();

        assert_eq!(
            current_phase(&election, day_before(election.nomination_day_date())),
            1
        );
        assert_eq!(current_phase(&election, election.nomination_day_date()), 1);
        assert_eq!(
            current_phase(&election, day_after(election.nomination_day_date())),
            2
        );
        assert_eq!(current_phase(&election, election.document_review_date()), 2);
        assert_eq!(
            current_phase(&election, day_after(election.document_review_date())),
            3
        );
        assert_eq!(
            current_phase(&election, election.public_session().datetime.date()),
            3
        );
        assert_eq!(
            current_phase(
                &election,
                day_after(election.public_session().datetime.date())
            ),
            4
        );
        assert_eq!(current_phase(&election, election.election_date()), 4);
        assert_eq!(
            current_phase(&election, day_after(election.election_date())),
            4
        );
    }
}
