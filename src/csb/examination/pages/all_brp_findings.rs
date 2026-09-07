use askama::Template;
use axum::response::{IntoResponse, Response};

use crate::{
    AppError, Context, CsbContext, CsbStore, HtmlTemplate,
    csb::{
        examination::{
            extractors::CsbPoliticalGroup,
            pages::CsbAllBrpFindingsPath,
            structs::{AllBrpFindings, BrpCheckState, brp_incomplete_reason},
        },
        import::brp_sweep_running,
    },
    filters,
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
    all_findings: AllBrpFindings,
}

pub async fn all_brp_findings(
    _: CsbAllBrpFindingsPath,
    context: CsbContext,
    store: CsbStore,
) -> Result<Response, AppError> {
    let political_group = CsbPoliticalGroup::new_from_csb_store(&store);
    let all_findings = store.get_all_brp_findings(&political_group, context.session.locale);

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
            brp,
            brp_running,
            political_group,
            all_findings,
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
        CsbAction,
        structs::{
            brp::{BrpFinding, BrpStatus, BrpValue},
            candidate_lists::CandidateListId,
            persons::PersonId,
        },
        test_utils::{response_body_string, sample_candidate_list, sample_person_with_last_name},
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
                    BrpFinding::NotDutch,
                    BrpFinding::Mismatch {
                        brp_value: BrpValue::PlaceOfResidence("Amsterdam".parse().unwrap()),
                    },
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
}
