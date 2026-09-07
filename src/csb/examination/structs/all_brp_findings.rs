use std::collections::HashSet;

use crate::{
    CsbStream, Locale,
    csb::examination::extractors::CsbPoliticalGroup,
    projection::WithCorrections,
    structs::persons::{Person, PersonId},
};

/// Every BRP finding of one political group, collected per candidate.
pub struct AllBrpFindings {
    pub candidates: Vec<CandidateFindings>,
}

pub struct CandidateFindings {
    pub person: Person,
    /// The candidate's own examination page, so a finding leads to the data it
    /// is about.
    pub path: String,
    /// The findings, already translated.
    pub messages: Vec<String>,
}

impl CsbStream {
    /// The findings of every candidate that has any, in the order the candidate
    /// lists put them forward. A candidate standing on more than one list is
    /// listed once, under the first list they appear on.
    pub fn get_all_brp_findings(
        &self,
        political_group: &CsbPoliticalGroup,
        locale: Locale,
    ) -> AllBrpFindings {
        let findings = self.get_brp_findings();
        let mut lists = self.get_candidate_lists(WithCorrections::All);
        lists.sort_by_key(|list| (list.created_at, list.id));

        let mut seen: HashSet<PersonId> = HashSet::new();
        let mut candidates = Vec::new();

        for list in lists {
            for person_id in &list.candidates {
                if !seen.insert(*person_id) {
                    continue;
                }
                let messages: Vec<String> = findings
                    .get(person_id)
                    .into_iter()
                    .flatten()
                    .map(|finding| finding.message(locale))
                    .collect();
                if messages.is_empty() {
                    continue;
                }
                let Some(person) = self.get_person(*person_id, WithCorrections::All) else {
                    continue;
                };
                candidates.push(CandidateFindings {
                    path: political_group
                        .candidate_path(&list.id, person_id)
                        .to_string(),
                    person,
                    messages,
                });
            }
        }

        AllBrpFindings { candidates }
    }
}
