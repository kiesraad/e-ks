use std::collections::HashSet;

use crate::{
    CsbStoreData, Locale,
    csb::examination::{extractors::CsbPoliticalGroup, structs::BrpFindingTag},
    projection::WithCorrections,
    structs::{
        candidate_lists::CandidateListId,
        common::PotentialProblems,
        persons::{Person, PersonId},
        problems::AllProblems,
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

/// A candidate with BRP findings, validation problems, or both.
pub struct ProblematicCandidate {
    pub person: Person,
    /// As [`CandidateFindings::path`].
    pub path: Option<String>,
    pub findings: Vec<BrpFindingTag>,
    pub problems: Vec<PotentialProblems>,
}

impl ProblematicCandidate {
    /// Whether nothing is left to act on: no problems, every finding handled.
    pub fn is_all_handled(&self) -> bool {
        self.problems.is_empty() && self.findings.iter().all(|finding| finding.handled)
    }
}

impl AllBrpFindings {
    /// The candidates with findings, followed by those with only problems,
    /// which link to `path_for`.
    pub fn with_problems(
        self,
        all_problems: &AllProblems,
        path_for: impl Fn(&Person) -> Option<String>,
    ) -> Vec<ProblematicCandidate> {
        let problems_for = |person: &Person| {
            all_problems
                .candidates
                .iter()
                .find(|candidate| candidate.entity.id == person.id)
                .map_or(Vec::new(), |candidate| candidate.problems.clone())
        };

        let mut candidates: Vec<ProblematicCandidate> = self
            .candidates
            .into_iter()
            .map(|candidate| ProblematicCandidate {
                problems: problems_for(&candidate.person),
                person: candidate.person,
                path: candidate.path,
                findings: candidate.findings,
            })
            .collect();

        for problematic in &all_problems.candidates {
            if !candidates
                .iter()
                .any(|candidate| candidate.person.id == problematic.entity.id)
            {
                candidates.push(ProblematicCandidate {
                    path: path_for(&problematic.entity),
                    person: problematic.entity.clone(),
                    findings: Vec::new(),
                    problems: problematic.problems.clone(),
                });
            }
        }

        candidates
    }
}

impl CsbStoreData {
    /// Every candidate once, in the order the candidate lists put them
    /// forward (oldest list first). A candidate standing on more than one list
    /// is listed under the first list they appear on.
    pub fn listed_candidates(&self) -> Vec<ListedCandidate> {
        let corrected = self.view(WithCorrections::All);

        let mut seen: HashSet<PersonId> = HashSet::new();
        let mut candidates = Vec::new();
        for list in corrected.candidate_lists() {
            for (index, person_id) in list.candidates.iter().enumerate() {
                if seen.insert(*person_id)
                    && let Some(person) = corrected.person(*person_id)
                {
                    candidates.push(ListedCandidate {
                        list_id: list.id,
                        position: index + 1,
                        person: person.clone(),
                    });
                }
            }
        }

        candidates
    }

    /// The findings of every candidate that has any, in the order
    /// [`Self::listed_candidates`] puts them.
    pub fn all_brp_findings(
        &self,
        political_group: &CsbPoliticalGroup,
        locale: Locale,
    ) -> AllBrpFindings {
        self.collect_brp_findings(locale, |list_id, person_id| {
            Some(political_group.candidate_path(list_id, person_id))
        })
    }

    /// As [`Self::all_brp_findings`], without candidate pages to link to.
    pub fn unlinked_brp_findings(&self, locale: Locale) -> AllBrpFindings {
        self.collect_brp_findings(locale, |_, _| None)
    }

    fn collect_brp_findings(
        &self,
        locale: Locale,
        path_for: impl Fn(&CandidateListId, &PersonId) -> Option<String>,
    ) -> AllBrpFindings {
        let findings = self.brp_findings();
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
