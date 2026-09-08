//! Store-backed operations for omissions.

use std::collections::BTreeMap;

use crate::{
    AppError, CsbAction, CsbStore, CsbStoreData, CsbStream, ElectionConfig, ElectoralDistrict,
    models::{
        i1::{DistrictLists, SubmittedList},
        i4::OmissionGroup,
    },
    projection::WithCorrections,
    store::StoreRegistry,
    structs::csb::{Omission, OmissionCategory, OmissionStatus},
};

const ALL_DISTRICTS: &str = "alle kieskringen";

/// The submitted candidate lists per electoral district, as listed in the
/// "Kandidatenlijsten" section of model I 1: one row per list and district,
/// with the political group's appellation, its first candidate and the number
/// of names on the list.
///
/// A list that covers several districts contributes a row to each of them.
/// Districts come out in electoral-district order, the rows within a district
/// in store scope order and then by list creation date.
pub async fn submitted_lists(
    registry: &StoreRegistry<CsbStoreData>,
    election: &ElectionConfig,
) -> Result<Vec<DistrictLists>, AppError> {
    let mut by_district: BTreeMap<ElectoralDistrict, Vec<SubmittedList>> = BTreeMap::new();
    for store in registry.stores_for_election(*election).await? {
        for (district, list) in store_submitted_lists(&store) {
            by_district.entry(district).or_default().push(list);
        }
    }

    Ok(by_district
        .into_iter()
        .map(|(district, lists)| DistrictLists {
            electoral_district: district_label(district, election),
            lists,
        })
        .collect())
}

/// The rows one political group contributes to the submitted lists section,
/// paired with the district they belong to.
fn store_submitted_lists(store: &CsbStream) -> Vec<(ElectoralDistrict, SubmittedList)> {
    let appellation = store.get_appellation(WithCorrections::All);
    let mut lists = store.get_candidate_lists(WithCorrections::All);
    lists.sort_unstable_by_key(|list| list.created_at);

    let mut rows = Vec::new();
    for list in lists {
        let first_candidate_name = list
            .candidates
            .first()
            .and_then(|id| store.get_person(*id, WithCorrections::All))
            .map(|person| person.name.display())
            .unwrap_or_default();

        for district in &list.electoral_districts {
            rows.push((
                *district,
                SubmittedList {
                    appellation: appellation.clone(),
                    first_candidate_name: first_candidate_name.clone(),
                    candidate_count: list.candidates.len(),
                },
            ));
        }
    }

    rows
}

/// A single district as printed after the "Kieskring" label: its number and
/// title, e.g. `1 (Groningen)`. Elections with one district have no meaningful
/// district number, so those print the title alone.
fn district_label(district: ElectoralDistrict, election: &ElectionConfig) -> String {
    let title = district.title();
    if election.has_only_one_district() {
        title.to_string()
    } else {
        format!("{} ({})", district.region_number(), title)
    }
}

/// The recoverable omissions of every political group, as the model input
/// groups shared by models I 1 and I 4: one group per political group and
/// electoral district, in store scope order.
pub async fn found_omissions(
    registry: &StoreRegistry<CsbStoreData>,
    election: &ElectionConfig,
) -> Result<Vec<OmissionGroup>, AppError> {
    let mut found_omissions = Vec::new();
    for store in registry.stores_for_election(*election).await? {
        let recoverable = store.get_recoverable_omissions();
        if recoverable.is_empty() {
            continue;
        }

        let mut by_district: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for omission in recoverable {
            let district = omission.category.electoral_district(&store, election)?;
            by_district
                .entry(district)
                .or_default()
                .push(omission.description.to_string());
        }

        let appellation = store.get_appellation(WithCorrections::All);
        for (district, descriptions) in by_district {
            found_omissions.push(OmissionGroup {
                appellation: appellation.clone(),
                electoral_district: district,
                omission_descriptions: descriptions,
            });
        }
    }

    Ok(found_omissions)
}

