use askama::Template;
use axum::{
    extract::{Query, State},
    response::{IntoResponse, Response},
};

use crate::{
    AppError, AppRequestState, Context,
    CsbAction::{self},
    CsbContext, CsbStore, HtmlTemplate, Overlay, QueryParamState,
    csb::{
        examination::{
            extractors::CsbPoliticalGroup,
            pages::{CsbBrpCheckPath, CsbPoliticalGroupPath, CsbPoliticalGroupToggleFinishPath},
            structs::{BrpCheckState, CsbCandidateList, RestorationStatus, brp_incomplete_reason},
        },
        import::{brp_sweep_running, do_brp_verification},
    },
    filters, redirect_success,
    structs::csb::{CsbPhase, Omission},
};

#[derive(Template)]
#[template(path = "csb/examination/pages/political_group.html")]
struct CsbPoliticalGroupTemplate {
    political_group: CsbPoliticalGroup,
    brp: BrpCheckState,
    /// Decides between offering to start a sweep and offering to reload.
    brp_running: bool,
    /// Why the BRP data on this page may be incomplete, when the check did not
    /// finish. `Some` is also what makes the start button worth offering.
    brp_incomplete: Option<String>,
    candidate_lists: Vec<CsbCandidateList>,
    political_group_status: RestorationStatus,
    declarations_of_support_omissions: Vec<Omission>,
    has_paper_corrections: bool,
    scrapped_districts: Vec<crate::ElectoralDistrict>,
}

#[derive(Template)]
#[template(path = "csb/examination/pages/delete.html")]
struct CsbPoliticalGroupDeleteTemplate {
    political_group: CsbPoliticalGroup,
    overlay: Overlay,
    close_action: String,
}

/// Render the placeholder political group overview page.
pub async fn overview(
    _: CsbPoliticalGroupPath,
    context: CsbContext,
    store: CsbStore,
) -> Result<Response, AppError> {
    render(context, store, CsbPhase::Examination).await
}

/// The political group page, shared between the examination and the recovery
/// ("Herstelde lijsten") phase.
pub(in crate::csb) async fn render(
    context: CsbContext,
    store: CsbStore,
    mode: CsbPhase,
) -> Result<Response, AppError> {
    let political_group = CsbPoliticalGroup::new_from_csb_store(&store).with_mode(mode);

    let imported_lists = store.get_candidate_lists(crate::projection::WithCorrections::None);
    let brp_findings = store.get_brp_findings();
    let mut candidate_lists = Vec::new();
    let mut all_candidates = Vec::new();
    for list in store.get_candidate_lists(crate::projection::WithCorrections::All) {
        let brp = BrpCheckState::for_candidates(&brp_findings, list.candidates.iter().copied());
        all_candidates.extend(list.candidates.iter().copied());

        let from_original_import = imported_lists.iter().any(|l| l.id == list.id);
        candidate_lists.push(CsbCandidateList {
            restoration_status: RestorationStatus::for_candidate_list(&store, list.id)?,
            is_scrapped: store.is_candidate_list_scrapped(list.id)?,
            scrapped_districts: store.get_candidate_list_scrapped_districts(list.id),
            list,
            brp,
            is_paper_added: !from_original_import,
        });
    }

    // Over the candidates rather than over everyone the sweep touched: the
    // snapshot also holds people who stand on no list at all.
    let brp = BrpCheckState::for_candidates(&brp_findings, all_candidates);
    let brp_running = brp_sweep_running(store.stream_id);
    let brp_incomplete = brp_incomplete_reason(
        &store.get_brp_status(),
        &brp,
        brp_running,
        context.session.locale,
    );
    let political_group_status = RestorationStatus::for_political_group(&store);

    Ok(HtmlTemplate(
        CsbPoliticalGroupTemplate {
            political_group,
            brp,
            brp_running,
            brp_incomplete,
            candidate_lists,
            political_group_status,
            declarations_of_support_omissions: store.get_all_declarations_of_support_omissions(),
            has_paper_corrections: store.has_paper_corrections(),
            scrapped_districts: store.get_scrapped_districts(),
        },
        context,
    )
    .into_response())
}

