use askama::Template;
use axum::response::{IntoResponse, Response};

use crate::{
    AppError, Context, CsbContext, CsbStore, HtmlTemplate,
    csb::{
        examination::{extractors::CsbPoliticalGroup, structs::AllOmissions},
        recovery::paths::CsbRecoveryOmissionsPath,
    },
    filters,
    structs::csb::CsbPhase,
};

#[derive(Template)]
#[template(path = "csb/recovery/pages/omissions.html")]
struct CsbRecoveryOmissionsTemplate {
    political_group: CsbPoliticalGroup,
    all_omissions: AllOmissions,
    omission_count: usize,
}

/// The recovery todo page: every omission of the political group grouped by
/// the item it applies to, each with its recovered / not-recovered control.
pub async fn omissions(
    _: CsbRecoveryOmissionsPath,
    context: CsbContext,
    store: CsbStore,
) -> Result<Response, AppError> {
    let political_group =
        CsbPoliticalGroup::new_from_csb_store(&store).with_mode(CsbPhase::Recovery);
    let all_omissions = store.get_all_omissions(&political_group)?;

    Ok(HtmlTemplate(
        CsbRecoveryOmissionsTemplate {
            omission_count: store.get_omission_count(),
            political_group,
            all_omissions,
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
        structs::csb::{Omission, OmissionCategory, OmissionStatus, sample_omission},
        test_utils::response_body_string,
    };

    #[tokio::test]
    async fn renders_status_controls_per_omission_and_progress() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        Omission::new(
            OmissionCategory::PoliticalGroup,
            "Deposit missing".parse().unwrap(),
            "The deposit has not been paid.".parse().unwrap(),
            None,
        )
        .create(&store)
        .await
        .unwrap();
        let mut irreparable = sample_omission(OmissionCategory::PoliticalGroup);
        irreparable.recoverable = false;
        irreparable.title = "Unregistered appellation".parse().unwrap();
        irreparable.create(&store).await.unwrap();

        let response = omissions(
            CsbRecoveryOmissionsPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // The recoverable omission gets the two decision buttons, posting to
        // the set-status route.
        assert!(body.contains(&format!("/csb/recovery/{stream_id}/omission/")));
        assert!(body.contains(r#"value="recovered""#));
        assert!(body.contains(r#"value="not-recovered""#));
        // The irreparable omission renders read-only.
        assert!(body.contains("Unregistered appellation"));
        assert!(body.contains("restoration-tag-unrecoverable"));
        // The progress header counts only assessable omissions.
        assert!(body.contains("0 of 1 assessed"));
        // No examination actions leak into the recovery page.
        assert!(!body.contains("/csb/examination/"));
    }

    #[tokio::test]
    async fn a_recorded_decision_is_highlighted_and_counted() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let omission = sample_omission(OmissionCategory::PoliticalGroup);
        omission.create(&store).await.unwrap();
        omission
            .set_status(&store, OmissionStatus::Recovered)
            .await
            .unwrap();

        let response = omissions(
            CsbRecoveryOmissionsPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("1 of 1 assessed"));
        assert!(body.contains("selected"));
    }

    #[tokio::test]
    async fn declarations_of_support_omissions_name_their_districts() {
        use crate::ElectoralDistrict;

        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        Omission::new(
            OmissionCategory::DeclarationsOfSupport(vec![
                ElectoralDistrict::Groningen,
                ElectoralDistrict::Fryslan,
            ]),
            "Declarations of support missing".parse().unwrap(),
            "Too few declarations of support were handed in."
                .parse()
                .unwrap(),
            None,
        )
        .create(&store)
        .await
        .unwrap();
        // Not district-scoped, so it names none.
        sample_omission(OmissionCategory::PoliticalGroup)
            .create(&store)
            .await
            .unwrap();

        let response = omissions(
            CsbRecoveryOmissionsPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // Only the reported districts are named, each with its own decision.
        assert!(body.contains("1. Groningen"));
        assert!(body.contains("2. Frysl"));
        assert!(!body.contains("Utrecht"));
        assert_eq!(
            body.matches(r#"name="electoral_district""#).count(),
            2,
            "one decision per district"
        );
    }

    #[tokio::test]
    async fn candidate_list_omissions_name_their_list() {
        use crate::{
            ElectoralDistrict, structs::candidate_lists::CandidateListId,
            test_utils::sample_candidate_list,
        };

        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let mut lists = Vec::new();
        for district in [ElectoralDistrict::Groningen, ElectoralDistrict::Utrecht] {
            let mut list = sample_candidate_list(CandidateListId::new());
            list.electoral_districts = vec![district];
            lists.push(list.id);
            store.add_candidate_list(list);
        }
        Omission::new(
            OmissionCategory::CandidateList(vec![lists[1]]),
            "Too many candidates".parse().unwrap(),
            "The list holds more candidates than allowed."
                .parse()
                .unwrap(),
            None,
        )
        .create(&store)
        .await
        .unwrap();

        let response = omissions(
            CsbRecoveryOmissionsPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // Decided as a whole, but the reader is told which list it is about.
        assert!(body.contains("Candidate list"));
        assert!(body.contains("7. Utrecht"));
        assert!(!body.contains("Groningen"));
        assert!(!body.contains("omission-part-table"));
    }

    #[tokio::test]
    async fn renders_empty_state_without_omissions() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let response = omissions(
            CsbRecoveryOmissionsPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("No omissions have been added yet."));
    }
}