impl OmissionCategory {
    /// Returns the electoral district string for use in models I 1 and I 4.
    pub fn electoral_district(
        &self,
        store: &CsbStream,
        election: &ElectionConfig,
    ) -> Result<String, AppError> {
        match self {
            OmissionCategory::PoliticalGroup | OmissionCategory::Appellation => {
                Ok(ALL_DISTRICTS.to_string())
            }
            OmissionCategory::CandidateList(lists) | OmissionCategory::Candidate { lists, .. } => {
                let mut districts: Vec<ElectoralDistrict> = Vec::new();
                for id in lists {
                    let list = store
                        .get_candidate_list(*id, crate::projection::WithCorrections::All)
                        .ok_or(AppError::GenericNotFound)?;
                    for d in list.electoral_districts {
                        if !districts.contains(&d) {
                            districts.push(d);
                        }
                    }
                }
                Ok(format_districts(&districts, election))
            }
            OmissionCategory::DeclarationsOfSupport(districts) => {
                Ok(format_districts(districts, election))
            }
        }
    }
}

fn format_districts(districts: &[ElectoralDistrict], election: &ElectionConfig) -> String {
    let all_districts = election.electoral_districts();
    if districts.is_empty() || all_districts.iter().all(|d| districts.contains(d)) {
        ALL_DISTRICTS.to_string()
    } else {
        let mut sorted = districts.to_vec();
        sorted.sort_by_key(ElectoralDistrict::region_number);
        let parts: Vec<String> = sorted
            .iter()
            .map(|d| format!("{} ({})", d.region_number(), d.title()))
            .collect();
        format!("kieskring {}", parts.join(", "))
    }
}

impl Omission {
    pub async fn create(&self, store: &CsbStore) -> Result<(), AppError> {
        store.update(CsbAction::CreateOmission(self.clone())).await
    }

    pub async fn update(&self, store: &CsbStore) -> Result<(), AppError> {
        store.update(CsbAction::UpdateOmission(self.clone())).await
    }

    pub async fn delete(&self, store: &CsbStore) -> Result<(), AppError> {
        store
            .update(CsbAction::DeleteOmission {
                omission_id: self.id,
            })
            .await
    }

