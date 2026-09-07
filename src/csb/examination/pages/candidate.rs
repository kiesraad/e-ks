use askama::Template;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, AppRequestState, Context, CsbAction, CsbContext, CsbStore, ElectoralDistrict,
    HtmlTemplate,
    csb::{
        examination::{
            extractors::CsbPoliticalGroup,
            pages::{CsbCandidateBrpCheckPath, CsbCandidatePath},
            structs::{
                BrpCheckState, CandidateBrpFindings, PaperCorrected, PaperCorrectedPersonDetails,
                brp_incomplete_reason,
            },
        },
        import::brp_sweep_running,
    },
    filters,
    projection::WithCorrections,
    redirect_success,
    structs::{
        candidate_lists::CandidateListId,
        csb::{CsbPhase, Omission},
        persons::{Person, PersonId},
    },
};

#[derive(Template)]
#[template(path = "csb/examination/pages/candidate.html")]
struct CsbCandidateTemplate {
    political_group: CsbPoliticalGroup,
    list_id: CandidateListId,
    electoral_districts: Vec<ElectoralDistrict>,
    candidate: Person,
    details: PaperCorrectedPersonDetails,
    position: PaperCorrected,
    candidate_omissions: Vec<Omission>,
    brp: CandidateBrp,
    is_scrapped: bool,
    recovery_position: Option<usize>,
    scrapped_districts: Vec<ElectoralDistrict>,
    all_districts_scrapped: bool,
}

/// What the candidate page shows about the BRP check.
struct CandidateBrp {
    /// What the BRP check found, per row of the details table.
    findings: CandidateBrpFindings,
    /// Whether this candidate has been checked at all, which is what decides
    /// whether a re-check is offered.
    state: BrpCheckState,
    /// Whether a sweep is under way. While it is, the candidate is waiting for
    /// its turn rather than for a check of their own.
    running: bool,
    /// Why the BRP data may be incomplete, if the check did not finish.
    incomplete: Option<String>,
}

impl CandidateBrp {
    fn for_candidate(store: &CsbStore, person_id: PersonId, locale: crate::Locale) -> Self {
        let state = BrpCheckState::for_candidate(store, person_id);
        let running = brp_sweep_running(store.stream_id);

        Self {
            findings: CandidateBrpFindings::new(
                &store.get_brp_findings_for_person(person_id),
                locale,
            ),
            incomplete: brp_incomplete_reason(&store.get_brp_status(), &state, running, locale),
            state,
            running,
        }
    }
}

pub async fn overview(
    CsbCandidatePath {
        list_id, person_id, ..
    }: CsbCandidatePath,
    context: CsbContext,
    store: CsbStore,
) -> Result<Response, AppError> {
    render(list_id, person_id, context, store, CsbPhase::Examination).await
}

/// The candidate detail page, shared between the examination and the recovery
/// ("Herstelde lijsten") phase.
pub(in crate::csb) async fn render(
    list_id: CandidateListId,
    person_id: PersonId,
    context: CsbContext,
    store: CsbStore,
    mode: CsbPhase,
) -> Result<Response, AppError> {
    let political_group = CsbPoliticalGroup::new_from_csb_store(&store).with_mode(mode);

    let imported = store.get_person(person_id, WithCorrections::None);
    let corrected = store.get_person(person_id, WithCorrections::Paper);
    let csb_corrected = store.get_person(person_id, WithCorrections::All);
    let candidate = imported
        .clone()
        .or_else(|| corrected.clone())
        .ok_or(AppError::GenericNotFound)?;
    let details = PaperCorrectedPersonDetails::new(
        imported.as_ref(),
        corrected.as_ref(),
        csb_corrected.as_ref(),
        context.session.locale,
    );
    let position = PaperCorrected::new(
        store
            .get_candidate_position(list_id, person_id, WithCorrections::None)
            .map(|p| p.to_string())
            .unwrap_or_default(),
        store
            .get_candidate_position(list_id, person_id, WithCorrections::Paper)
            .map(|p| p.to_string())
            .unwrap_or_default(),
    );
    // The corrected electoral districts take precedence over the imported ones.
    let electoral_districts = store
        .get_candidate_list(list_id, WithCorrections::All)
        .map(|list| list.electoral_districts)
        .ok_or(AppError::GenericNotFound)?;
    let candidate_omissions = store.get_candidate_omissions(person_id);
    let brp = CandidateBrp::for_candidate(&store, person_id, context.session.locale);
    let scrapped_districts = store.get_candidate_list_scrapped_districts(list_id);
    let all_districts_scrapped =
        !electoral_districts.is_empty() && scrapped_districts.len() == electoral_districts.len();

    Ok(HtmlTemplate(
        CsbCandidateTemplate {
            political_group,
            list_id,
            electoral_districts,
            candidate,
            details,
            position,
            candidate_omissions,
            brp,
            is_scrapped: store.is_candidate_scrapped(person_id, list_id),
            recovery_position: store.get_recovery_position(list_id, person_id),
            scrapped_districts,
            all_districts_scrapped,
        },
        context,
    )
    .into_response())
}

