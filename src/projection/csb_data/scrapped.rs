//! What the unresolved omissions scrap ("schrappen"), derived once per event
//! so every page and model reads the same outcome.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::{
    ElectionConfig, ElectoralDistrict, PgStoreData,
    structs::{
        candidate_lists::CandidateListId,
        csb::{Omission, OmissionCategory, OmissionId},
        list_designation::ListDesignation,
        persons::PersonId,
    },
};

/// The appellation, districts, lists and candidates scrapped by the unresolved
/// omissions (irreparable, or marked as not recovered). Facts only: whether
/// they are shown is up to the phase the page renders for.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scrapped {
    /// Every omission that scraps something.
    omissions: BTreeSet<OmissionId>,
    /// The appellation omissions; a blank list has no appellation to scrap.
    appellation: BTreeSet<OmissionId>,
    /// Districts scrapped by declarations-of-support omissions.
    districts: BTreeSet<ElectoralDistrict>,
    /// Whether a declarations-of-support omission without districts, which
    /// covers all of them, scraps every district.
    all_districts: bool,
    /// Every candidate list, scrapped or not.
    lists: BTreeMap<CandidateListId, ScrappedList>,
    /// Candidates scrapped from a list, by candidate omissions.
    candidates: BTreeSet<(CandidateListId, PersonId)>,
}

/// How much of one candidate list is scrapped.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrappedList {
    /// A list omission scraps the list as a whole.
    whole: bool,
    /// The list's districts that are scrapped, in the list's district order.
    districts: Vec<ElectoralDistrict>,
    /// Whether every district the list was submitted in is scrapped, which
    /// scraps the list as well.
    all_districts: bool,
}

impl ScrappedList {
    pub fn is_scrapped(&self) -> bool {
        self.whole || self.all_districts
    }
}

impl Scrapped {
    /// Derive from the corrected data and the omissions. Candidate omissions
    /// scrap only the candidate, list omissions the list, and declarations of
    /// support the district; a list is also gone once all of its districts are.
    pub(crate) fn derive(data: &PgStoreData, omissions: &HashMap<OmissionId, Omission>) -> Self {
        let mut scrapped = Scrapped::default();
        let is_blank = data.political_group.list_designation == Some(ListDesignation::Blank);

        for omission in omissions.values().filter(|o| o.is_unresolved()) {
            match &omission.category {
                OmissionCategory::Appellation if is_blank => continue,
                OmissionCategory::Appellation => {
                    scrapped.appellation.insert(omission.id);
                }
                OmissionCategory::PoliticalGroup => continue,
                OmissionCategory::DeclarationsOfSupport(districts) if districts.is_empty() => {
                    scrapped.all_districts = true;
                }
                OmissionCategory::DeclarationsOfSupport(districts) => {
                    scrapped.districts.extend(districts.iter().copied());
                }
                OmissionCategory::CandidateList(lists) => {
                    for list in lists {
                        scrapped.lists.entry(*list).or_default().whole = true;
                    }
                }
                OmissionCategory::Candidate { person, lists } => {
                    scrapped
                        .candidates
                        .extend(lists.iter().map(|list| (*list, *person)));
                }
            }
            scrapped.omissions.insert(omission.id);
        }

        for list in data.candidate_lists.values() {
            let districts: Vec<ElectoralDistrict> = list
                .electoral_districts
                .iter()
                .filter(|district| scrapped.is_district_scrapped(**district))
                .copied()
                .collect();
            let entry = scrapped.lists.entry(list.id).or_default();
            entry.all_districts =
                !districts.is_empty() && districts.len() == list.electoral_districts.len();
            entry.districts = districts;
        }

        scrapped
    }

    /// Whether this omission scraps something.
    pub fn is_caused_by(&self, omission: OmissionId) -> bool {
        self.omissions.contains(&omission)
    }

    /// The omissions scrapping the appellation.
    pub fn appellation_omissions(&self) -> &BTreeSet<OmissionId> {
        &self.appellation
    }

    pub fn is_appellation_scrapped(&self) -> bool {
        !self.appellation.is_empty()
    }

    pub fn is_district_scrapped(&self, district: ElectoralDistrict) -> bool {
        self.all_districts || self.districts.contains(&district)
    }

