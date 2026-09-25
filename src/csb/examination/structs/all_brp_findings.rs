use std::collections::HashSet;

use crate::{
    CsbStream, Locale,
    csb::examination::{extractors::CsbPoliticalGroup, structs::BrpFindingTag},
    projection::WithCorrections,
    structs::{
        candidate_lists::CandidateListId,
        persons::{Person, PersonId},
    },
};

/// Every BRP finding of one political group, collected per candidate.
pub struct AllBrpFindings {
    pub candidates: Vec<CandidateFindings>,
}

pub struct CandidateFindings {
    /// Position on the list the candidate is listed under.
    pub position: usize,
    pub person: Person,
    /// The candidate's examination page, so a finding leads to the data it is
    /// about; `None` in the pre-submission check, which has no such page.
    pub path: Option<String>,
    /// The findings, already translated.
    pub findings: Vec<BrpFindingTag>,
}

impl CandidateFindings {
    pub fn is_all_handled(&self) -> bool {
        self.findings.iter().all(|finding| finding.handled)
    }
}

/// A candidate as the lists put them forward: on `list_id` at `position`.
pub struct ListedCandidate {
    pub list_id: CandidateListId,
    pub position: usize,
    pub person: Person,
}

impl CsbStream {
    /// Every candidate once, in the order the candidate lists put them
    /// forward. A candidate standing on more than one list is listed under the
    /// first list they appear on.
    pub fn listed_candidates(&self) -> Vec<ListedCandidate> {
        let mut lists = self.get_candidate_lists(WithCorrections::All);
        lists.sort_by_key(|list| (list.created_at, list.id));

        let mut seen: HashSet<PersonId> = HashSet::new();
        let mut candidates = Vec::new();
        for list in lists {
            for (index, person_id) in list.candidates.iter().enumerate() {
                if seen.insert(*person_id)
                    && let Some(person) = self.get_person(*person_id, WithCorrections::All)
                {

                    candidates.push(ListedCandidate {
                        list_id: list.id,
                        position: index + 1,
                        person,
                    });
                }
            }
        }

        candidates
    }

    /// The findings of every candidate that has any, in the order
    /// [`Self::listed_candidates`] puts them.
    pub fn get_all_brp_findings(
        &self,
        political_group: &CsbPoliticalGroup,
        locale: Locale,
    ) -> AllBrpFindings {
        self.collect_brp_findings(locale, |list_id, person_id| {
            Some(political_group.candidate_path(list_id, person_id))
        })
    }

    /// As [`Self::get_all_brp_findings`], without candidate pages to link to.
    pub fn get_unlinked_brp_findings(&self, locale: Locale) -> AllBrpFindings {
        self.collect_brp_findings(locale, |_, _| None)
    }

    fn collect_brp_findings(
        &self,
        locale: Locale,
        path_for: impl Fn(&CandidateListId, &PersonId) -> Option<String>,
    ) -> AllBrpFindings {
        let findings = self.get_brp_findings();
        let candidates = self
            .listed_candidates()
            .into_iter()
            .filter_map(|candidate| {
                let tags: Vec<BrpFindingTag> = findings
                    .get(&candidate.person.id)
                    .into_iter()
                    .flatten()
                    .map(|finding| BrpFindingTag::new(finding, locale))
                    .collect();
                if tags.is_empty() {
                    return None;
                }
                Some(CandidateFindings {
                    path: path_for(&candidate.list_id, &candidate.person.id),
                    position: candidate.position,
                    person: candidate.person,
                    findings: tags,
                })
            })
            .collect();

        AllBrpFindings { candidates }
    }
}