    /// Record whether this omission was recovered during the "Herstelde
    /// lijsten" phase. Irreparable omissions cannot be assessed.
    pub async fn set_status(
        &self,
        store: &CsbStore,
        status: OmissionStatus,
    ) -> Result<(), AppError> {
        if !self.is_actionable() {
            return Err(AppError::UserError(
                "an irreparable omission cannot be assessed".to_string(),
            ));
        }

        store
            .update(CsbAction::SetOmissionStatus {
                omission_id: self.id,
                status,
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppRequestState, AppState, CsbStore, ElectionConfig, ElectoralDistrict, PgStoreData,
        Province, StreamId,
        structs::{
            candidate_lists::{CandidateList, CandidateListId},
            common::UtcDateTime,
            csb::sample_omission,
            list_designation::ListDesignation,
            persons::{Person, PersonId},
            political_groups::PoliticalGroup,
        },
        test_utils::{sample_person, sample_person_with},
    };

    const EK: ElectionConfig = ElectionConfig::EK27;

    fn store_with_list(districts: Vec<ElectoralDistrict>) -> (CsbStore, CandidateListId) {
        let store = CsbStore::new_for_test();
        let list = CandidateList {
            electoral_districts: districts,
            ..Default::default()
        };
        let id = list.id;
        store.add_candidate_list(list);
        (store, id)
    }

    fn utc(value: &str) -> UtcDateTime {
        value
            .parse::<chrono::DateTime<chrono::Utc>>()
            .expect("rfc3339 timestamp")
            .into()
    }

    fn named_group(appellation: &str) -> PoliticalGroup {
        PoliticalGroup {
            appellation: Some(appellation.parse().unwrap()),
            list_designation: Some(ListDesignation::Standalone),
            ..Default::default()
        }
    }

    /// Persist a CSB stream in the (in-memory) test registry carrying a single
    /// import event, so it shows up in [`StoreRegistry::stores_by_scope`].
    async fn seed_csb_store(
        state: &AppState,
        political_group: PoliticalGroup,
        persons: Vec<Person>,
        lists: Vec<CandidateList>,
    ) -> CsbStore {
        let store = state
            .csb_store_for_stream(StreamId::new(), EK)
            .await
            .unwrap()
            .acting_as_test_user();

        let mut snapshot = PgStoreData {
            political_group,
            ..PgStoreData::default()
        };
        for person in persons {
            snapshot.persons.insert(person.id, person);
        }
        for list in lists {
            snapshot.candidate_lists.insert(list.id, list);
        }

        store
            .update(CsbAction::Import {
                hash: [0u8; 32],
                source_stream_id: StreamId::new(),
                snapshot: Box::new(snapshot),
            })
            .await
            .unwrap();

        store
    }

    async fn create_omission(store: &CsbStore, category: OmissionCategory, description: &str) {
        let mut omission = sample_omission(category);
        omission.description = description.parse().unwrap();
        omission.create(store).await.unwrap();
    }

    #[test]
    fn political_group_maps_to_all_districts() {
        let store = CsbStore::new_for_test();
        assert_eq!(
            OmissionCategory::PoliticalGroup
                .electoral_district(&store, &EK)
                .unwrap(),
            "alle kieskringen"
        );
    }

    #[test]
    fn candidate_with_all_districts_maps_to_all() {
        let (store, id) = store_with_list(EK.electoral_districts().to_vec());
        let category = OmissionCategory::Candidate {
            person: crate::structs::persons::PersonId::new(),
            lists: vec![id],
        };
        assert_eq!(
            category.electoral_district(&store, &EK).unwrap(),
            "alle kieskringen"
        );
    }

    #[test]
    fn dos_all_districts_maps_to_all() {
        let store = CsbStore::new_for_test();
        assert_eq!(
            OmissionCategory::DeclarationsOfSupport(EK.electoral_districts().to_vec())
                .electoral_district(&store, &EK)
                .unwrap(),
            "alle kieskringen"
        );
    }

    #[test]
    fn dos_one_district() {
        let store = CsbStore::new_for_test();
        assert_eq!(
            OmissionCategory::DeclarationsOfSupport(vec![ElectoralDistrict::Bonaire])
                .electoral_district(&store, &EK)
                .unwrap(),
            "kieskring 13 (Bonaire)"
        );
    }

    #[test]
    fn dos_multiple_districts() {
        let store = CsbStore::new_for_test();
        // The districts should be sorted by region number.
        assert_eq!(
            OmissionCategory::DeclarationsOfSupport(vec![
                ElectoralDistrict::Drenthe,
                ElectoralDistrict::Groningen
            ])
            .electoral_district(&store, &EK)
            .unwrap(),
            "kieskring 1 (Groningen), 3 (Drenthe)"
        );
    }

    #[test]
    fn dos_no_districts_maps_to_all() {
        let store = CsbStore::new_for_test();
        // An empty district list is treated as "all districts" in format_districts.
        assert_eq!(
            OmissionCategory::DeclarationsOfSupport(vec![])
                .electoral_district(&store, &EK)
                .unwrap(),
            "alle kieskringen"
        );
    }

    #[test]
    fn candidate_with_list_specific_district() {
        let (store, id) = store_with_list(vec![ElectoralDistrict::Groningen]);
        let category = OmissionCategory::Candidate {
            person: crate::structs::persons::PersonId::new(),
            lists: vec![id],
        };
        assert_eq!(
            category.electoral_district(&store, &EK).unwrap(),
            "kieskring 1 (Groningen)"
        );
    }

    #[test]
    fn candidate_with_paper_added_list_uses_the_corrected_projection() {
        let store = CsbStore::new_for_test();
        let list = CandidateList {
            electoral_districts: vec![ElectoralDistrict::Groningen],
            ..Default::default()
        };
        let id = list.id;
        store.set_paper_corrected_candidate_list(list);
        let category = OmissionCategory::Candidate {
            person: crate::structs::persons::PersonId::new(),
            lists: vec![id],
        };

        assert_eq!(
            category.electoral_district(&store, &EK).unwrap(),
            "kieskring 1 (Groningen)"
        );
    }

    #[test]
    fn candidate_with_corrected_list_uses_the_corrected_districts() {
        let (store, id) = store_with_list(vec![ElectoralDistrict::Utrecht]);
        store.set_paper_corrected_candidate_list(CandidateList {
            id,
            electoral_districts: vec![ElectoralDistrict::Groningen],
            ..Default::default()
        });
        let category = OmissionCategory::Candidate {
            person: crate::structs::persons::PersonId::new(),
            lists: vec![id],
        };

        assert_eq!(
            category.electoral_district(&store, &EK).unwrap(),
            "kieskring 1 (Groningen)"
        );
    }

    #[test]
    fn candidate_with_missing_list_returns_error() {
        let store = CsbStore::new_for_test();
        let category = OmissionCategory::Candidate {
            person: crate::structs::persons::PersonId::new(),
            lists: vec![CandidateListId::new()],
        };
        assert!(category.electoral_district(&store, &EK).is_err());
    }

    #[test]
    fn submitted_list_row_per_district_carries_first_candidate_and_count() {
        let store = CsbStore::new_for_test();
        let person = sample_person(PersonId::new());
        let other = sample_person(PersonId::new());
        store.add_person(person.clone());
        store.add_person(other.clone());
        store.add_candidate_list(CandidateList {
            electoral_districts: vec![ElectoralDistrict::Groningen, ElectoralDistrict::Bonaire],
            candidates: vec![person.id, other.id],
            ..Default::default()
        });

        let rows = store_submitted_lists(&store);

        // The list covers two districts, so it contributes a row to each.
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, ElectoralDistrict::Groningen);
        assert_eq!(rows[1].0, ElectoralDistrict::Bonaire);
        for (_, list) in &rows {
            assert_eq!(list.first_candidate_name, person.name.display());
            assert_eq!(list.candidate_count, 2);
        }
    }

    #[test]
    fn submitted_list_without_candidates_has_an_empty_first_candidate() {
        let (store, _) = store_with_list(vec![ElectoralDistrict::Bonaire]);

        let rows = store_submitted_lists(&store);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.first_candidate_name, "");
        assert_eq!(rows[0].1.candidate_count, 0);
    }

