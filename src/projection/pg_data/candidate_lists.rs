//! Candidate list reads that look across every list in the store.

use std::collections::BTreeSet;

use crate::{
    ElectoralDistrict, PgStore,
    structs::{
        candidate_lists::{CandidateList, CandidateListId, CandidateListSummary},
        common::Problematic,
    },
};

impl CandidateList {
    /// Districts already claimed by candidate lists other than `list_id`
    pub fn districts_on_other_lists(
        store: &PgStore,
        list_id: Option<CandidateListId>,
    ) -> Vec<ElectoralDistrict> {
        let districts: BTreeSet<ElectoralDistrict> = store
            .get_candidate_lists()
            .into_iter()
            .filter(|list| Some(list.id) != list_id)
            .flat_map(|list| list.electoral_districts)
            .collect();

        districts.into_iter().collect()
    }

    pub fn duplicate_districts(&self, store: &PgStore) -> Vec<ElectoralDistrict> {
        let other_districts = Self::districts_on_other_lists(store, Some(self.id));

        self.electoral_districts
            .iter()
            .filter(|d| other_districts.contains(d))
            .copied()
            .collect()
    }
}

impl CandidateListSummary {
    pub fn list(store: &PgStore) -> Vec<CandidateListSummary> {
        let max_count = store.get_political_group().get_max_candidates();
        store
            .get_candidate_lists()
            .into_iter()
            .map(|list| {
                let duplicate_districts = list.duplicate_districts(store);
                let candidates_with_problems = list
                    .candidates
                    .iter()
                    .filter(|id| {
                        store.get_person(**id).is_ok_and(|person| {
                            !person
                                .get_problems(store.election)
                                .potential_problems
                                .is_empty()
                        })
                    })
                    .count();
                CandidateListSummary {
                    list,
                    max_count,
                    duplicate_districts,
                    candidates_with_problems,
                }
            })
            .collect()
    }
}
