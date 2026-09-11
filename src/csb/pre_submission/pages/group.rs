use askama::Template;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, AppRequestState, Context, CsbContext, HtmlTemplate,
    csb::{
        examination::structs::{AllBrpFindings, brp_incomplete_reason},
        import::{brp_sweep_running, do_brp_verification},
        pre_submission::{
            extractors::{PreSubmissionGroup, PreSubmissionStore},
            pages::{CsbPreSubmissionBrpCheckPath, CsbPreSubmissionGroupPath},
        },
    },
    filters, redirect_success,
};

#[derive(Template)]
#[template(path = "csb/pre_submission/pages/group.html")]
struct PreSubmissionGroupTemplate {
    group: PreSubmissionGroup,
    /// Decides between offering to start a sweep and offering to reload.
    brp_running: bool,
    /// Why the list may be incomplete, when the check did not finish.
    brp_incomplete: Option<String>,
    all_findings: AllBrpFindings,
}

/// The BRP findings of one pre-submitted package, per candidate.
pub async fn group(
    _: CsbPreSubmissionGroupPath,
    context: CsbContext,
    store: PreSubmissionStore,
) -> Result<Response, AppError> {
    let group = PreSubmissionGroup::from_store(&store);
    let brp_running = brp_sweep_running(store.stream_id);
    let locale = context.session.locale;

    Ok(HtmlTemplate(
        PreSubmissionGroupTemplate {
            brp_incomplete: brp_incomplete_reason(
                &store.get_brp_status(),
                &group.brp,
                brp_running,
                locale,
            ),
            all_findings: store.get_unlinked_brp_findings(locale),
            group,
            brp_running,
        },
        context,
    )
    .into_response())
}

/// Start (or resume) the BRP check for this package.
pub async fn start_brp_check<S: AppRequestState>(
    path: CsbPreSubmissionBrpCheckPath,
    State(state): State<S>,
    store: PreSubmissionStore,
) -> Result<Response, AppError> {
    do_brp_verification(&store, state.brp_client()).await?;

    Ok(redirect_success(CsbPreSubmissionGroupPath {
        stream_id: path.stream_id,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    use crate::{
        AppState, CsbAction, CsbStore,
        structs::{
            brp::{BrpFinding, BrpStatus, BrpValue},
            candidate_lists::CandidateListId,
            persons::PersonId,
        },
        test_utils::{
            response_body_string, sample_candidate_list, sample_person_with_last_name,
            sample_political_group,
        },
    };

    /// A store holding the sample group with one candidate.
    fn store_with_a_candidate() -> (PreSubmissionStore, PersonId) {
        let store = CsbStore::new_for_test();
        store.set_political_group(sample_political_group());
        let person = sample_person_with_last_name(PersonId::new(), "Kandidaat");
        let person_id = person.id;
        let mut list = sample_candidate_list(CandidateListId::new());
        list.candidates = vec![person_id];
        store.add_person(person);
        store.add_candidate_list(list);
        (PreSubmissionStore(store), person_id)
    }

    async fn render(store: PreSubmissionStore) -> String {
        let stream_id = store.stream_id;
        let response = group(
            CsbPreSubmissionGroupPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        response_body_string(response).await
    }

    async fn finish_check(store: &PreSubmissionStore, person: PersonId, findings: Vec<BrpFinding>) {
        store
            .update(CsbAction::BrpPersonChecked { person, findings })
            .await
            .unwrap();
        store
            .update(CsbAction::SetBrpStatus(BrpStatus::Finished))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_check_that_never_ran_offers_to_start_one() {
        let (store, _) = store_with_a_candidate();
        let stream_id = store.stream_id;

        let body = render(store).await;

        assert!(body.contains("Kiesraad Demo"), "{body}");
        assert!(body.contains("Not checked"));
        assert!(body.contains(&format!("/csb/pre-submission/{stream_id}/brp-check")));
        assert!(body.contains("Check against the BRP"));
    }

    #[tokio::test]
    async fn lists_the_findings_per_candidate_without_linking_anywhere() {
        let (store, person_id) = store_with_a_candidate();
        finish_check(
            &store,
            person_id,
            vec![
                BrpFinding::NotDutch,
                BrpFinding::Mismatch {
                    brp_value: BrpValue::PlaceOfResidence("Amsterdam".parse().unwrap()),
                },
            ],
        )
        .await;
        let stream_id = store.stream_id;

        let body = render(store).await;

        assert!(body.contains("Kandidaat"), "{body}");
        assert!(body.contains("2 BRP errors"));
        assert!(body.contains("no Dutch nationality"));
        assert!(body.contains("Amsterdam"));
        assert!(body.contains("<span class=\"restoration-tag restoration-tag-error\">"));
        assert!(!body.contains("/csb/examination"));
        // The check is done, so there is nothing left to start.
        assert!(!body.contains(&format!("/csb/pre-submission/{stream_id}/brp-check")));
    }

    #[tokio::test]
    async fn a_finished_check_without_findings_says_so() {
        let (store, person_id) = store_with_a_candidate();
        finish_check(&store, person_id, Vec::new()).await;

        let body = render(store).await;

        assert!(body.contains("No BRP errors"), "{body}");
        assert!(body.contains("found no errors"));
    }

    #[tokio::test]
    async fn the_page_carries_none_of_the_examination_actions() {
        let (store, _) = store_with_a_candidate();

        let body = render(store).await;

        for action in [
            "toggle-finish",
            "paper-corrections",
            "omission",
            "correction",
            "delete",
        ] {
            assert!(!body.contains(action), "found {action:?} in {body}");
        }
    }

    #[tokio::test]
    async fn starting_the_check_marks_the_sweep_as_running() {
        let state = AppState::new_for_tests().await;
        let (store, _) = store_with_a_candidate();
        let stream_id = store.stream_id;

        let response = start_brp_check(
            CsbPreSubmissionBrpCheckPath { stream_id },
            State(state),
            PreSubmissionStore(store.0.clone()),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response.headers()["Location"].to_str().unwrap();
        assert!(location.starts_with(&format!("/csb/pre-submission/{stream_id}")));
        assert!(brp_sweep_running(stream_id));
        assert!(matches!(
            store.get_brp_status(),
            BrpStatus::InProgress { .. }
        ));
    }
}
