//! Model I 1 and I 4 inputs, collected over every imported political group.
//! What the omissions scrap is read from the store's [`Scrapped`] state, so
//! the models report the same outcome as the recovery pages.

use std::collections::BTreeMap;

use crate::{
    AppError, CsbStoreData, CsbStream, ElectionConfig, ElectoralDistrict,
    core::AnyLocale,
    models::{i1, i4},
    projection::{Scrapped, WithCorrections},
    store::StoreRegistry,
    structs::{
        candidate_lists::{CandidateList, CandidateListId},
        common::UtcDateTime,
        csb::{Omission, OmissionCategory, OmissionId},
        persons::PersonId,
    },
};

const ALL_DISTRICTS: &str = "alle kieskringen";

/// The non-deleted imported groups of this election, in store scope order.
async fn examined_stores(
    registry: &StoreRegistry<CsbStoreData>,
    election: &ElectionConfig,
) -> Result<Vec<CsbStream>, AppError> {
    Ok(registry
        .stores_for_election(*election)
        .await?
        .into_iter()
        .filter(|store| !store.is_deleted())
        .collect())
}

/// The I 1 "Kandidatenlijsten" rows per district: a list covering several
/// districts appears in each of them.
pub async fn submitted_lists(
    registry: &StoreRegistry<CsbStoreData>,
    election: &ElectionConfig,
) -> Result<Vec<i1::DistrictLists>, AppError> {
    let mut by_district: BTreeMap<ElectoralDistrict, Vec<i1::SubmittedList>> = BTreeMap::new();
    for store in examined_stores(registry, election).await? {
        for (district, list) in store_submitted_lists(&store) {
            by_district.entry(district).or_default().push(list);
        }
    }

    Ok(by_district
        .into_iter()
        .map(|(district, lists)| i1::DistrictLists {
            electoral_district: district_label(district, election),
            lists,
        })
        .collect())
}

fn store_submitted_lists(store: &CsbStream) -> Vec<(ElectoralDistrict, i1::SubmittedList)> {
    let appellation = store.get_appellation(WithCorrections::All);
    let mut rows = Vec::new();
    for list in lists_by_creation(store) {
        let first_candidate_name = list
            .candidates
            .first()
            .and_then(|id| store.get_person(*id, WithCorrections::All))
            .map(|person| person.name.display())
            .unwrap_or_default();

        for district in &list.electoral_districts {
            rows.push((
                *district,
                i1::SubmittedList {
                    appellation: appellation.clone(),
                    first_candidate_name: first_candidate_name.clone(),
                    candidate_count: list.candidates.len(),
                },
            ));
        }
    }

    rows
}

/// The "Geconstateerde verzuimen" of I 1 and I 4: every recoverable omission.
pub async fn found_omissions(
    registry: &StoreRegistry<CsbStoreData>,
    election: &ElectionConfig,
) -> Result<Vec<i4::OmissionGroup>, AppError> {
    let mut found = Vec::new();
    for store in examined_stores(registry, election).await? {
        let omissions = sorted_omissions(&store);
        found.extend(omission_groups(
            &store,
            election,
            omissions.iter().filter(|omission| omission.recoverable),
        )?);
    }

    Ok(found)
}

/// The I 4 sections derived from the omissions and corrections; the numbering
/// and objections are recorded during the public session.
#[derive(Debug, Default)]
pub struct I4Inputs {
    pub found_omissions: Vec<i4::OmissionGroup>,
    pub recovered_omissions: Vec<i4::OmissionGroup>,
    pub invalid_lists: Vec<i4::OmissionGroup>,
    pub removed_candidates: Vec<i4::RemovedCandidates>,
    pub removed_appellations: Vec<i4::RemovedAppellation>,
    pub corrected_appellations: Vec<i4::CorrectedAppellation>,
    pub valid_lists: Vec<i4::DistrictLists>,
}

