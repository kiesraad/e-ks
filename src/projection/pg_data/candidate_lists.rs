//! Candidate list reads that look across every list in the projection.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    ElectionConfig, ElectoralDistrict, PgStoreData,
    structs::{
        candidate_lists::{CandidateList, CandidateListId, CandidateListSummary},
        common::Problematic,
    },
};

impl CandidateList {
    /// Districts already claimed by candidate lists other than `list_id`
    pub fn districts_on_other_lists(
        data: &PgStoreData,
        list_id: Option<CandidateListId>,
    ) -> Vec<ElectoralDistrict> {
        let districts: BTreeSet<ElectoralDistrict> = data
            .candidate_lists
            .values()
            .filter(|list| Some(list.id) != list_id)
            .flat_map(|list| list.electoral_districts.iter().copied())
            .collect();

        districts.into_iter().collect()
    }

    pub fn duplicate_districts(&self, data: &PgStoreData) -> Vec<ElectoralDistrict> {
        let other_districts = Self::districts_on_other_lists(data, Some(self.id));

        self.electoral_districts
            .iter()
            .filter(|d| other_districts.contains(d))
            .copied()
            .collect()
    }
}

impl CandidateListSummary {
    pub fn list(data: &PgStoreData, election: ElectionConfig) -> Vec<CandidateListSummary> {
        let max_count = data.get_political_group().get_max_candidates();
        // How many lists claim each district; a district claimed twice is a
        // duplicate on every list that has it.
        let mut claims: BTreeMap<ElectoralDistrict, usize> = BTreeMap::new();
        for district in data
            .candidate_lists
            .values()
            .flat_map(|l| &l.electoral_districts)
        {
            *claims.entry(*district).or_default() += 1;
        }
        data.get_candidate_lists()
            .into_iter()
            .map(|list| {
                let duplicate_districts = list
                    .electoral_districts
                    .iter()
                    .filter(|d| claims.get(d).is_some_and(|n| *n > 1))
                    .copied()
                    .collect();
                let candidates_with_problems = list
                    .candidates
                    .iter()
                    .filter(|id| {
                        data.get_person(**id).is_ok_and(|person| {
                            !person.get_problems(election).potential_problems.is_empty()
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