    /// The scrapped districts in the election's district order.
    pub fn districts(&self, election: &ElectionConfig) -> Vec<ElectoralDistrict> {
        election
            .electoral_districts()
            .iter()
            .filter(|district| self.is_district_scrapped(**district))
            .copied()
            .collect()
    }

    /// Whether the whole list is scrapped: by a list omission, or because
    /// every district it was submitted in is. An unknown list is not.
    pub fn is_list_scrapped(&self, list: CandidateListId) -> bool {
        self.lists.get(&list).is_some_and(ScrappedList::is_scrapped)
    }

    /// The scrapped districts of one list, in the list's district order.
    pub fn list_districts(&self, list: CandidateListId) -> &[ElectoralDistrict] {
        self.lists
            .get(&list)
            .map_or(&[], |list| list.districts.as_slice())
    }

    /// Whether every district the list was submitted in is scrapped.
    pub fn all_list_districts_scrapped(&self, list: CandidateListId) -> bool {
        self.lists.get(&list).is_some_and(|list| list.all_districts)
    }

    /// Whether a candidate omission scraps this candidate from this list.
    /// Group and list omissions do not cascade down to the candidates.
    pub fn is_candidate_scrapped(&self, list: CandidateListId, person: PersonId) -> bool {
        self.candidates.contains(&(list, person))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structs::{
        candidate_lists::CandidateList,
        csb::{OmissionStatus, sample_omission},
        political_groups::PoliticalGroup,
    };

    fn omission(category: OmissionCategory, recoverable: bool, status: OmissionStatus) -> Omission {
        let mut omission = sample_omission(category);
        omission.recoverable = recoverable;
        omission.status = status;
        omission
    }

    fn not_recovered(category: OmissionCategory) -> Omission {
        omission(category, true, OmissionStatus::NotRecovered)
    }

    fn irreparable(category: OmissionCategory) -> Omission {
        omission(category, false, OmissionStatus::Pending)
    }

    fn derive(data: &PgStoreData, omissions: Vec<Omission>) -> Scrapped {
        let omissions = omissions.into_iter().map(|o| (o.id, o)).collect();
        Scrapped::derive(data, &omissions)
    }

    fn data_with_list(id: CandidateListId, districts: Vec<ElectoralDistrict>) -> PgStoreData {
        let mut data = PgStoreData::default();
        data.candidate_lists.insert(
            id,
            CandidateList {
                id,
                electoral_districts: districts,
                ..Default::default()
            },
        );
        data
    }

    #[test]
    fn pending_and_recovered_omissions_scrap_nothing() {
        let list = CandidateListId::new();
        let person = PersonId::new();
        let data = data_with_list(list, vec![ElectoralDistrict::Groningen]);

        let scrapped = derive(
            &data,
            vec![
                sample_omission(OmissionCategory::Appellation),
                omission(
                    OmissionCategory::CandidateList(vec![list]),
                    true,
                    OmissionStatus::Recovered,
                ),
                sample_omission(OmissionCategory::DeclarationsOfSupport(vec![
                    ElectoralDistrict::Groningen,
                ])),
                sample_omission(OmissionCategory::Candidate {
                    person,
                    lists: vec![list],
                }),
            ],
        );

        assert!(!scrapped.is_appellation_scrapped());
        assert!(!scrapped.is_list_scrapped(list));
        assert!(!scrapped.is_district_scrapped(ElectoralDistrict::Groningen));
        assert!(!scrapped.is_candidate_scrapped(list, person));
        assert!(scrapped.omissions.is_empty());
    }

    #[test]
    fn candidate_is_scrapped_only_from_the_lists_the_omission_references() {
        let person = PersonId::new();
        let (list_a, list_b) = (CandidateListId::new(), CandidateListId::new());
        let omission = not_recovered(OmissionCategory::Candidate {
            person,
            lists: vec![list_a],
        });
        let id = omission.id;

        let scrapped = derive(&PgStoreData::default(), vec![omission]);

        assert!(scrapped.is_candidate_scrapped(list_a, person));
        assert!(!scrapped.is_candidate_scrapped(list_b, person));
        assert!(!scrapped.is_candidate_scrapped(list_a, PersonId::new()));
        assert!(scrapped.is_caused_by(id));
        // The candidate is gone, the list is not.
        assert!(!scrapped.is_list_scrapped(list_a));
    }

    #[test]
    fn irreparable_omission_scraps_right_away() {
        let person = PersonId::new();
        let list = CandidateListId::new();

        let scrapped = derive(
            &PgStoreData::default(),
            vec![irreparable(OmissionCategory::Candidate {
                person,
                lists: vec![list],
            })],
        );

        assert!(scrapped.is_candidate_scrapped(list, person));
    }

    #[test]
    fn list_omission_scraps_the_whole_list() {
        let list = CandidateListId::new();
        let data = data_with_list(
            list,
            vec![ElectoralDistrict::Groningen, ElectoralDistrict::Drenthe],
        );

        let scrapped = derive(
            &data,
            vec![not_recovered(OmissionCategory::CandidateList(vec![list]))],
        );

        assert!(scrapped.is_list_scrapped(list));
        assert!(!scrapped.all_list_districts_scrapped(list));
        assert!(scrapped.list_districts(list).is_empty());
        assert!(!scrapped.is_list_scrapped(CandidateListId::new()));
    }

    #[test]
    fn list_is_scrapped_once_all_its_districts_are() {
        let list = CandidateListId::new();
        let data = data_with_list(
            list,
            vec![ElectoralDistrict::Groningen, ElectoralDistrict::Drenthe],
        );

        let groningen = not_recovered(OmissionCategory::DeclarationsOfSupport(vec![
            ElectoralDistrict::Groningen,
        ]));
        let scrapped = derive(&data, vec![groningen.clone()]);
        assert_eq!(
            scrapped.list_districts(list),
            [ElectoralDistrict::Groningen]
        );
        assert!(!scrapped.is_list_scrapped(list));

        let drenthe = not_recovered(OmissionCategory::DeclarationsOfSupport(vec![
            ElectoralDistrict::Drenthe,
        ]));
        let scrapped = derive(&data, vec![groningen, drenthe]);
        assert_eq!(
            scrapped.list_districts(list),
            [ElectoralDistrict::Groningen, ElectoralDistrict::Drenthe]
        );
        assert!(scrapped.all_list_districts_scrapped(list));
        assert!(scrapped.is_list_scrapped(list));
    }

    #[test]
    fn declarations_of_support_without_districts_scrap_every_district() {
        let list = CandidateListId::new();
        let data = data_with_list(list, vec![ElectoralDistrict::Utrecht]);

        let scrapped = derive(
            &data,
            vec![irreparable(OmissionCategory::DeclarationsOfSupport(vec![]))],
        );

        assert!(scrapped.is_district_scrapped(ElectoralDistrict::Bonaire));
        assert_eq!(
            scrapped.districts(&ElectionConfig::EK27),
            ElectionConfig::EK27.electoral_districts()
        );
        assert!(scrapped.is_list_scrapped(list));
    }

    #[test]
    fn districts_come_out_in_election_order() {
        let scrapped = derive(
            &PgStoreData::default(),
            vec![not_recovered(OmissionCategory::DeclarationsOfSupport(
                vec![ElectoralDistrict::Drenthe, ElectoralDistrict::Groningen],
            ))],
        );

        assert_eq!(
            scrapped.districts(&ElectionConfig::EK27),
            [ElectoralDistrict::Groningen, ElectoralDistrict::Drenthe]
        );
    }

    #[test]
    fn appellation_is_scrapped_by_appellation_omissions_only() {
        let appellation = irreparable(OmissionCategory::Appellation);
        let id = appellation.id;

        let scrapped = derive(
            &PgStoreData::default(),
            vec![appellation, irreparable(OmissionCategory::PoliticalGroup)],
        );

        assert!(scrapped.is_appellation_scrapped());
        assert_eq!(scrapped.appellation_omissions().len(), 1);
        assert!(scrapped.appellation_omissions().contains(&id));
        // A political-group omission has no consequences of its own.
        assert_eq!(scrapped.omissions.len(), 1);
    }

    #[test]
    fn blank_list_has_no_appellation_to_scrap() {
        let data = PgStoreData {
            political_group: PoliticalGroup {
                list_designation: Some(ListDesignation::Blank),
                ..Default::default()
            },
            ..Default::default()
        };
        let omission = irreparable(OmissionCategory::Appellation);
        let id = omission.id;

        let scrapped = derive(&data, vec![omission]);

        assert!(!scrapped.is_appellation_scrapped());
        assert!(!scrapped.is_caused_by(id));
    }
}
