use askama::Template;
use axum::response::{IntoResponse, Response};

use crate::{
    AppError, Context, CsbContext, CsbStore, HtmlTemplate,
    csb::{
        examination::{
            extractors::CsbPoliticalGroup,
            pages::CsbAllBrpFindingsPath,
            structs::{BrpBadge, BrpCheckState, ProblematicCandidate, brp_incomplete_reason},
        },
        import::brp_sweep_running,
    },
    filters,
    structs::{common::HasSeverity, problems::AllProblems},
};

#[derive(Template)]
#[template(path = "csb/examination/pages/all_brp_findings.html")]
struct CsbAllBrpFindingsTemplate {
    political_group: CsbPoliticalGroup,
    brp: BrpCheckState,
    /// Decides between offering to start a sweep and offering to reload.
    brp_running: bool,
    /// Why the list below may be incomplete, when the check did not finish.
    brp_incomplete: Option<String>,
    /// The badges of the BRP strip, derived from `brp` and `brp_running`.
    brp_badges: Vec<BrpBadge>,
    all_problems: AllProblems,
    candidates: Vec<ProblematicCandidate>,
}
/*
TODO #1263:
 - merge main into this branch after  #1188 is merged
 - set the redirect_to param on problems and brp error links on all_brp_findings page
 - use it to change behaviour of back button on:
   - candidate page
   - candidate-list page
   - basisgegevens page
   - how long should the back navigation be honored? https://github.com/kiesraad/e-ks/issues/1263#issuecomment-5873737016
   - should something similar be implemented for all restorations? https://github.com/kiesraad/e-ks/issues/1263#issuecomment-5873737016 
*/

