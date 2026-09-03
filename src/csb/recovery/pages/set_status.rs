use axum::{extract::Query, response::Response};
use serde::Deserialize;

use crate::{
    AppError, CsbContext, CsbStore, ElectoralDistrict, Form, QueryParamState,
    csb::{examination::extractors::CsbPoliticalGroup, recovery::paths::CsbSetOmissionStatusPath},
    structs::{
        candidate_lists::CandidateListId,
        csb::{CsbPhase, OmissionPart, OmissionStatus},
    },
};

#[derive(Deserialize)]
pub struct OmissionStatusForm {
    status: OmissionStatusFormValue,
    /// Set by the per-part controls; both absent when the decision applies to
    /// the omission as a whole.
    #[serde(default)]
    electoral_district: Option<ElectoralDistrict>,
    #[serde(default)]
    candidate_list: Option<CandidateListId>,
}

impl OmissionStatusForm {
    fn part(&self) -> Option<OmissionPart> {
        self.electoral_district
            .map(OmissionPart::ElectoralDistrict)
            .or(self.candidate_list.map(OmissionPart::CandidateList))
    }
}

/// The submitted decision. A dedicated form enum keeps the kebab-case form
/// values separate from the persisted event encoding of [`OmissionStatus`].
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum OmissionStatusFormValue {
    Recovered,
    NotRecovered,
}

