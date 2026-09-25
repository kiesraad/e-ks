use askama::Template;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
};

use crate::structs::{common::HasSeverity, persons::Person};

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
    structs::problems::AllProblems,
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
    all_problems: AllProblems,
    candidates: Vec<Person>,
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

    let all_findings = store.get_unlinked_brp_findings(locale);
    let all_problems = store.get_all_problems(context.election)?;

    Ok(HtmlTemplate(
        PreSubmissionGroupTemplate {
            brp_incomplete: brp_incomplete_reason(
                &store.get_brp_status(),
                &group.brp,
                brp_running,
                locale,
            ),
            candidates: problematic_candidates(&all_findings, &all_problems),
            all_findings,
            all_problems,
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

fn problematic_candidates(
    all_findings: &AllBrpFindings,
    all_problems: &AllProblems,
) -> Vec<Person> {
    all_findings
        .candidates
        .iter()
        .map(|c| c.person.clone())
        .chain(all_problems.candidates.iter().map(|c| c.entity.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use axum::http::StatusCode;

    use crate::{
        AppState, CsbAction, CsbStore, ElectoralDistrict, PgEvent,
        structs::{
            brp::{BrpFinding, BrpStatus, BrpValue},
            candidate_lists::{CandidateList, CandidateListId},
            common::{Address, PreviousElectionResults, UtcDateTime},
            list_designation::ListDesignation,
            list_submitters::ListSubmitterId,
            persons::PersonId,
            political_groups::PoliticalGroup,
        },
        test_utils::{
            response_body_string, sample_candidate_list, sample_list_submitter, sample_person,
            sample_person_with_last_name, sample_political_group,
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
        assert!(body.contains("Problems</span>"));
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

    #[tokio::test]
    async fn general_problem_shows_up() {
        let store = PreSubmissionStore(CsbStore::new_for_test());
        store.set_political_group(PoliticalGroup {
            appellation: None,
            list_designation: Some(ListDesignation::Standalone),
            previous_election_results: Some(PreviousElectionResults::ZeroSeats),
        });

        let body = render(store).await;

        assert!(body.contains("General Information</h4>"));
        assert!(body.contains(">Appellation</span>"));
    }

    #[tokio::test]
    async fn list_submitter_problem_shows_up() {
        let store = PreSubmissionStore(CsbStore::new_for_test());
        let mut list_submitter = sample_list_submitter(ListSubmitterId::new());
        list_submitter.name.initials = "A.".parse().expect("parse initials");
        list_submitter.name.last_name = "Nagelhout II".parse().expect("parse last name");
        if let Address::Dutch(ref mut address) = list_submitter.address {
            address.locality = None
        } else {
            panic!("expected Dutch Address")
        }

        store
            .update(CsbAction::PaperCorrectedUpdate(Box::new(
                PgEvent::UpdateListSubmitter(list_submitter),
            )))
            .await
            .expect("Update list submitter");

        let body = render(store).await;

        assert!(body.contains("General Information</h4>"));
        assert!(body.contains("Nagelhout II, A. (List submitter)</h3>"));
        assert!(body.contains(">Address</span>"));
    }

    #[tokio::test]
    async fn substitute_submitter_problem_shows_up() {
        let store = PreSubmissionStore(CsbStore::new_for_test());
        let mut list_submitter = sample_list_submitter(ListSubmitterId::new());
        list_submitter.name.initials = "A.".parse().expect("parse initials");
        list_submitter.name.last_name = "Nagelhout III".parse().expect("parse last name");
        if let Address::Dutch(ref mut address) = list_submitter.address {
            address.locality = None
        } else {
            panic!("expected Dutch Address")
        }

        store
            .update(CsbAction::PaperCorrectedUpdate(Box::new(
                PgEvent::CreateSubstituteSubmitter(list_submitter),
            )))
            .await
            .expect("Create substitute submitter");

        let body = render(store).await;

        assert!(body.contains("General Information</h4>"));
        assert!(body.contains("Nagelhout III, A. (Substitute submitter)</h3>"));
        assert!(body.contains(">Address</span>"));
    }

    #[tokio::test]
    async fn list_problem_shows_up() {
        let store = PreSubmissionStore(CsbStore::new_for_test());
        let list_id = CandidateListId::new();

        store.add_candidate_list(CandidateList {
            id: list_id,
            electoral_districts: BTreeSet::from([ElectoralDistrict::Flevoland]),
            candidates: Vec::new(),
            created_at: UtcDateTime::now(),
        });

        let body = render(store).await;

        assert!(body.contains("Candidate lists</h4>"));
        assert!(body.contains("Flevoland</h3>"));
        assert!(body.contains(">No candidates</span>"));
    }

    #[tokio::test]
    async fn candidate_problem_shows_up() {
        let store = PreSubmissionStore(CsbStore::new_for_test());
        let list_id = CandidateListId::new();
        let person_id = PersonId::new();

        let mut person = sample_person(person_id);
        person.name.first_name = None;
        person.name.initials = "A.".parse().expect("parse initials");
        person.name.last_name = "Nagelhout IV".parse().expect("parse last name");
        person.personal_data.bsn = None;

        let mut list = sample_candidate_list(list_id);
        list.candidates.push(person_id);

        store.add_person(person);
        store.add_candidate_list(list);

        let body = render(store).await;

        assert!(body.contains("Candidates</h4>"));
        assert!(body.contains("Nagelhout IV, A.</h3>"));
        assert!(body.contains(">BSN</span>"));
    }
}