pub async fn i4_inputs(
    registry: &StoreRegistry<CsbStoreData>,
    election: &ElectionConfig,
) -> Result<I4Inputs, AppError> {
    let mut inputs = I4Inputs {
        found_omissions: found_omissions(registry, election).await?,
        ..Default::default()
    };
    let mut valid_by_district: BTreeMap<ElectoralDistrict, Vec<i4::ValidList>> = BTreeMap::new();

    for store in examined_stores(registry, election).await? {
        let omissions = sorted_omissions(&store);
        let scrapped = store.get_scrapped();

        inputs.recovered_omissions.extend(omission_groups(
            &store,
            election,
            omissions
                .iter()
                .filter(|omission| omission.recoverable && omission.status.is_recovered()),
        )?);
        inputs.invalid_lists.extend(omission_groups(
            &store,
            election,
            omissions.iter().filter(|omission| {
                scrapped.is_caused_by(omission.id) && invalidates_list(omission)
            }),
        )?);
        inputs
            .removed_candidates
            .extend(removed_candidates(&store, election, &omissions, &scrapped)?);
        inputs
            .removed_appellations
            .extend(removed_appellation(&store, election, &omissions, &scrapped));
        inputs
            .corrected_appellations
            .extend(corrected_appellation(&store, election, &scrapped));
        for (district, list) in valid_lists(&store, &scrapped)? {
            valid_by_district.entry(district).or_default().push(list);
        }
    }

    inputs.valid_lists = valid_by_district
        .into_iter()
        .map(|(district, lists)| i4::DistrictLists {
            electoral_district: district_label(district, election),
            lists,
        })
        .collect();

    Ok(inputs)
}

/// Whether a scrapping omission of this category is reported under the
/// invalid lists (rather than the removed candidates or appellations).
fn invalidates_list(omission: &Omission) -> bool {
    matches!(
        omission.category,
        OmissionCategory::CandidateList(_) | OmissionCategory::DeclarationsOfSupport(_)
    )
}

/// Reading order: group, declarations of support, lists, candidates by position.
fn sorted_omissions(store: &CsbStream) -> Vec<Omission> {
    let mut omissions = store.get_omissions();
    omissions.sort_by_cached_key(|omission| omission_order(store, omission));
    omissions
}

fn omission_order(store: &CsbStream, omission: &Omission) -> (u8, usize, UtcDateTime, OmissionId) {
    let (rank, position) = match &omission.category {
        OmissionCategory::PoliticalGroup | OmissionCategory::Appellation => (0, 0),
        OmissionCategory::DeclarationsOfSupport(_) => (1, 0),
        OmissionCategory::CandidateList(_) => (2, 0),
        OmissionCategory::Candidate { person, lists } => {
            let position = lists
                .first()
                .and_then(|list| store.get_candidate_position(*list, *person, WithCorrections::All))
                .unwrap_or(usize::MAX);
            (3, position)
        }
    };
    (rank, position, omission.updated_at, omission.id)
}

/// One group per district label.
fn omission_groups<'a>(
    store: &CsbStream,
    election: &ElectionConfig,
    omissions: impl IntoIterator<Item = &'a Omission>,
) -> Result<Vec<i4::OmissionGroup>, AppError> {
    let mut by_district: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for omission in omissions {
        let district = omission.category.electoral_district(store, election)?;
        by_district
            .entry(district)
            .or_default()
            .push(omission.description.to_string());
    }

    let appellation = store.get_appellation(WithCorrections::All);
    Ok(by_district
        .into_iter()
        .map(
            |(electoral_district, omission_descriptions)| i4::OmissionGroup {
                appellation: appellation.clone(),
                electoral_district,
                omission_descriptions,
            },
        )
        .collect())
}

/// Candidates scrapped from lists that stay valid, one row per candidate.
fn removed_candidates(
    store: &CsbStream,
    election: &ElectionConfig,
    omissions: &[Omission],
    scrapped: &Scrapped,
) -> Result<Vec<i4::RemovedCandidates>, AppError> {
    let mut by_district: BTreeMap<String, Vec<(PersonId, i4::RemovedCandidate)>> = BTreeMap::new();
    for omission in omissions
        .iter()
        .filter(|omission| scrapped.is_caused_by(omission.id))
    {
        let OmissionCategory::Candidate { person, lists } = &omission.category else {
            continue;
        };
        let districts = valid_districts_with_candidate(store, scrapped, *person, lists)?;
        if districts.is_empty() {
            continue;
        }

        let district = format_districts(&districts, election);
        let rows = by_district.entry(district).or_default();
        if let Some((_, row)) = rows.iter_mut().find(|(id, _)| id == person) {
            row.reasons.push(omission.description.to_string());
        } else {
            let candidate = store
                .get_person(*person, WithCorrections::All)
                .ok_or(AppError::GenericNotFound)?;
            rows.push((
                *person,
                i4::RemovedCandidate {
                    name: candidate.name_as_printed_on_list(AnyLocale::Nl),
                    reasons: vec![omission.description.to_string()],
                },
            ));
        }
    }

    let appellation = store.get_appellation(WithCorrections::All);
    Ok(by_district
        .into_iter()
        .map(|(electoral_district, rows)| i4::RemovedCandidates {
            appellation: appellation.clone(),
            electoral_district,
            candidates: rows.into_iter().map(|(_, row)| row).collect(),
        })
        .collect())
}