/// Record whether an omission was recovered, returning to the page the
/// control was on. A decision on one part of it (an electoral district, or a
/// candidate list) splits it, or merges the part into an omission with the
/// same details decided the same way. Irreparable omissions are rejected by
/// [`Omission::set_status`](crate::structs::csb::Omission).
pub async fn set_status(
    CsbSetOmissionStatusPath { omission_id, .. }: CsbSetOmissionStatusPath,
    _context: CsbContext,
    store: CsbStore,
    Query(query): Query<QueryParamState>,
    Form(form): Form<OmissionStatusForm>,
) -> Result<Response, AppError> {
    let omission = store.get_omission(omission_id)?;
    let status = match form.status {
        OmissionStatusFormValue::Recovered => OmissionStatus::Recovered,
        OmissionStatusFormValue::NotRecovered => OmissionStatus::NotRecovered,
    };
    match form.part() {
        Some(part) => omission.set_part_status(&store, part, status).await?,
        None => omission.set_status(&store, status).await?,
    }

    let political_group =
        CsbPoliticalGroup::new_from_csb_store(&store).with_mode(CsbPhase::Recovery);
    Ok(query.redirect_or(political_group.all_restorations_path()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::StatusCode, response::IntoResponse};

    use crate::{
        structs::{
            candidate_lists::CandidateList,
            csb::{Omission, OmissionCategory, OmissionId, sample_omission},
            persons::PersonId,
        },
        test_utils::sample_candidate_list,
    };

    fn declarations_of_support(districts: Vec<ElectoralDistrict>) -> Omission {
        Omission::new(
            OmissionCategory::DeclarationsOfSupport(districts),
            "Declarations of support missing".parse().unwrap(),
            "Too few declarations of support were handed in."
                .parse()
                .unwrap(),
            None,
        )
    }

    /// A candidate on `lists`, with one omission covering all of them.
    fn candidate_on_lists(
        store: &CsbStore,
        districts: Vec<Vec<ElectoralDistrict>>,
    ) -> (PersonId, Vec<CandidateListId>, Omission) {
        let person = PersonId::new();
        let lists: Vec<CandidateListId> = districts
            .into_iter()
            .map(|districts| {
                let list = CandidateList {
                    electoral_districts: districts,
                    candidates: vec![person],
                    ..sample_candidate_list(CandidateListId::new())
                };
                let id = list.id;
                store.add_candidate_list(list);
                id
            })
            .collect();

        let omission = Omission::new(
            OmissionCategory::Candidate {
                person,
                lists: lists.clone(),
            },
            "Missing consent".parse().unwrap(),
            "The declaration of consent is missing.".parse().unwrap(),
            None,
        );
        (person, lists, omission)
    }

    /// One omission covering `lists`, reported on the lists themselves.
    fn on_lists(lists: Vec<CandidateListId>) -> Omission {
        Omission::new(
            OmissionCategory::CandidateList(lists),
            "Too many candidates".parse().unwrap(),
            "The list holds more candidates than allowed."
                .parse()
                .unwrap(),
            None,
        )
    }

    fn form(value: OmissionStatusFormValue) -> Form<OmissionStatusForm> {
        Form(OmissionStatusForm {
            status: value,
            electoral_district: None,
            candidate_list: None,
        })
    }

    fn district_form(
        value: OmissionStatusFormValue,
        district: ElectoralDistrict,
    ) -> Form<OmissionStatusForm> {
        Form(OmissionStatusForm {
            electoral_district: Some(district),
            ..form(value).0
        })
    }

    fn list_form(
        value: OmissionStatusFormValue,
        list_id: CandidateListId,
    ) -> Form<OmissionStatusForm> {
        Form(OmissionStatusForm {
            candidate_list: Some(list_id),
            ..form(value).0
        })
    }

    #[tokio::test]
    async fn records_the_decision_and_redirects_to_the_todo_page() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let omission = sample_omission(OmissionCategory::PoliticalGroup);
        omission.create(&store).await.unwrap();

        let response = set_status(
            CsbSetOmissionStatusPath {
                stream_id,
                omission_id: omission.id,
            },
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            form(OmissionStatusFormValue::Recovered),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            store.get_omission(omission.id).unwrap().status,
            OmissionStatus::Recovered
        );
        let location = response
            .headers()
            .get("Location")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(location.contains(&format!("/csb/recovery/{stream_id}/omissions")));
    }

    #[tokio::test]
    async fn honours_the_redirect_to() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let omission = sample_omission(OmissionCategory::PoliticalGroup);
        omission.create(&store).await.unwrap();

        let response = set_status(
            CsbSetOmissionStatusPath {
                stream_id,
                omission_id: omission.id,
            },
            CsbContext::new_test(),
            store,
            Query(QueryParamState::redirect_to("/back/here".to_string())),
            form(OmissionStatusFormValue::NotRecovered),
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
    async fn rejects_irreparable_omissions() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let mut omission = sample_omission(OmissionCategory::PoliticalGroup);
        omission.recoverable = false;
        omission.create(&store).await.unwrap();

        let result = set_status(
            CsbSetOmissionStatusPath {
                stream_id,
                omission_id: omission.id,
            },
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            form(OmissionStatusFormValue::Recovered),
        )
        .await;

        assert!(result.is_err());
        assert_eq!(
            store.get_omission(omission.id).unwrap().status,
            OmissionStatus::Pending
        );
    }

    #[tokio::test]
    async fn a_district_decision_splits_a_multi_district_omission() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let omission = declarations_of_support(vec![
            ElectoralDistrict::Groningen,
            ElectoralDistrict::Fryslan,
            ElectoralDistrict::Utrecht,
        ]);
        omission.create(&store).await.unwrap();

        // Three districts, so three decisions.
        assert_eq!(store.get_actionable_omission_count(), 3);
        assert_eq!(store.get_pending_omission_count(), 3);

        set_status(
            CsbSetOmissionStatusPath {
                stream_id,
                omission_id: omission.id,
            },
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            district_form(
                OmissionStatusFormValue::NotRecovered,
                ElectoralDistrict::Fryslan,
            ),
        )
        .await
        .unwrap();

        // The omission keeps the districts still waiting.
        let original = store.get_omission(omission.id).unwrap();
        assert_eq!(
            original.category,
            OmissionCategory::DeclarationsOfSupport(vec![
                ElectoralDistrict::Groningen,
                ElectoralDistrict::Utrecht
            ])
        );
        assert_eq!(original.status, OmissionStatus::Pending);

        // Fryslân is split off with the decision, keeping the text.
        let all = store.get_all_declarations_of_support_omissions();
        assert_eq!(all.len(), 2);
        // Ordered by first district: Groningen (1) before Fryslân (2).
        assert_eq!(all[0].id, omission.id);
        let split = &all[1];
        assert_eq!(
            split.category,
            OmissionCategory::DeclarationsOfSupport(vec![ElectoralDistrict::Fryslan])
        );
        assert_eq!(split.status, OmissionStatus::NotRecovered);
        assert_eq!(split.title, omission.title);
        assert_eq!(split.description, omission.description);

        // Counting per district keeps the progress from jumping.
        assert_eq!(store.get_actionable_omission_count(), 3);
        assert_eq!(store.get_pending_omission_count(), 2);

        // Only the unrecovered district is scrapped.
        assert_eq!(
            store.get_scrapped_districts(),
            vec![ElectoralDistrict::Fryslan]
        );
    }

    #[tokio::test]
    async fn the_last_district_is_decided_on_the_omission_itself() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let omission = declarations_of_support(vec![ElectoralDistrict::Groningen]);
        omission.create(&store).await.unwrap();

        set_status(
            CsbSetOmissionStatusPath {
                stream_id,
                omission_id: omission.id,
            },
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            district_form(
                OmissionStatusFormValue::Recovered,
                ElectoralDistrict::Groningen,
            ),
        )
        .await
        .unwrap();

        // Nothing is split off.
        assert_eq!(store.get_omission_count(), 1);
        assert_eq!(
            store.get_omission(omission.id).unwrap().status,
            OmissionStatus::Recovered
        );
        assert!(store.get_scrapped_districts().is_empty());
    }

    #[tokio::test]
    async fn rejects_a_district_the_omission_was_not_reported_for() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let omission = declarations_of_support(vec![
            ElectoralDistrict::Groningen,
            ElectoralDistrict::Fryslan,
        ]);
        omission.create(&store).await.unwrap();

        let result = set_status(
            CsbSetOmissionStatusPath {
                stream_id,
                omission_id: omission.id,
            },
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            district_form(
                OmissionStatusFormValue::Recovered,
                ElectoralDistrict::Utrecht,
            ),
        )
        .await;

        assert!(result.is_err());
        assert_eq!(store.get_omission_count(), 1);
        assert_eq!(
            store.get_omission(omission.id).unwrap().status,
            OmissionStatus::Pending
        );
    }

    #[tokio::test]
    async fn each_district_of_a_split_omission_keeps_its_own_decision() {
        let store = CsbStore::new_for_test();
        let omission = declarations_of_support(vec![
            ElectoralDistrict::Groningen,
            ElectoralDistrict::Fryslan,
            ElectoralDistrict::Utrecht,
        ]);
        omission.create(&store).await.unwrap();

        // Recover every district but Fryslân.
        for (district, decision) in [
            (
                ElectoralDistrict::Groningen,
                OmissionStatusFormValue::Recovered,
            ),
            (
                ElectoralDistrict::Fryslan,
                OmissionStatusFormValue::NotRecovered,
            ),
            (
                ElectoralDistrict::Utrecht,
                OmissionStatusFormValue::Recovered,
            ),
        ] {
            decide_district(&store, district, decision).await;
        }

        // All decided, only Fryslân scrapped.
        assert_eq!(store.get_actionable_omission_count(), 3);
        assert_eq!(store.get_pending_omission_count(), 0);
        assert_eq!(
            store.get_scrapped_districts(),
            vec![ElectoralDistrict::Fryslan]
        );

        // The districts decided the same way read as one omission again.
        let all = store.get_all_declarations_of_support_omissions();
        assert_eq!(all.len(), 2);
        assert_eq!(
            all[0].category,
            OmissionCategory::DeclarationsOfSupport(vec![
                ElectoralDistrict::Groningen,
                ElectoralDistrict::Utrecht
            ])
        );
        assert_eq!(all[0].status, OmissionStatus::Recovered);
        assert_eq!(
            all[1].category,
            OmissionCategory::DeclarationsOfSupport(vec![ElectoralDistrict::Fryslan])
        );
        assert_eq!(all[1].status, OmissionStatus::NotRecovered);
    }

    /// Decide `district` on whichever part covers it.
    async fn decide_district(
        store: &CsbStore,
        district: ElectoralDistrict,
        decision: OmissionStatusFormValue,
    ) {
        let omission = store
            .get_all_declarations_of_support_omissions()
            .into_iter()
            .find(|o| o.electoral_districts(&store.election).contains(&district))
            .expect("every district stays covered by one of the parts");

        set_status(
            CsbSetOmissionStatusPath {
                stream_id: store.stream_id,
                omission_id: omission.id,
            },
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            district_form(decision, district),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn the_last_district_decided_the_same_way_merges_the_parts_back() {
        let store = CsbStore::new_for_test();
        let omission = declarations_of_support(vec![
            ElectoralDistrict::Groningen,
            ElectoralDistrict::Fryslan,
        ]);
        omission.create(&store).await.unwrap();

        decide_district(
            &store,
            ElectoralDistrict::Groningen,
            OmissionStatusFormValue::Recovered,
        )
        .await;
        assert_eq!(store.get_omission_count(), 2);

        decide_district(
            &store,
            ElectoralDistrict::Fryslan,
            OmissionStatusFormValue::Recovered,
        )
        .await;

        // The split-off part took the last district over; the original is gone.
        let all = store.get_all_declarations_of_support_omissions();
        assert_eq!(all.len(), 1);
        assert_ne!(all[0].id, omission.id);
        assert_eq!(
            all[0].category,
            OmissionCategory::DeclarationsOfSupport(vec![
                ElectoralDistrict::Groningen,
                ElectoralDistrict::Fryslan
            ])
        );
        assert_eq!(all[0].status, OmissionStatus::Recovered);
        assert!(store.get_omission(omission.id).is_err());

        // Still two decisions, both made.
        assert_eq!(store.get_actionable_omission_count(), 2);
        assert_eq!(store.get_pending_omission_count(), 0);
    }

    #[tokio::test]
    async fn changing_a_decision_moves_the_district_to_the_part_holding_it() {
        let store = CsbStore::new_for_test();
        declarations_of_support(vec![
            ElectoralDistrict::Groningen,
            ElectoralDistrict::Fryslan,
            ElectoralDistrict::Utrecht,
        ])
        .create(&store)
        .await
        .unwrap();
        for (district, decision) in [
            (
                ElectoralDistrict::Groningen,
                OmissionStatusFormValue::Recovered,
            ),
            (
                ElectoralDistrict::Fryslan,
                OmissionStatusFormValue::NotRecovered,
            ),
            (
                ElectoralDistrict::Utrecht,
                OmissionStatusFormValue::Recovered,
            ),
        ] {
            decide_district(&store, district, decision).await;
        }

        // Groningen turns out not recovered after all.
        decide_district(
            &store,
            ElectoralDistrict::Groningen,
            OmissionStatusFormValue::NotRecovered,
        )
        .await;

        // It joins Fryslân, in the election's district order.
        let all = store.get_all_declarations_of_support_omissions();
        assert_eq!(all.len(), 2);
        assert_eq!(
            all[0].category,
            OmissionCategory::DeclarationsOfSupport(vec![
                ElectoralDistrict::Groningen,
                ElectoralDistrict::Fryslan
            ])
        );
        assert_eq!(all[0].status, OmissionStatus::NotRecovered);
        assert_eq!(
            all[1].category,
            OmissionCategory::DeclarationsOfSupport(vec![ElectoralDistrict::Utrecht])
        );
        assert_eq!(all[1].status, OmissionStatus::Recovered);
        assert_eq!(
            store.get_scrapped_districts(),
            vec![ElectoralDistrict::Groningen, ElectoralDistrict::Fryslan]
        );
    }

    #[tokio::test]
    async fn deciding_a_district_the_way_it_already_is_changes_nothing() {
        let store = CsbStore::new_for_test();
        declarations_of_support(vec![
            ElectoralDistrict::Groningen,
            ElectoralDistrict::Utrecht,
        ])
        .create(&store)
        .await
        .unwrap();
        for district in [ElectoralDistrict::Groningen, ElectoralDistrict::Utrecht] {
            decide_district(&store, district, OmissionStatusFormValue::Recovered).await;
        }
        let before = store.get_all_declarations_of_support_omissions();
        assert_eq!(before.len(), 1);

        // Confirming one district must not split it off again.
        decide_district(
            &store,
            ElectoralDistrict::Groningen,
            OmissionStatusFormValue::Recovered,
        )
        .await;

        let after = store.get_all_declarations_of_support_omissions();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].id, before[0].id);
        assert_eq!(after[0].category, before[0].category);
        assert_eq!(after[0].status, before[0].status);
    }

    /// Decide an omission as a whole, the way the controls of a single-part
    /// omission submit it.
    async fn decide_whole(
        store: &CsbStore,
        omission_id: OmissionId,
        decision: OmissionStatusFormValue,
    ) {
        set_status(
            CsbSetOmissionStatusPath {
                stream_id: store.stream_id,
                omission_id,
            },
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            form(decision),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn separately_reported_omissions_with_the_same_details_merge_once_decided_alike() {
        let store = CsbStore::new_for_test();
        let groningen = declarations_of_support(vec![ElectoralDistrict::Groningen]);
        let fryslan = declarations_of_support(vec![ElectoralDistrict::Fryslan]);
        groningen.create(&store).await.unwrap();
        fryslan.create(&store).await.unwrap();

        decide_whole(&store, groningen.id, OmissionStatusFormValue::Recovered).await;
        // Decided differently, so they stay apart.
        decide_whole(&store, fryslan.id, OmissionStatusFormValue::NotRecovered).await;
        assert_eq!(store.get_omission_count(), 2);

        decide_whole(&store, fryslan.id, OmissionStatusFormValue::Recovered).await;

        // Fryslân joined the omission decided first.
        assert_eq!(store.get_omission_count(), 1);
        let merged = store.get_omission(groningen.id).unwrap();
        assert_eq!(
            merged.category,
            OmissionCategory::DeclarationsOfSupport(vec![
                ElectoralDistrict::Groningen,
                ElectoralDistrict::Fryslan
            ])
        );
        assert_eq!(merged.status, OmissionStatus::Recovered);
        assert!(store.get_omission(fryslan.id).is_err());
    }

    #[tokio::test]
    async fn omissions_with_different_details_stay_apart() {
        let store = CsbStore::new_for_test();
        declarations_of_support(vec![ElectoralDistrict::Groningen])
            .create(&store)
            .await
            .unwrap();
        Omission::new(
            OmissionCategory::DeclarationsOfSupport(vec![ElectoralDistrict::Fryslan]),
            "Declarations of support invalid".parse().unwrap(),
            "The declarations of support handed in are invalid."
                .parse()
                .unwrap(),
            None,
        )
        .create(&store)
        .await
        .unwrap();

        for district in [ElectoralDistrict::Groningen, ElectoralDistrict::Fryslan] {
            decide_district(&store, district, OmissionStatusFormValue::Recovered).await;
        }

        assert_eq!(store.get_omission_count(), 2);
    }

    #[tokio::test]
    async fn lists_decided_the_same_way_merge_back_in_district_order() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        // The Utrecht list comes first, so the merge has to reorder.
        let (person, lists, omission) = candidate_on_lists(
            &store,
            vec![
                vec![ElectoralDistrict::Utrecht],
                vec![ElectoralDistrict::Groningen],
            ],
        );
        omission.create(&store).await.unwrap();

        for list_id in [lists[0], lists[1]] {
            let omission = store
                .get_candidate_omissions(person)
                .into_iter()
                .find(|o| o.candidate_lists().contains(&list_id))
                .expect("every list stays covered by one of the parts");
            set_status(
                CsbSetOmissionStatusPath {
                    stream_id,
                    omission_id: omission.id,
                },
                CsbContext::new_test(),
                store.clone(),
                Query(QueryParamState::default()),
                list_form(OmissionStatusFormValue::Recovered, list_id),
            )
            .await
            .unwrap();
        }

        let all = store.get_candidate_omissions(person);
        assert_eq!(all.len(), 1);
        assert_eq!(
            all[0].category,
            OmissionCategory::Candidate {
                person,
                lists: vec![lists[1], lists[0]],
            }
        );
        assert_eq!(all[0].status, OmissionStatus::Recovered);
        assert_eq!(store.get_actionable_omission_count(), 2);
        assert_eq!(store.get_pending_omission_count(), 0);
        assert!(!store.is_candidate_scrapped(person, lists[0]));
        assert!(!store.is_candidate_scrapped(person, lists[1]));
    }

    #[tokio::test]
    async fn a_list_decision_splits_a_multi_list_candidate_omission() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let (person, lists, omission) = candidate_on_lists(
            &store,
            vec![
                vec![ElectoralDistrict::Groningen],
                vec![ElectoralDistrict::Utrecht],
            ],
        );
        omission.create(&store).await.unwrap();

        // Two lists, so two decisions.
        assert_eq!(store.get_actionable_omission_count(), 2);
        assert_eq!(store.get_pending_omission_count(), 2);

        set_status(
            CsbSetOmissionStatusPath {
                stream_id,
                omission_id: omission.id,
            },
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            list_form(OmissionStatusFormValue::NotRecovered, lists[1]),
        )
        .await
        .unwrap();

        // The omission keeps the list still waiting.
        assert_eq!(
            store.get_omission(omission.id).unwrap().category,
            OmissionCategory::Candidate {
                person,
                lists: vec![lists[0]],
            }
        );

        // The Utrecht list is split off with the decision.
        let all = store.get_candidate_omissions(person);
        assert_eq!(all.len(), 2);
        // Ordered by their list's first district: Groningen (1) before Utrecht (7).
        assert_eq!(all[0].id, omission.id);
        assert_eq!(
            all[1].category,
            OmissionCategory::Candidate {
                person,
                lists: vec![lists[1]],
            }
        );
        assert_eq!(all[1].status, OmissionStatus::NotRecovered);
        assert_eq!(all[1].title, omission.title);

        assert_eq!(store.get_actionable_omission_count(), 2);
        assert_eq!(store.get_pending_omission_count(), 1);

        // The candidate is scrapped from the Utrecht list only.
        assert!(!store.is_candidate_scrapped(person, lists[0]));
        assert!(store.is_candidate_scrapped(person, lists[1]));
    }

    #[tokio::test]
    async fn the_last_list_is_decided_on_the_omission_itself() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let (person, lists, omission) =
            candidate_on_lists(&store, vec![vec![ElectoralDistrict::Groningen]]);
        omission.create(&store).await.unwrap();

        set_status(
            CsbSetOmissionStatusPath {
                stream_id,
                omission_id: omission.id,
            },
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            list_form(OmissionStatusFormValue::Recovered, lists[0]),
        )
        .await
        .unwrap();

        assert_eq!(store.get_omission_count(), 1);
        assert_eq!(
            store.get_omission(omission.id).unwrap().status,
            OmissionStatus::Recovered
        );
        assert!(!store.is_candidate_scrapped(person, lists[0]));
    }

    #[tokio::test]
    async fn rejects_a_list_the_omission_was_not_reported_for() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let (_, _, omission) = candidate_on_lists(
            &store,
            vec![
                vec![ElectoralDistrict::Groningen],
                vec![ElectoralDistrict::Utrecht],
            ],
        );
        omission.create(&store).await.unwrap();

        let result = set_status(
            CsbSetOmissionStatusPath {
                stream_id,
                omission_id: omission.id,
            },
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            list_form(OmissionStatusFormValue::Recovered, CandidateListId::new()),
        )
        .await;

        assert!(result.is_err());
        assert_eq!(store.get_omission_count(), 1);
    }

    #[tokio::test]
    async fn a_district_decision_does_not_apply_to_a_candidate_omission() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let (_, _, omission) = candidate_on_lists(
            &store,
            vec![
                vec![ElectoralDistrict::Groningen],
                vec![ElectoralDistrict::Utrecht],
            ],
        );
        omission.create(&store).await.unwrap();

        let result = set_status(
            CsbSetOmissionStatusPath {
                stream_id,
                omission_id: omission.id,
            },
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            district_form(
                OmissionStatusFormValue::Recovered,
                ElectoralDistrict::Groningen,
            ),
        )
        .await;

        assert!(result.is_err());
        assert_eq!(store.get_omission_count(), 1);
    }

    #[tokio::test]
    async fn a_list_decision_splits_a_multi_list_candidate_list_omission() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let (_, lists, _) = candidate_on_lists(
            &store,
            vec![
                vec![ElectoralDistrict::Groningen],
                vec![ElectoralDistrict::Utrecht],
            ],
        );
        let omission = on_lists(lists.clone());
        omission.create(&store).await.unwrap();

        assert_eq!(store.get_actionable_omission_count(), 2);

        set_status(
            CsbSetOmissionStatusPath {
                stream_id,
                omission_id: omission.id,
            },
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            list_form(OmissionStatusFormValue::NotRecovered, lists[1]),
        )
        .await
        .unwrap();

        // The omission keeps the list still waiting...
        assert_eq!(
            store.get_omission(omission.id).unwrap().category,
            OmissionCategory::CandidateList(vec![lists[0]])
        );
        // ...and the Utrecht list is split off with the decision.
        let split = store.get_candidate_list_omissions(lists[1]).unwrap();
        assert_eq!(split.len(), 1);
        assert_eq!(
            split[0].category,
            OmissionCategory::CandidateList(vec![lists[1]])
        );
        assert_eq!(split[0].status, OmissionStatus::NotRecovered);

        assert_eq!(store.get_actionable_omission_count(), 2);
        assert_eq!(store.get_pending_omission_count(), 1);

        // Only the Utrecht list is scrapped.
        assert!(!store.is_candidate_list_scrapped(lists[0]).unwrap());
        assert!(store.is_candidate_list_scrapped(lists[1]).unwrap());
    }

    #[tokio::test]
    async fn errors_for_an_unknown_omission() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let result = set_status(
            CsbSetOmissionStatusPath {
                stream_id,
                omission_id: OmissionId::new(),
            },
            CsbContext::new_test(),
            store,
            Query(QueryParamState::default()),
            form(OmissionStatusFormValue::Recovered),
        )
        .await;

        assert!(matches!(result, Err(AppError::GenericNotFound)));
    }
}