/// Start the BRP check for this stream, for a group that was imported without
/// one (the fixtures) or whose sweep stopped early.
pub async fn start_brp_check<S: AppRequestState>(
    path: CsbBrpCheckPath,
    State(state): State<S>,
    store: CsbStore,
) -> Result<Response, AppError> {
    do_brp_verification(&store, state.brp_client()).await?;

    Ok(redirect_success(CsbPoliticalGroupPath {
        stream_id: path.stream_id,
    }))
}

pub async fn toggle_examination_finish(
    _: CsbPoliticalGroupToggleFinishPath,
    Query(query): Query<QueryParamState>,
    store: CsbStore,
) -> Result<Response, AppError> {
    let finished = store.is_examination_finished();
    store.update(CsbAction::SetFinished(!finished)).await?;
    Ok(query.redirect_or(CsbPoliticalGroup::new_from_csb_store(&store).group_path()))
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::http::StatusCode;

    use crate::{
        AppState,
        csb::import::claim_sweep_for_test,
        structs::{
            brp::{BrpFinding, BrpStatus},
            candidate_lists::CandidateListId,
            csb::{Omission, OmissionCategory},
            persons::PersonId,
        },
        test_utils::{
            response_body_string, sample_candidate_list, sample_person, sample_political_group,
        },
    };

    /// A store holding one candidate on one list.
    fn store_with_a_candidate() -> (CsbStore, PersonId) {
        let store = CsbStore::new_for_test();
        store.set_political_group(sample_political_group());
        let person = sample_person(PersonId::new());
        let person_id = person.id;
        let mut list = sample_candidate_list(CandidateListId::new());
        list.candidates = vec![person_id];
        store.add_person(person);
        store.add_candidate_list(list);
        (store, person_id)
    }

    /// The examination page's body, for asserting on what it renders.
    async fn examination_body(store: CsbStore) -> String {
        let stream_id = store.stream_id;
        let response = overview(
            CsbPoliticalGroupPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        response_body_string(response).await
    }

    #[tokio::test]
    async fn a_check_that_never_ran_is_not_reported_as_no_errors() {
        let (store, _) = store_with_a_candidate();
        let stream_id = store.stream_id;

        let body = examination_body(store).await;

        assert!(body.contains("Not checked"), "{body}");
        assert!(!body.contains("No BRP errors"));
        // ...and the committee is offered a way to start the check.
        assert!(body.contains(&format!("/csb/examination/{stream_id}/brp-check")));
        assert!(body.contains("Check against the BRP"));
    }

    #[tokio::test]
    async fn a_finished_check_reports_the_findings_and_drops_the_start_button() {
        let (store, person_id) = store_with_a_candidate();
        let stream_id = store.stream_id;
        store
            .update(CsbAction::BrpPersonChecked {
                person: person_id,
                findings: vec![BrpFinding::NotDutch],
            })
            .await
            .unwrap();
        store
            .update(CsbAction::SetBrpStatus(
                crate::structs::brp::BrpStatus::Finished,
            ))
            .await
            .unwrap();

        let body = examination_body(store).await;

        assert!(body.contains("1 BRP error"), "{body}");
        assert!(!body.contains(&format!("/csb/examination/{stream_id}/brp-check")));
    }

    #[tokio::test]
    async fn findings_for_someone_who_is_not_a_candidate_are_not_counted() {
        let (store, person_id) = store_with_a_candidate();
        // The sweep covers every person in the imported snapshot, which holds
        // more people than the lists put forward as candidates.
        let bystander = sample_person(PersonId::new());
        let bystander_id = bystander.id;
        store.add_person(bystander);
        for (person, findings) in [
            (person_id, Vec::new()),
            (
                bystander_id,
                vec![BrpFinding::NotDutch, BrpFinding::NotDutch],
            ),
        ] {
            store
                .update(CsbAction::BrpPersonChecked { person, findings })
                .await
                .unwrap();
        }
        store
            .update(CsbAction::SetBrpStatus(
                crate::structs::brp::BrpStatus::Finished,
            ))
            .await
            .unwrap();

        let body = examination_body(store).await;

        assert!(body.contains("No BRP errors"), "{body}");
        assert!(!body.contains("2 BRP errors"));
    }

    /// The stuck state the committee could not get out of: `InProgress` with
    /// no sweep behind it has to offer a new one, not a refresh.
    #[tokio::test]
    async fn an_abandoned_sweep_still_offers_to_start_a_new_one() {
        let (store, _) = store_with_a_candidate();
        store
            .update(CsbAction::SetBrpStatus(BrpStatus::in_progress()))
            .await
            .unwrap();

        let body = examination_body(store).await;

        assert!(body.contains("Check against the BRP"), "{body}");
        assert!(!body.contains("still running"), "{body}");
        assert!(body.contains("stopped before it finished"), "{body}");
    }

    #[tokio::test]
    async fn a_running_sweep_offers_a_refresh_rather_than_another_check() {
        let (store, _) = store_with_a_candidate();
        let stream_id = store.stream_id;
        store
            .update(CsbAction::SetBrpStatus(BrpStatus::in_progress()))
            .await
            .unwrap();
        let _sweep = claim_sweep_for_test(stream_id);

        let body = examination_body(store).await;

        assert!(body.contains("Refresh this page"), "{body}");
        assert!(body.contains("still running"), "{body}");
    }

    #[tokio::test]
    async fn starting_the_check_marks_the_sweep_as_running() {
        let state = AppState::new_for_tests().await;
        let (store, _) = store_with_a_candidate();
        let stream_id = store.stream_id;

        let response = start_brp_check(CsbBrpCheckPath { stream_id }, State(state), store.clone())
            .await
            .unwrap();

        assert!(
            response.status().is_redirection(),
            "{:?}",
            response.status()
        );
        assert!(brp_sweep_running(stream_id));
        assert!(matches!(
            store.get_brp_status(),
            BrpStatus::InProgress { .. }
        ));
    }

    #[tokio::test]
    async fn political_group_renders_imported_appellation() {
        let store = CsbStore::new_for_test();
        store.set_political_group(sample_political_group());
        let stream_id = store.stream_id;

        let response = overview(
            CsbPoliticalGroupPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        // The appellation is used as the page title.
        let body = response_body_string(response).await;
        assert!(body.contains("Kiesraad Demo"));
        // The paper corrections card posts to the start route.
        assert!(body.contains(&format!("/csb/examination/{stream_id}/paper-corrections")));
    }

    #[tokio::test]
    async fn political_group_falls_back_to_placeholder_when_unnamed() {
        // A fresh store has no imported political group, so the name is unknown.
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let response = overview(
            CsbPoliticalGroupPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("???"));
    }

    #[tokio::test]
    async fn renders_card_for_list_added_in_paper_corrections() {
        use crate::{structs::candidate_lists::CandidateListId, test_utils::sample_candidate_list};

        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let list_id = CandidateListId::new();
        store.set_paper_corrected_candidate_list(sample_candidate_list(list_id));

        let response = overview(
            CsbPoliticalGroupPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Added during paper corrections"));
        assert!(body.contains(&format!("/csb/examination/{stream_id}/list/{list_id}")));
    }

    #[tokio::test]
    async fn hides_card_for_list_deleted_in_paper_corrections() {
        use crate::{structs::candidate_lists::CandidateListId, test_utils::sample_candidate_list};

        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let list_id = CandidateListId::new();
        // An imported list without a corrected counterpart was deleted on paper.
        store
            .data
            .write()
            .imported_data
            .candidate_lists
            .insert(list_id, sample_candidate_list(list_id));

        let response = overview(
            CsbPoliticalGroupPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(!body.contains(&format!("/csb/examination/{stream_id}/list/{list_id}")));
    }

    #[tokio::test]
    async fn card_shows_corrected_electoral_districts() {
        use crate::{
            ElectoralDistrict, structs::candidate_lists::CandidateListId,
            test_utils::sample_candidate_list,
        };

        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let list_id = CandidateListId::new();
        store.add_candidate_list(sample_candidate_list(list_id));
        let mut corrected = sample_candidate_list(list_id);
        corrected.electoral_districts = vec![ElectoralDistrict::Groningen];
        store.set_paper_corrected_candidate_list(corrected);

        let response = overview(
            CsbPoliticalGroupPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // The card shows the corrected districts, not the imported ones.
        assert!(body.contains("Groningen"));
        assert!(!body.contains("Utrecht"));
    }

    #[tokio::test]
    async fn card_shows_corrected_candidate_count() {
        use crate::{
            structs::{candidate_lists::CandidateListId, persons::PersonId},
            test_utils::sample_candidate_list,
        };

        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![PersonId::new()];
        store.add_candidate_list(list.clone());
        let mut corrected = list;
        corrected.candidates.push(PersonId::new());
        store.set_paper_corrected_candidate_list(corrected);

        let response = overview(
            CsbPoliticalGroupPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // The card counts the corrected candidates, not the imported ones.
        assert!(body.contains("<strong>2</strong> candidates"));
    }

    #[tokio::test]
    async fn renders_political_group_omission_count_badge() {
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

        let response = overview(
            CsbPoliticalGroupPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Omissions added"));
    }

    #[tokio::test]
    async fn recovery_mode_hides_the_brp_errors() {
        use crate::{structs::candidate_lists::CandidateListId, test_utils::sample_candidate_list};

        let store = CsbStore::new_for_test();
        store.set_political_group(sample_political_group());
        store.add_candidate_list(sample_candidate_list(CandidateListId::new()));

        let response = render(CsbContext::new_test(), store, CsbPhase::Recovery)
            .await
            .unwrap()
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // The BRP check belongs to the examination, so neither the panel
        // counting its errors nor the per-list error tag renders here.
        assert!(!body.contains("BRP"));
        assert!(!body.contains("restoration-tag-error"));
    }

    #[tokio::test]
    async fn examination_mode_shows_the_brp_errors_panel() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        store.set_political_group(sample_political_group());

        let response = overview(
            CsbPoliticalGroupPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("BRP"));
    }

    #[tokio::test]
    async fn recovery_mode_lists_the_districts_of_a_declarations_of_support_omission() {
        use crate::ElectoralDistrict;

        let store = CsbStore::new_for_test();
        store.set_political_group(sample_political_group());
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

        let response = render(CsbContext::new_test(), store, CsbPhase::Recovery)
            .await
            .unwrap()
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Groningen"));
        assert!(body.contains("Frysl"));
    }

    #[tokio::test]
    async fn toggle_examination_finish_twice() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        // default unfinished => false
        assert!(!store.is_examination_finished());

        toggle_examination_finish(
            CsbPoliticalGroupToggleFinishPath { stream_id },
            Query(QueryParamState::default()),
            store.clone(),
        )
        .await
        .unwrap();

        // toggle once => true
        assert!(store.is_examination_finished());

        toggle_examination_finish(
            CsbPoliticalGroupToggleFinishPath { stream_id },
            Query(QueryParamState::default()),
            store.clone(),
        )
        .await
        .unwrap();

        // toggle twice => false
        assert!(!store.is_examination_finished());
    }

    #[tokio::test]
    async fn toggle_examination_finish_honours_the_redirect_to() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let response = toggle_examination_finish(
            CsbPoliticalGroupToggleFinishPath { stream_id },
            Query(QueryParamState::redirect_to("/back/here".to_string())),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get("Location")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(location.starts_with("/back/here"));
    }

    #[tokio::test]
    async fn toggle_examination_finish_redirects_to_examination_by_default() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let response = toggle_examination_finish(
            CsbPoliticalGroupToggleFinishPath { stream_id },
            Query(QueryParamState::default()),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get("Location")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(location.contains(&format!("csb/examination/{stream_id}")));
    }
}