/// The districts in which the lists still carrying `person` stay valid.
fn valid_districts_with_candidate(
    store: &CsbStream,
    scrapped: &Scrapped,
    person: PersonId,
    lists: &[CandidateListId],
) -> Result<Vec<ElectoralDistrict>, AppError> {
    let mut valid = Vec::new();
    for id in lists {
        let list = store
            .get_candidate_list(*id, WithCorrections::All)
            .ok_or(AppError::GenericNotFound)?;
        if !list.candidates.contains(&person) || scrapped.is_list_scrapped(*id) {
            continue;
        }
        for district in list.electoral_districts {
            if !scrapped.is_district_scrapped(district) && !valid.contains(&district) {
                valid.push(district);
            }
        }
    }
    Ok(valid)
}

fn removed_appellation(
    store: &CsbStream,
    election: &ElectionConfig,
    omissions: &[Omission],
    scrapped: &Scrapped,
) -> Option<i4::RemovedAppellation> {
    if !scrapped.is_appellation_scrapped() {
        return None;
    }

    Some(i4::RemovedAppellation {
        appellation: store.get_appellation(WithCorrections::All),
        electoral_district: format_districts(&group_districts(store), election),
        first_candidate_name: first_candidate_name(store, scrapped),
        reasons: omissions
            .iter()
            .filter(|omission| scrapped.appellation_omissions().contains(&omission.id))
            .map(|omission| omission.description.to_string())
            .collect(),
    })
}

fn corrected_appellation(
    store: &CsbStream,
    election: &ElectionConfig,
    scrapped: &Scrapped,
) -> Option<i4::CorrectedAppellation> {
    (store.get_political_group_csb_corrections_count() > 0).then(|| i4::CorrectedAppellation {
        first_candidate_name: first_candidate_name(store, scrapped),
        electoral_district: format_districts(&group_districts(store), election),
        submitted_appellation: store.get_appellation(WithCorrections::Paper),
        edited_appellation: store.get_appellation(WithCorrections::All),
    })
}

/// The lists that are not scrapped, per district that is not scrapped.
fn valid_lists(
    store: &CsbStream,
    scrapped: &Scrapped,
) -> Result<Vec<(ElectoralDistrict, i4::ValidList)>, AppError> {
    let appellation = if scrapped.is_appellation_scrapped() {
        first_candidate_name(store, scrapped)
    } else {
        store.get_appellation_with_scrapped(WithCorrections::All, Some(scrapped))
    };

    let mut valid = Vec::new();
    for list in lists_by_creation(store) {
        if scrapped.is_list_scrapped(list.id) {
            continue;
        }
        let candidates = valid_candidates(store, scrapped, &list)?;
        for district in &list.electoral_districts {
            if !scrapped.is_district_scrapped(*district) {
                valid.push((
                    *district,
                    i4::ValidList {
                        appellation: appellation.clone(),
                        candidates: candidates.clone(),
                    },
                ));
            }
        }
    }

    Ok(valid)
}

/// The candidates that are not scrapped, renumbered.
fn valid_candidates(
    store: &CsbStream,
    scrapped: &Scrapped,
    list: &CandidateList,
) -> Result<Vec<i4::ValidListCandidate>, AppError> {
    list.candidates
        .iter()
        .filter(|person| !scrapped.is_candidate_scrapped(list.id, **person))
        .enumerate()
        .map(|(index, person)| {
            let person = store
                .get_person(*person, WithCorrections::All)
                .ok_or(AppError::GenericNotFound)?;
            Ok(i4::ValidListCandidate {
                position: index + 1,
                last_name: person.name.last_name_with_prefix(),
                initials: person.initials_as_printed_on_list(AnyLocale::Nl),
                locality: person.personal_data.locality().unwrap_or_default(),
            })
        })
        .collect()
}

// TODO: sort by the list numbering once it is implemented, falling back to
// creation order for the draft I 4 that predates the numbering.
fn lists_by_creation(store: &CsbStream) -> Vec<CandidateList> {
    let mut lists = store.get_candidate_lists(WithCorrections::All);
    lists.sort_unstable_by_key(|list| list.created_at);
    lists
}