    #[test]
    fn submitted_list_orders_the_lists_by_creation_date() {
        let store = CsbStore::new_for_test();
        let early = sample_person_with(PersonId::new(), None, "Aarts", None, "A.");
        let late = sample_person_with(PersonId::new(), None, "Zwart", None, "Z.");
        store.add_person(early.clone());
        store.add_person(late.clone());
        // Added newest first, so insertion order cannot pass this by accident.
        store.add_candidate_list(CandidateList {
            electoral_districts: vec![ElectoralDistrict::Groningen],
            candidates: vec![late.id],
            created_at: utc("2027-04-02T09:00:00Z"),
            ..Default::default()
        });
        store.add_candidate_list(CandidateList {
            electoral_districts: vec![ElectoralDistrict::Groningen],
            candidates: vec![early.id],
            created_at: utc("2027-04-01T09:00:00Z"),
            ..Default::default()
        });

        let rows = store_submitted_lists(&store);

        let names: Vec<String> = rows
            .into_iter()
            .map(|(_, list)| list.first_candidate_name)
            .collect();
        assert_eq!(names, vec![early.name.display(), late.name.display()]);
    }

    #[test]
    fn submitted_list_of_a_blank_list_is_designated_by_its_first_candidate() {
        let store = CsbStore::new_for_test();
        store.set_political_group(PoliticalGroup {
            list_designation: Some(ListDesignation::Blank),
            ..Default::default()
        });
        let person = sample_person_with(PersonId::new(), None, "Jansen", None, "A.B.");
        store.add_person(person.clone());
        store.add_candidate_list(CandidateList {
            electoral_districts: vec![ElectoralDistrict::Groningen],
            candidates: vec![person.id],
            ..Default::default()
        });

        let rows = store_submitted_lists(&store);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.appellation, "Blanco (Jansen, A.B.)");
    }

    #[test]
    fn district_label_prefixes_the_district_number() {
        assert_eq!(
            district_label(ElectoralDistrict::Bonaire, &EK),
            "13 (Bonaire)"
        );
        assert_eq!(
            district_label(ElectoralDistrict::Groningen, &EK),
            "1 (Groningen)"
        );
    }

    #[test]
    fn district_label_omits_the_number_for_single_district_elections() {
        let ps = ElectionConfig::PS27(Province::Groningen);
        assert!(ps.has_only_one_district());
        assert_eq!(
            district_label(ElectoralDistrict::PsGroningen, &ps),
            "Groningen"
        );
    }

    #[tokio::test]
    async fn submitted_lists_groups_the_rows_by_district_in_district_order() {
        let state = AppState::new_for_tests().await;
        let person = sample_person(PersonId::new());
        seed_csb_store(
            &state,
            named_group("Kiesraad Demo"),
            vec![person.clone()],
            vec![CandidateList {
                // Named in reverse district order on purpose.
                electoral_districts: vec![ElectoralDistrict::Bonaire, ElectoralDistrict::Groningen],
                candidates: vec![person.id],
                ..Default::default()
            }],
        )
        .await;

        let districts = submitted_lists(state.csb_store_registry(), &EK)
            .await
            .unwrap();

        assert_eq!(districts.len(), 2);
        assert_eq!(districts[0].electoral_district, "1 (Groningen)");
        assert_eq!(districts[1].electoral_district, "13 (Bonaire)");
        for district in &districts {
            assert_eq!(district.lists.len(), 1);
            assert_eq!(district.lists[0].appellation, "Kiesraad Demo");
            assert_eq!(
                district.lists[0].first_candidate_name,
                person.name.display()
            );
            assert_eq!(district.lists[0].candidate_count, 1);
        }
    }

    #[tokio::test]
    async fn submitted_lists_is_empty_when_nothing_was_imported() {
        let state = AppState::new_for_tests().await;

        assert!(
            submitted_lists(state.csb_store_registry(), &EK)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn found_omissions_groups_the_descriptions_per_district() {
        let state = AppState::new_for_tests().await;
        let store = seed_csb_store(
            &state,
            named_group("De Geconstateerde Partij"),
            vec![],
            vec![],
        )
        .await;
        create_omission(&store, OmissionCategory::PoliticalGroup, "Eerste verzuim").await;
        create_omission(&store, OmissionCategory::PoliticalGroup, "Tweede verzuim").await;
        create_omission(
            &store,
            OmissionCategory::DeclarationsOfSupport(vec![ElectoralDistrict::Bonaire]),
            "Derde verzuim",
        )
        .await;

        let groups = found_omissions(state.csb_store_registry(), &EK)
            .await
            .unwrap();

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].appellation, "De Geconstateerde Partij");
        assert_eq!(groups[0].electoral_district, "alle kieskringen");
        // The store hands out its omissions unordered, so only the grouping is
        // defined, not the order within a group.
        let mut descriptions = groups[0].omission_descriptions.clone();
        descriptions.sort();
        assert_eq!(descriptions, ["Eerste verzuim", "Tweede verzuim"]);

        assert_eq!(groups[1].appellation, "De Geconstateerde Partij");
        assert_eq!(groups[1].electoral_district, "kieskring 13 (Bonaire)");
        assert_eq!(groups[1].omission_descriptions, ["Derde verzuim"]);
    }

    #[tokio::test]
    async fn found_omissions_skips_groups_without_recoverable_omissions() {
        let state = AppState::new_for_tests().await;

        // An irreparable omission does not put the group in the I 1 table.
        let without = seed_csb_store(&state, named_group("Zonder Verzuimen"), vec![], vec![]).await;
        let mut irreparable = sample_omission(OmissionCategory::PoliticalGroup);
        irreparable.recoverable = false;
        irreparable.create(&without).await.unwrap();

        let with = seed_csb_store(&state, named_group("Met Verzuimen"), vec![], vec![]).await;
        create_omission(
            &with,
            OmissionCategory::PoliticalGroup,
            "Een herstelbaar verzuim",
        )
        .await;

        let groups = found_omissions(state.csb_store_registry(), &EK)
            .await
            .unwrap();

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].appellation, "Met Verzuimen");
        assert_eq!(groups[0].omission_descriptions, ["Een herstelbaar verzuim"]);
    }

    #[tokio::test]
    async fn found_omissions_is_empty_when_nothing_was_imported() {
        let state = AppState::new_for_tests().await;

        assert!(
            found_omissions(state.csb_store_registry(), &EK)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