/// Check this one candidate against the BRP, for a candidate whose findings a
/// correction dropped or whom the sweep never reached.
///
/// A single lookup, so it answers within the request instead of running in the
/// background like the sweep over a whole list does.
pub async fn check_against_brp<S: AppRequestState>(
    path: CsbCandidateBrpCheckPath,
    State(state): State<S>,
    store: CsbStore,
) -> Result<Response, AppError> {
    let candidate = store
        .get_person(path.person_id, WithCorrections::All)
        .ok_or(AppError::GenericNotFound)?;

    for (person, findings) in state
        .brp_client()
        .verify_batch(std::slice::from_ref(&candidate))
        .await?
    {
        store
            .update(CsbAction::BrpPersonChecked { person, findings })
            .await?;
    }

    Ok(redirect_success(CsbCandidatePath {
        stream_id: path.stream_id,
        list_id: path.list_id,
        person_id: path.person_id,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::http::StatusCode;

    use crate::{
        csb::import::claim_sweep_for_test,
        structs::{
            brp::{BrpFinding, BrpStatus},
            csb::OmissionCategory,
            persons::PersonId,
        },
        test_utils::{response_body_string, sample_candidate_list, sample_person},
    };

    /// A store holding one candidate on one list.
    fn store_with_candidate() -> (CsbStore, CandidateListId, PersonId) {
        let store = CsbStore::new_for_test();
        let person = sample_person(PersonId::new());
        let person_id = person.id;
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id];
        store.add_person(person);
        store.add_candidate_list(list);
        (store, list_id, person_id)
    }

    /// The examination page's body, for asserting on what it renders.
    async fn examination_body(
        store: CsbStore,
        list_id: CandidateListId,
        person_id: PersonId,
    ) -> String {
        let stream_id = store.stream_id;
        let response = overview(
            CsbCandidatePath {
                stream_id,
                list_id,
                person_id,
            },
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
    async fn checking_one_candidate_records_what_the_brp_answered() {
        use crate::brp_stub::{BrpStub, matching_record};

        let store = CsbStore::new_for_test();
        let mut person = crate::test_utils::sample_person_from_brp();
        let person_id = person.id;
        // The BRP disagrees on the place of residence.
        person.personal_data.place_of_residence = Some("Amsterdam".parse().unwrap());
        let bsn = match &person.personal_data.bsn {
            Some(crate::structs::common::BsnOrNoneConfirmed::Bsn(bsn)) => bsn.to_exposed_string(),
            _ => panic!("the fixture has a BSN"),
        };
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id];
        store.add_person(person);
        store.add_candidate_list(list);
        let stream_id = store.stream_id;

        let stub = BrpStub::serving(vec![matching_record(&bsn)]).await;
        let mut state = crate::AppState::new_for_tests().await;
        state.brp_client = stub.client.clone();

        let response = check_against_brp(
            CsbCandidateBrpCheckPath {
                stream_id,
                list_id,
                person_id,
            },
            axum::extract::State(state),
            store.clone(),
        )
        .await
        .unwrap();

        assert!(
            response.status().is_redirection(),
            "{:?}",
            response.status()
        );
        assert!(store.is_brp_checked(person_id));
        assert_eq!(
            store.get_brp_findings_for_person(person_id),
            vec![crate::structs::brp::BrpFinding::Mismatch {
                brp_value: crate::structs::brp::BrpValue::PlaceOfResidence(
                    "Utrecht".parse().unwrap()
                ),
            }]
        );
    }

    #[tokio::test]
    async fn an_unchecked_candidate_is_offered_a_check_of_their_own() {
        let (store, list_id, person_id) = store_with_candidate();
        let stream_id = store.stream_id;

        let body = examination_body(store, list_id, person_id).await;

        assert!(body.contains(&format!(
            "/csb/examination/{stream_id}/list/{list_id}/candidate/{person_id}/brp-check"
        )));
        assert!(body.contains("Check this candidate against the BRP"));
    }

    #[tokio::test]
    async fn a_candidate_waiting_for_a_running_sweep_is_offered_a_refresh_instead() {
        let (store, list_id, person_id) = store_with_candidate();
        let stream_id = store.stream_id;
        store
            .update(CsbAction::SetBrpStatus(BrpStatus::in_progress()))
            .await
            .unwrap();
        let _sweep = claim_sweep_for_test(stream_id);

        let body = examination_body(store, list_id, person_id).await;

        assert!(body.contains("Refresh this page"), "{body}");
        // A check of their own would only ask the BRP the same thing twice.
        assert!(!body.contains(&format!(
            "/csb/examination/{stream_id}/list/{list_id}/candidate/{person_id}/brp-check"
        )));
    }

    /// The BRP check belongs to the examination, so the recovery phase shows
    /// the candidate's data without it.
    #[tokio::test]
    async fn recovery_mode_leaves_the_brp_out_of_the_candidate_page() {
        let (store, list_id, person_id) = store_with_candidate();
        store
            .update(CsbAction::BrpPersonChecked {
                person: person_id,
                findings: vec![BrpFinding::NotDutch],
            })
            .await
            .unwrap();

        let response = render(
            list_id,
            person_id,
            CsbContext::new_test(),
            store,
            CsbPhase::Recovery,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(!body.contains("BRP"), "{body}");
        assert!(!body.contains("brp-check"), "{body}");
    }

    /// The sweep that never came back: its status is still `InProgress`, but
    /// nothing is running, so waiting for it is pointless.
    #[tokio::test]
    async fn a_candidate_left_behind_by_an_abandoned_sweep_is_offered_a_check() {
        let (store, list_id, person_id) = store_with_candidate();
        let stream_id = store.stream_id;
        store
            .update(CsbAction::SetBrpStatus(BrpStatus::in_progress()))
            .await
            .unwrap();

        let body = examination_body(store, list_id, person_id).await;

        assert!(!body.contains("Refresh this page"), "{body}");
        assert!(!body.contains("still running"), "{body}");
        assert!(
            body.contains("Check this candidate against the BRP"),
            "{body}"
        );
        assert!(body.contains(&format!(
            "/csb/examination/{stream_id}/list/{list_id}/candidate/{person_id}/brp-check"
        )));
    }

    #[tokio::test]
    async fn a_candidate_the_brp_already_answered_for_is_not_offered_another_check() {
        let (store, list_id, person_id) = store_with_candidate();
        let stream_id = store.stream_id;
        store
            .update(CsbAction::BrpPersonChecked {
                person: person_id,
                findings: vec![BrpFinding::NotDutch],
            })
            .await
            .unwrap();

        let body = examination_body(store, list_id, person_id).await;

        assert!(!body.contains(&format!(
            "/csb/examination/{stream_id}/list/{list_id}/candidate/{person_id}/brp-check"
        )));
    }

    #[tokio::test]
    async fn a_candidate_corrected_after_the_check_is_told_why_they_are_unchecked() {
        let (store, list_id, person_id) = store_with_candidate();
        store
            .update(CsbAction::BrpPersonChecked {
                person: person_id,
                findings: Vec::new(),
            })
            .await
            .unwrap();
        store
            .update(CsbAction::SetBrpStatus(
                crate::structs::brp::BrpStatus::Finished,
            ))
            .await
            .unwrap();
        // Correcting the candidate drops what the BRP said about the old value.
        store
            .update(CsbAction::UpdateCorrection(
                crate::structs::csb::Correction::Person(
                    person_id,
                    crate::structs::csb::PersonCorrection::LastName(
                        "Gecorrigeerd".parse().unwrap(),
                    ),
                ),
            ))
            .await
            .unwrap();

        let body = examination_body(store, list_id, person_id).await;

        assert!(
            body.contains("changed after the check against the BRP"),
            "{body}"
        );
        assert!(body.contains("Check this candidate against the BRP"));
    }

    #[tokio::test]
    async fn renders_candidate_details_and_add_omission_buttons() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let person = sample_person(PersonId::new());
        let person_id = person.id;
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id];
        store.add_person(person);
        store.add_candidate_list(list);

        let response = overview(
            CsbCandidatePath {
                stream_id,
                list_id,
                person_id,
            },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // The candidate's imported details render.
        assert!(body.contains("Jansen"));
        assert!(body.contains("Juinen"));
        // The add-omission button targets the candidate omission dialog,
        // carrying the list so the candidate's position can be resolved.
        assert!(body.contains(&format!(
            "/csb/examination/{stream_id}/omission/candidate/{person_id}"
        )));
        assert!(body.contains(&format!("list={list_id}")));
        // The header shows the electoral districts of the candidate's list
        // (the sample list covers Utrecht).
        assert!(body.contains("Electoral districts"));
        assert!(body.contains("Utrecht"));
        // date of birth formatted correctly
        assert!(body.contains("01-02-1990"))
    }

    #[tokio::test]
    async fn shows_bsn_house_number_addition_and_representative_corrections() {
        use crate::{
            structs::{common::BsnOrNoneConfirmed, persons::Representative},
            test_utils::{sample_dutch_address, sample_full_name},
        };

        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let mut person = sample_person(PersonId::new());
        person.personal_data.bsn = Some(BsnOrNoneConfirmed::Bsn("999995972".parse().unwrap()));
        person.representative = Some(Representative {
            name: sample_full_name(None, "Gemachtigde", None, "G.G."),
            address: sample_dutch_address("Den Haag", "2513 AA", "1", "B", "Plein"),
        });
        let person_id = person.id;
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id];
        store.add_person(person.clone());
        store.add_candidate_list(list);

        // The corrections change the BSN, the candidate's house number
        // addition and the representative's last name.
        let mut corrected = person;
        corrected.personal_data.bsn = Some(BsnOrNoneConfirmed::NoneConfirmed);
        corrected.address.house_number_addition = Some("C".parse().unwrap());
        corrected.representative.as_mut().unwrap().name =
            sample_full_name(None, "Opvolger", None, "G.G.");
        store
            .data
            .write()
            .paper_corrected_data
            .persons
            .insert(person_id, corrected);

        let response = overview(
            CsbCandidatePath {
                stream_id,
                list_id,
                person_id,
            },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // The corrected BSN renders next to the struck-through imported one.
        assert!(body.contains(r#"<s class="imported-value">999995972</s>"#));
        // The corrected house number addition is highlighted.
        assert!(body.contains(r#"<strong class="paper-corrected-value">C</strong>"#));
        // The representative table renders with the corrected name.
        assert!(body.contains("Authorised person"));
        assert!(body.contains("Gemachtigde"));
        assert!(body.contains("Opvolger"));
    }

    #[tokio::test]
    async fn shows_corrected_electoral_districts() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let person = sample_person(PersonId::new());
        let person_id = person.id;
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id];
        store.add_person(person);
        store.add_candidate_list(list.clone());
        let mut corrected = list;
        corrected.electoral_districts = vec![ElectoralDistrict::Groningen];
        store.set_paper_corrected_candidate_list(corrected);

        let response = overview(
            CsbCandidatePath {
                stream_id,
                list_id,
                person_id,
            },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // The corrected districts replace the imported ones.
        assert!(body.contains("Groningen"));
        assert!(!body.contains("Utrecht"));
    }

    #[tokio::test]
    async fn renders_the_corrected_position_when_it_differs() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let person = sample_person(PersonId::new());
        let person_id = person.id;
        let other = sample_person(PersonId::new());
        let other_id = other.id;
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id, other_id];
        store.add_person(person);
        store.add_person(other);
        store.add_candidate_list(list.clone());

        // The corrections move the candidate from position 1 to 2.
        list.candidates = vec![other_id, person_id];
        store.set_paper_corrected_candidate_list(list);

        let response = overview(
            CsbCandidatePath {
                stream_id,
                list_id,
                person_id,
            },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // The imported position renders struck through, followed by the
        // corrected position badge.
        assert!(
            body.contains(
                r#"<s class="badge position-badge imported-value candidate-number">1</s>"#
            )
        );
        assert!(body.contains("paper-corrected-value"));
    }

    #[tokio::test]
    async fn renders_added_candidate_omissions_as_badges() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let person = sample_person(PersonId::new());
        let person_id = person.id;
        let list_id = CandidateListId::new();
        store.add_person(person);
        store.add_candidate_list(sample_candidate_list(list_id));

        Omission::new(
            OmissionCategory::Candidate {
                person: person_id,
                lists: vec![list_id],
            },
            "Missing consent".parse().unwrap(),
            "The declaration of consent is missing.".parse().unwrap(),
            None,
        )
        .create(&store)
        .await
        .unwrap();

        let response = overview(
            CsbCandidatePath {
                stream_id,
                list_id,
                person_id,
            },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("restoration-tag"));
        assert!(body.contains("Missing consent"));
    }

    #[tokio::test]
    async fn returns_not_found_for_unknown_candidate() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let result = overview(
            CsbCandidatePath {
                stream_id,
                list_id: CandidateListId::new(),
                person_id: PersonId::new(),
            },
            CsbContext::new_test(),
            store,
        )
        .await;

        assert!(matches!(result, Err(AppError::GenericNotFound)));
    }

    #[tokio::test]
    async fn returns_not_found_for_unknown_list() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        // A known candidate but an unknown list: the person lookup succeeds,
        // so the handler fails when resolving the list's electoral districts.
        let person = sample_person(PersonId::new());
        let person_id = person.id;
        store.add_person(person);

        let result = overview(
            CsbCandidatePath {
                stream_id,
                list_id: CandidateListId::new(),
                person_id,
            },
            CsbContext::new_test(),
            store,
        )
        .await;

        assert!(matches!(result, Err(AppError::GenericNotFound)));
    }
}