/// E.g. `van Dijk, A.B. (Anne)`; empty without candidates.
fn first_candidate_name(store: &CsbStream, scrapped: &Scrapped) -> String {
    store
        .get_first_candidate_name(WithCorrections::All, Some(scrapped))
        .map(|name| name.display())
        .unwrap_or_default()
}

impl OmissionCategory {
    /// The "kieskring(en)" column of models I 1 and I 4.
    pub fn electoral_district(
        &self,
        store: &CsbStream,
        election: &ElectionConfig,
    ) -> Result<String, AppError> {
        let districts = match self {
            OmissionCategory::PoliticalGroup | OmissionCategory::Appellation => {
                group_districts(store)
            }
            OmissionCategory::CandidateList(lists) | OmissionCategory::Candidate { lists, .. } => {
                list_districts(store, lists)?
            }
            OmissionCategory::DeclarationsOfSupport(districts) => districts.clone(),
        };
        Ok(format_districts(&districts, election))
    }
}

fn list_districts(
    store: &CsbStream,
    lists: &[CandidateListId],
) -> Result<Vec<ElectoralDistrict>, AppError> {
    let mut districts = Vec::new();
    for id in lists {
        let list = store
            .get_candidate_list(*id, WithCorrections::All)
            .ok_or(AppError::GenericNotFound)?;
        for district in list.electoral_districts {
            if !districts.contains(&district) {
                districts.push(district);
            }
        }
    }
    Ok(districts)
}

fn group_districts(store: &CsbStream) -> Vec<ElectoralDistrict> {
    let mut districts = Vec::new();
    for list in store.get_candidate_lists(WithCorrections::All) {
        for district in list.electoral_districts {
            if !districts.contains(&district) {
                districts.push(district);
            }
        }
    }
    districts
}

/// `alle kieskringen`, or e.g. `kieskring 1 (Groningen), 3 (Drenthe)`.
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