pub async fn all_brp_findings(
    _: CsbAllBrpFindingsPath,
    context: CsbContext,
    store: CsbStore,
) -> Result<Response, AppError> {
    let political_group = CsbPoliticalGroup::new_from_csb_store(&store);
    let all_problems = store.get_all_problems(context.election)?;
    let candidates = store
        .get_all_brp_findings(&political_group, context.session.locale)
        .with_problems(&all_problems, |person| {
            store
                .get_first_list(person.id)
                .map(|list| political_group.candidate_path(&list.id, &person.id))
        });

    let brp = BrpCheckState::for_political_group(&store);
    let brp_running = brp_sweep_running(store.stream_id);

    Ok(HtmlTemplate(
        CsbAllBrpFindingsTemplate {
            brp_incomplete: brp_incomplete_reason(
                &store.get_brp_status(),
                &brp,
                brp_running,
                context.session.locale,
            ),
            brp_badges: brp.strip_badges(brp_running),
            brp,
            brp_running,
            political_group,
            all_problems,
            candidates,
        },
        context,
    )
    .into_response())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use axum::http::StatusCode;

    use crate::{
        CsbAction, ElectoralDistrict, PgEvent,
        structs::{
            brp::{BrpFindingKind, BrpStatus, BrpValue},
            candidate_lists::{CandidateList, CandidateListId},
            common::{Address, PreviousElectionResults, UtcDateTime},
            list_designation::ListDesignation,
            list_submitters::ListSubmitterId,
            persons::PersonId,
            political_groups::PoliticalGroup,
        },
        test_utils::{
            response_body_string, sample_candidate_list, sample_list_submitter, sample_person,
            sample_person_with_last_name,
        },
    };

    /// A store with `candidates` on one list, in that order.
    fn store_with_candidates(candidates: &[PersonId]) -> CsbStore {
        let store = CsbStore::new_for_test();
        let mut list = sample_candidate_list(CandidateListId::new());
        list.candidates = candidates.to_vec();
        for (index, id) in candidates.iter().enumerate() {
            store.add_person(sample_person_with_last_name(
                *id,
                &format!("Kandidaat{index}"),
            ));
        }
        store.add_candidate_list(list);
        store
    }

    async fn render(store: CsbStore) -> String {
        let stream_id = store.stream_id;
        let response = all_brp_findings(
            CsbAllBrpFindingsPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        response_body_string(response).await
    }

    #[tokio::test]
    async fn lists_every_finding_under_the_candidate_it_belongs_to() {
        let (first, second) = (PersonId::new(), PersonId::new());
        let store = store_with_candidates(&[first, second]);
        store
            .update(CsbAction::BrpPersonChecked {
                person: first,
                findings: vec![
                    BrpFindingKind::NotDutch.into(),
                    BrpFindingKind::Mismatch {
                        brp_value: BrpValue::PlaceOfResidence("Amsterdam".parse().unwrap()),
                    }
                    .into(),
                ],
            })
            .await
            .unwrap();
        // Checked with nothing found: this candidate does not belong here.
        store
            .update(CsbAction::BrpPersonChecked {
                person: second,
                findings: Vec::new(),
            })
            .await
            .unwrap();
        store
            .update(CsbAction::SetBrpStatus(BrpStatus::Finished))
            .await
            .unwrap();
        let stream_id = store.stream_id;

        let body = render(store).await;

        assert!(body.contains("Kandidaat0"), "{body}");
        assert!(!body.contains("Kandidaat1"));
        assert!(body.contains("no Dutch nationality"));
        assert!(body.contains("Amsterdam"));
        // Every finding links to the candidate it is about.
        assert!(body.contains(&format!("/csb/examination/{stream_id}/list/")));
    }

    #[tokio::test]
    async fn handled_findings_are_shown_as_handled() {
        let person = PersonId::new();
        let store = store_with_candidates(&[person]);
        let kinds = [BrpFindingKind::NotDutch, BrpFindingKind::BsnUnknown];
        store
            .update(CsbAction::BrpPersonChecked {
                person,
                findings: kinds.iter().cloned().map(Into::into).collect(),
            })
            .await
            .unwrap();
        store
            .update(CsbAction::SetBrpStatus(BrpStatus::Finished))
            .await
            .unwrap();
        let mark_handled = async |finding: BrpFindingKind| {
            store
                .update(CsbAction::SetBrpFindingHandled {
                    person,
                    finding,
                    handled: true,
                })
                .await
                .unwrap();
        };

        // The general information of the sample group stays red throughout,
        // over its problems.
        mark_handled(kinds[0].clone()).await;
        let body = render(store.clone()).await;
        assert_eq!(body.matches("restoration-tag-handled").count(), 1, "{body}");
        // The summary strip, the candidate and the general information.
        assert_eq!(body.matches("restoration-strip-error").count(), 3, "{body}");

        mark_handled(kinds[1].clone()).await;
        let body = render(store.clone()).await;
        // Both findings, and the summary strip above them.
        assert_eq!(body.matches("restoration-tag-handled").count(), 3, "{body}");
        assert_eq!(body.matches("restoration-strip-error").count(), 1, "{body}");
    }

    #[tokio::test]
    async fn a_check_that_never_ran_is_not_reported_as_an_empty_list_of_errors() {
        let store = store_with_candidates(&[PersonId::new()]);

        let body = render(store).await;

        assert!(body.contains("Not checked"), "{body}");
        assert!(body.contains("have not been checked against the BRP yet"));
    }

    #[tokio::test]
    async fn a_finished_check_without_findings_says_so() {
        let person = PersonId::new();
        let store = store_with_candidates(&[person]);
        store
            .update(CsbAction::BrpPersonChecked {
                person,
                findings: Vec::new(),
            })
            .await
            .unwrap();
        store
            .update(CsbAction::SetBrpStatus(BrpStatus::Finished))
            .await
            .unwrap();

        let body = render(store).await;

        assert!(body.contains("No BRP errors"), "{body}");
    }

    #[tokio::test]
    async fn general_problem_shows_up() {
        let store = store_with_candidates(&[PersonId::new()]);
        let stream_id = store.stream_id;
        store.set_political_group(PoliticalGroup {
            appellation: None,
            list_designation: Some(ListDesignation::Standalone),
            previous_election_results: Some(PreviousElectionResults::ZeroSeats),
        });

        let body = render(store).await;

        assert!(body.contains("General Information</h4>"));
        assert!(body.contains(&format!(
            "href=\"/csb/examination/{stream_id}/general-information\">Appellation</a>"
        )));
    }

    #[tokio::test]
    async fn list_submitter_problem_shows_up() {
        let store = store_with_candidates(&[PersonId::new()]);
        let stream_id = store.stream_id;
        let mut list_submitter = sample_list_submitter(ListSubmitterId::new());
        list_submitter.name.initials = Some("A.".parse().expect("parse initials"));
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
        assert!(body.contains(&format!(
            "href=\"/csb/examination/{stream_id}/general-information\">Address</a>"
        )));
    }

    #[tokio::test]
    async fn substitute_submitter_problem_shows_up() {
        let store = store_with_candidates(&[PersonId::new()]);
        let stream_id = store.stream_id;
        let mut list_submitter = sample_list_submitter(ListSubmitterId::new());
        list_submitter.name.initials = Some("A.".parse().expect("parse initials"));
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
        assert!(body.contains(&format!(
            "href=\"/csb/examination/{stream_id}/general-information\">Address</a>"
        )));
    }

    #[tokio::test]
    async fn list_problem_shows_up() {
        let store = store_with_candidates(&[PersonId::new()]);
        let stream_id = store.stream_id;
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
        assert!(body.contains(&format!(
            "href=\"/csb/examination/{stream_id}/list/{list_id}\">No candidates</a>"
        )));
    }

    #[tokio::test]
    async fn candidate_problem_shows_up() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let list_id = CandidateListId::new();
        let person_id = PersonId::new();

        let mut person = sample_person(person_id);
        person.name.first_name = None;
        person.name.initials = Some("A.".parse().expect("parse initials"));
        person.name.last_name = "Nagelhout IV".parse().expect("parse last name");
        person.personal_data.bsn = None;

        let mut list = sample_candidate_list(list_id);
        list.candidates.push(person_id);

        store.add_person(person);
        store.add_candidate_list(list);

        let body = render(store).await;

        assert!(body.contains("Candidates</h4>"));
        assert!(body.contains("Nagelhout IV, A.</h3>"));
        assert!(body.contains(&format!(
            "href=\"/csb/examination/{stream_id}/list/{list_id}/candidate/{person_id}\">BSN</a>"
        )));
    }
}
