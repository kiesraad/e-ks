//! Candidate list reads that look across every list in the projection.

use std::collections::BTreeMap;

use crate::{
    ElectionConfig, ElectoralDistrict, PgStoreData,
    structs::candidate_lists::{CandidateList, CandidateListId, CandidateListSummary},
};

impl CandidateList {
    /// Districts already claimed by candidate lists other than `list_id`,
    /// each with the (oldest) list that claims it.
    pub fn districts_on_other_lists(
        data: &PgStoreData,
        list_id: Option<CandidateListId>,
    ) -> BTreeMap<ElectoralDistrict, CandidateList> {
        let mut districts = BTreeMap::new();
        for list in data.get_candidate_lists() {
            if Some(list.id) == list_id {
                continue;
            }
            for district in &list.electoral_districts {
                districts.entry(*district).or_insert_with(|| list.clone());
            }
        }
        districts
    }

    pub fn duplicate_districts(&self, data: &PgStoreData) -> Vec<ElectoralDistrict> {
        let other_districts = Self::districts_on_other_lists(data, Some(self.id));

        self.electoral_districts
            .iter()
            .filter(|d| other_districts.contains_key(d))
            .copied()
            .collect()
    }
}

impl CandidateListSummary {
    pub fn list(data: &PgStoreData, election: ElectionConfig) -> Vec<CandidateListSummary> {
        let max_count = data.get_political_group().get_max_candidates();
        data.get_candidate_lists()
            .into_iter()
            .map(|list| {
                let duplicate_districts = list.duplicate_districts(data);
                let candidates_with_problems = list
                    .candidates
                    .iter()
                    .filter(|id| {
                        data.get_person(**id).is_ok_and(|person| {
                            !data
                                .get_person_problems(person, election)
                                .potential_problems
                                .is_empty()
                        })
                    })
                    .count();
                CandidateListSummary {
                    list: list.clone(),
                    max_count,
                    duplicate_districts,
                    candidates_with_problems,
                }
            })
            .collect()
    }
}