/// E.g. `1 (Groningen)`; the title alone for single-district elections.
fn district_label(district: ElectoralDistrict, election: &ElectionConfig) -> String {
    let title = district.title();
    if election.has_only_one_district() {
        title.to_string()
    } else {
        format!("{} ({})", district.region_number(), title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppRequestState, AppState, CsbAction, CsbStore, ElectionConfig, ElectoralDistrict,
        PgStoreData, Province, StreamId,
        structs::{
            candidate_lists::{CandidateList, CandidateListId},
            common::UtcDateTime,
            csb::{Correction, OmissionStatus, sample_omission},
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

    /// An imported stream in the test registry.
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

    /// One list in Groningen and Bonaire with Aarts, de Boer and Cornelissen.
    async fn seed_group_with_list(
        state: &AppState,
        appellation: &str,
    ) -> (CsbStore, CandidateList, Vec<Person>) {
        let persons = vec![
            sample_person_with(PersonId::new(), Some("Anna"), "Aarts", None, "A."),
            sample_person_with(PersonId::new(), Some("Bas"), "Boer", Some("de"), "B."),
            sample_person_with(PersonId::new(), Some("Cas"), "Cornelissen", None, "C."),
        ];
        let list = CandidateList {
            electoral_districts: vec![ElectoralDistrict::Groningen, ElectoralDistrict::Bonaire],
            candidates: persons.iter().map(|person| person.id).collect(),
            ..Default::default()
        };
        let store = seed_csb_store(
            state,
            named_group(appellation),
            persons.clone(),
            vec![list.clone()],
        )
        .await;
        (store, list, persons)
    }

    async fn create_omission(
        store: &CsbStore,
        category: OmissionCategory,
        description: &str,
    ) -> Omission {
        let mut omission = sample_omission(category);
        omission.description = description.parse().unwrap();
        omission.create(store).await.unwrap();
        omission
    }

    async fn create_irreparable_omission(
        store: &CsbStore,
        category: OmissionCategory,
        description: &str,
    ) -> Omission {
        let mut omission = sample_omission(category);
        omission.description = description.parse().unwrap();
        omission.recoverable = false;
        omission.create(store).await.unwrap();
        omission
    }

    async fn create_omission_with_status(
        store: &CsbStore,
        category: OmissionCategory,
        description: &str,
        status: OmissionStatus,
    ) -> Omission {
        let omission = create_omission(store, category, description).await;
        omission.set_status(store, status).await.unwrap();
        omission
    }

    fn last_names(list: &i4::ValidList) -> Vec<(usize, String)> {
        list.candidates
            .iter()
            .map(|candidate| (candidate.position, candidate.last_name.clone()))
            .collect()
    }

    #[test]
    fn political_group_without_lists_maps_to_all_districts() {
        let store = CsbStore::new_for_test();
        assert_eq!(
            OmissionCategory::PoliticalGroup
                .electoral_district(&store, &EK)
                .unwrap(),
            "alle kieskringen"
        );
    }

    #[test]
    fn political_group_maps_to_the_districts_of_its_lists() {
        let (store, _) = store_with_list(vec![ElectoralDistrict::Groningen]);
        store.add_candidate_list(CandidateList {
            electoral_districts: vec![ElectoralDistrict::Groningen, ElectoralDistrict::Bonaire],
            ..Default::default()
        });

        assert_eq!(
            OmissionCategory::PoliticalGroup
                .electoral_district(&store, &EK)
                .unwrap(),
            "kieskring 1 (Groningen), 13 (Bonaire)"
        );
    }

    #[test]
    fn candidate_with_all_districts_maps_to_all() {
        let (store, id) = store_with_list(EK.electoral_districts().to_vec());
        let category = OmissionCategory::Candidate {
            person: PersonId::new(),
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
            person: PersonId::new(),
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
            person: PersonId::new(),
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
            person: PersonId::new(),
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
            person: PersonId::new(),
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
    async fn sorted_omissions_follow_the_category_and_list_position() {
        let state = AppState::new_for_tests().await;
        let (store, list, persons) = seed_group_with_list(&state, "Gesorteerd").await;
        let candidate = |index: usize| OmissionCategory::Candidate {
            person: persons[index].id,
            lists: vec![list.id],
        };
        // Created in reverse reading order on purpose.
        create_omission(&store, candidate(2), "Kandidaat 3").await;
        create_omission(&store, candidate(0), "Kandidaat 1").await;
        create_omission(
            &store,
            OmissionCategory::CandidateList(vec![list.id]),
            "Lijst",
        )
        .await;
        create_omission(
            &store,
            OmissionCategory::DeclarationsOfSupport(vec![ElectoralDistrict::Bonaire]),
            "Ondersteuningsverklaringen",
        )
        .await;
        create_omission(&store, OmissionCategory::PoliticalGroup, "Groepering").await;

        let descriptions: Vec<String> = sorted_omissions(&store)
            .iter()
            .map(|omission| omission.description.to_string())
            .collect();

        assert_eq!(
            descriptions,
            [
                "Groepering",
                "Ondersteuningsverklaringen",
                "Lijst",
                "Kandidaat 1",
                "Kandidaat 3"
            ]
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
    async fn deleted_groups_are_left_out_of_the_models() {
        let state = AppState::new_for_tests().await;
        let (store, list, _) = seed_group_with_list(&state, "Verwijderd").await;
        create_omission(
            &store,
            OmissionCategory::CandidateList(vec![list.id]),
            "Een verzuim",
        )
        .await;
        store.update(CsbAction::Delete).await.unwrap();

        let registry = state.csb_store_registry();
        assert!(submitted_lists(registry, &EK).await.unwrap().is_empty());
        assert!(found_omissions(registry, &EK).await.unwrap().is_empty());
        let inputs = i4_inputs(registry, &EK).await.unwrap();
        assert!(inputs.found_omissions.is_empty());
        assert!(inputs.valid_lists.is_empty());
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
        assert_eq!(
            groups[0].omission_descriptions,
            ["Eerste verzuim", "Tweede verzuim"]
        );

        assert_eq!(groups[1].appellation, "De Geconstateerde Partij");
        assert_eq!(groups[1].electoral_district, "kieskring 13 (Bonaire)");
        assert_eq!(groups[1].omission_descriptions, ["Derde verzuim"]);
    }

    #[tokio::test]
    async fn found_omissions_skips_groups_without_recoverable_omissions() {
        let state = AppState::new_for_tests().await;

        // An irreparable omission does not put the group in the I 1 table.
        let without = seed_csb_store(&state, named_group("Zonder Verzuimen"), vec![], vec![]).await;
        create_irreparable_omission(&without, OmissionCategory::PoliticalGroup, "Onherstelbaar")
            .await;

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

    #[tokio::test]
    async fn i4_inputs_is_empty_when_nothing_was_imported() {
        let state = AppState::new_for_tests().await;

        let inputs = i4_inputs(state.csb_store_registry(), &EK).await.unwrap();

        assert!(inputs.found_omissions.is_empty());
        assert!(inputs.recovered_omissions.is_empty());
        assert!(inputs.invalid_lists.is_empty());
        assert!(inputs.removed_candidates.is_empty());
        assert!(inputs.removed_appellations.is_empty());
        assert!(inputs.corrected_appellations.is_empty());
        assert!(inputs.valid_lists.is_empty());
    }

    #[tokio::test]
    async fn i4_lists_every_recoverable_omission_as_found_and_the_recovered_ones_again() {
        let state = AppState::new_for_tests().await;
        let (store, list, persons) = seed_group_with_list(&state, "De Herstelde Partij").await;
        let candidate = |index: usize| OmissionCategory::Candidate {
            person: persons[index].id,
            lists: vec![list.id],
        };
        create_omission(&store, candidate(0), "Nog te beoordelen").await;
        create_omission_with_status(&store, candidate(1), "Hersteld", OmissionStatus::Recovered)
            .await;
        create_omission_with_status(
            &store,
            candidate(2),
            "Niet hersteld",
            OmissionStatus::NotRecovered,
        )
        .await;
        create_irreparable_omission(&store, OmissionCategory::PoliticalGroup, "Onherstelbaar")
            .await;

        let inputs = i4_inputs(state.csb_store_registry(), &EK).await.unwrap();

        // Irreparable omissions are not "found"; the status does not matter here.
        assert_eq!(inputs.found_omissions.len(), 1);
        assert_eq!(inputs.found_omissions[0].appellation, "De Herstelde Partij");
        assert_eq!(
            inputs.found_omissions[0].omission_descriptions,
            ["Nog te beoordelen", "Hersteld", "Niet hersteld"]
        );
        assert_eq!(inputs.recovered_omissions.len(), 1);
        assert_eq!(
            inputs.recovered_omissions[0].omission_descriptions,
            ["Hersteld"]
        );
    }

    #[tokio::test]
    async fn i4_unresolved_list_omissions_make_the_list_invalid() {
        let state = AppState::new_for_tests().await;
        let (store, list, _) = seed_group_with_list(&state, "De Ongeldige Partij").await;
        create_omission_with_status(
            &store,
            OmissionCategory::CandidateList(vec![list.id]),
            "Niet hersteld lijstverzuim",
            OmissionStatus::NotRecovered,
        )
        .await;
        create_omission_with_status(
            &store,
            OmissionCategory::CandidateList(vec![list.id]),
            "Hersteld lijstverzuim",
            OmissionStatus::Recovered,
        )
        .await;
        create_omission(
            &store,
            OmissionCategory::CandidateList(vec![list.id]),
            "Nog te beoordelen lijstverzuim",
        )
        .await;

        let inputs = i4_inputs(state.csb_store_registry(), &EK).await.unwrap();

        assert_eq!(inputs.invalid_lists.len(), 1);
        assert_eq!(inputs.invalid_lists[0].appellation, "De Ongeldige Partij");
        assert_eq!(
            inputs.invalid_lists[0].electoral_district,
            "kieskring 1 (Groningen), 13 (Bonaire)"
        );
        assert_eq!(
            inputs.invalid_lists[0].omission_descriptions,
            ["Niet hersteld lijstverzuim"]
        );
        assert!(inputs.valid_lists.is_empty());
    }

    #[tokio::test]
    async fn i4_irreparable_list_omission_makes_the_list_invalid_without_a_decision() {
        let state = AppState::new_for_tests().await;
        let (store, list, _) = seed_group_with_list(&state, "De Onherstelbare Partij").await;
        create_irreparable_omission(
            &store,
            OmissionCategory::CandidateList(vec![list.id]),
            "Onherstelbaar lijstverzuim",
        )
        .await;

        let inputs = i4_inputs(state.csb_store_registry(), &EK).await.unwrap();

        assert!(inputs.found_omissions.is_empty());
        assert_eq!(inputs.invalid_lists.len(), 1);
        assert_eq!(
            inputs.invalid_lists[0].omission_descriptions,
            ["Onherstelbaar lijstverzuim"]
        );
        assert!(inputs.valid_lists.is_empty());
    }

    #[tokio::test]
    async fn i4_unresolved_declarations_of_support_invalidate_the_list_in_those_districts() {
        let state = AppState::new_for_tests().await;
        let (store, list, persons) = seed_group_with_list(&state, "De Halve Partij").await;
        create_omission_with_status(
            &store,
            OmissionCategory::DeclarationsOfSupport(vec![ElectoralDistrict::Bonaire]),
            "Te weinig ondersteuningsverklaringen",
            OmissionStatus::NotRecovered,
        )
        .await;
        create_omission_with_status(
            &store,
            OmissionCategory::Candidate {
                person: persons[1].id,
                lists: vec![list.id],
            },
            "Kandidaatverzuim",
            OmissionStatus::NotRecovered,
        )
        .await;

        let inputs = i4_inputs(state.csb_store_registry(), &EK).await.unwrap();

        assert_eq!(inputs.invalid_lists.len(), 1);
        assert_eq!(
            inputs.invalid_lists[0].electoral_district,
            "kieskring 13 (Bonaire)"
        );
        // The list stays valid in Groningen only, without de Boer.
        assert_eq!(inputs.valid_lists.len(), 1);
        assert_eq!(inputs.valid_lists[0].electoral_district, "1 (Groningen)");
        assert_eq!(inputs.valid_lists[0].lists.len(), 1);
        assert_eq!(
            inputs.valid_lists[0].lists[0].appellation,
            "De Halve Partij"
        );
        assert_eq!(
            last_names(&inputs.valid_lists[0].lists[0]),
            [(1, "Aarts".to_string()), (2, "Cornelissen".to_string())]
        );
        // The candidate is scrapped from the list where it still exists.
        assert_eq!(inputs.removed_candidates.len(), 1);
        assert_eq!(
            inputs.removed_candidates[0].electoral_district,
            "kieskring 1 (Groningen)"
        );
        assert_eq!(inputs.removed_candidates[0].candidates.len(), 1);
    }

    #[tokio::test]
    async fn i4_unresolved_candidate_omissions_scrap_the_candidate() {
        let state = AppState::new_for_tests().await;
        let (store, list, persons) = seed_group_with_list(&state, "De Geschrapte Partij").await;
        let candidate = |index: usize| OmissionCategory::Candidate {
            person: persons[index].id,
            lists: vec![list.id],
        };
        // Both unresolved omissions on de Boer merge into one row.
        create_omission_with_status(
            &store,
            candidate(1),
            "Instemmingsverklaring ontbreekt",
            OmissionStatus::NotRecovered,
        )
        .await;
        create_irreparable_omission(&store, candidate(1), "Kopie ID ontbreekt").await;
        create_omission_with_status(&store, candidate(0), "Hersteld", OmissionStatus::Recovered)
            .await;

        let inputs = i4_inputs(state.csb_store_registry(), &EK).await.unwrap();

        assert_eq!(inputs.removed_candidates.len(), 1);
        let removed = &inputs.removed_candidates[0];
        assert_eq!(removed.appellation, "De Geschrapte Partij");
        assert_eq!(
            removed.electoral_district,
            "kieskring 1 (Groningen), 13 (Bonaire)"
        );
        assert_eq!(removed.candidates.len(), 1);
        assert_eq!(removed.candidates[0].name, "de Boer, B. (Bas) (v)");
        assert_eq!(
            removed.candidates[0].reasons,
            ["Instemmingsverklaring ontbreekt", "Kopie ID ontbreekt"]
        );
        assert!(inputs.invalid_lists.is_empty());

        // Both districts: without de Boer, renumbered.
        assert_eq!(inputs.valid_lists.len(), 2);
        for district in &inputs.valid_lists {
            assert_eq!(district.lists.len(), 1);
            assert_eq!(
                last_names(&district.lists[0]),
                [(1, "Aarts".to_string()), (2, "Cornelissen".to_string())]
            );
        }
    }

    #[tokio::test]
    async fn i4_valid_list_candidates_are_printed_like_the_candidate_list() {
        let state = AppState::new_for_tests().await;
        let (_, _, persons) = seed_group_with_list(&state, "De Correcte Partij").await;

        let inputs = i4_inputs(state.csb_store_registry(), &EK).await.unwrap();

        let candidate = &inputs.valid_lists[0].lists[0].candidates[1];
        assert_eq!(candidate.position, 2);
        assert_eq!(candidate.last_name, "de Boer");
        assert_eq!(candidate.initials, "B. (Bas) (v)");
        assert_eq!(
            candidate.locality,
            persons[1].personal_data.locality().unwrap()
        );
    }

    #[tokio::test]
    async fn i4_does_not_scrap_candidates_from_an_invalid_list() {
        let state = AppState::new_for_tests().await;
        let (store, list, persons) = seed_group_with_list(&state, "De Dubbele Partij").await;
        create_omission_with_status(
            &store,
            OmissionCategory::Candidate {
                person: persons[0].id,
                lists: vec![list.id],
            },
            "Kandidaatverzuim",
            OmissionStatus::NotRecovered,
        )
        .await;
        create_omission_with_status(
            &store,
            OmissionCategory::CandidateList(vec![list.id]),
            "Lijstverzuim",
            OmissionStatus::NotRecovered,
        )
        .await;

        let inputs = i4_inputs(state.csb_store_registry(), &EK).await.unwrap();

        assert_eq!(inputs.invalid_lists.len(), 1);
        assert!(inputs.removed_candidates.is_empty());
        assert!(inputs.valid_lists.is_empty());
    }

    #[tokio::test]
    async fn i4_unresolved_appellation_omission_scraps_the_appellation() {
        let state = AppState::new_for_tests().await;
        let (store, _, _) = seed_group_with_list(&state, "De Geschrapte Aanduiding").await;
        create_irreparable_omission(
            &store,
            OmissionCategory::Appellation,
            "De aanduiding is niet geregistreerd",
        )
        .await;

        let inputs = i4_inputs(state.csb_store_registry(), &EK).await.unwrap();

        assert_eq!(inputs.removed_appellations.len(), 1);
        let removed = &inputs.removed_appellations[0];
        assert_eq!(removed.appellation, "De Geschrapte Aanduiding");
        assert_eq!(
            removed.electoral_district,
            "kieskring 1 (Groningen), 13 (Bonaire)"
        );
        assert_eq!(removed.first_candidate_name, "Aarts, A. (Anna)");
        assert_eq!(removed.reasons, ["De aanduiding is niet geregistreerd"]);

        assert!(inputs.invalid_lists.is_empty());
        assert_eq!(inputs.valid_lists.len(), 2);
        assert_eq!(
            inputs.valid_lists[0].lists[0].appellation,
            "Aarts, A. (Anna)"
        );
    }

    #[tokio::test]
    async fn i4_reports_the_csb_corrected_appellation() {
        let state = AppState::new_for_tests().await;
        let (store, _, _) = seed_group_with_list(&state, "AAP").await;
        store
            .update(CsbAction::UpdateCorrection(Correction::Appellation(
                "De Aangepaste Aanduiding Partij (AAP)".parse().unwrap(),
            )))
            .await
            .unwrap();

        let inputs = i4_inputs(state.csb_store_registry(), &EK).await.unwrap();

        assert_eq!(inputs.corrected_appellations.len(), 1);
        let corrected = &inputs.corrected_appellations[0];
        assert_eq!(corrected.first_candidate_name, "Aarts, A. (Anna)");
        assert_eq!(
            corrected.electoral_district,
            "kieskring 1 (Groningen), 13 (Bonaire)"
        );
        assert_eq!(corrected.submitted_appellation, "AAP");
        assert_eq!(
            corrected.edited_appellation,
            "De Aangepaste Aanduiding Partij (AAP)"
        );
        assert_eq!(
            inputs.valid_lists[0].lists[0].appellation,
            "De Aangepaste Aanduiding Partij (AAP)"
        );
    }

    #[tokio::test]
    async fn i4_groups_the_valid_lists_per_district_in_district_order() {
        let state = AppState::new_for_tests().await;
        let person = sample_person(PersonId::new());
        seed_csb_store(
            &state,
            named_group("Alleen Bonaire"),
            vec![person.clone()],
            vec![CandidateList {
                electoral_districts: vec![ElectoralDistrict::Bonaire],
                candidates: vec![person.id],
                ..Default::default()
            }],
        )
        .await;
        seed_group_with_list(&state, "Twee Kieskringen").await;

        let inputs = i4_inputs(state.csb_store_registry(), &EK).await.unwrap();

        assert_eq!(inputs.valid_lists.len(), 2);
        assert_eq!(inputs.valid_lists[0].electoral_district, "1 (Groningen)");
        assert_eq!(inputs.valid_lists[0].lists.len(), 1);
        assert_eq!(
            inputs.valid_lists[0].lists[0].appellation,
            "Twee Kieskringen"
        );
        assert_eq!(inputs.valid_lists[1].electoral_district, "13 (Bonaire)");
        let appellations: Vec<&str> = inputs.valid_lists[1]
            .lists
            .iter()
            .map(|list| list.appellation.as_str())
            .collect();
        assert_eq!(appellations.len(), 2);
        assert!(appellations.contains(&"Alleen Bonaire"));
        assert!(appellations.contains(&"Twee Kieskringen"));
    }
}
