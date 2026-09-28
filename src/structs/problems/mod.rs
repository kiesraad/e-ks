//! Validation problems reported on the finalise page and on the CSB examination pages
mod problems_sort;

use crate::{
    Locale,
    structs::{
        candidate_lists::{CandidateList, CandidateListId},
        common::{HasSeverity, InfoProblems, PotentialProblems, Problematic, Severity},
        list_submitters::{ListSubmitter, ListSubmitterId},
        name_authorisations::NameAuthorisation,
        persons::{Person, PersonId},
    },
};

/// Aggregation struct for everything that can be missing or incomplete for a list submission
#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq, Clone))]
pub struct AllProblems {
    pub general: GeneralProblems,
    pub candidates: Vec<PersonProblems>,
    pub lists: ListProblems,
    pub info_problems: Vec<EntityInfoProblems>,
}

impl AllProblems {
    fn flatten_problems(&self) -> impl Iterator<Item = &PotentialProblems> {
        let candidate_iter = self.candidates.iter().flat_map(|ci| &ci.problems);
        let list_iter = self.lists.per_list.iter().flat_map(|ci| &ci.problems);
        let list_general_iter = self.lists.general.iter();
        let general_iter = self.general.flatten();

        candidate_iter
            .chain(list_iter)
            .chain(general_iter)
            .chain(list_general_iter)
    }

    pub fn models_downloadable(&self) -> bool {
        !self
            .flatten_problems()
            .any(|ii| ii.severity() == Severity::Error)
    }

    pub fn get_problems_for_person(&self, person: &Person) -> Vec<&PotentialProblems> {
        self.candidates
            .iter()
            .find(|c| &c.entity == person)
            .map_or(Vec::new(), |c| c.problems.iter().collect())
    }

    /// Determines the max severity of problems of this list and the candidates on this list
    pub fn max_severity_for_list_and_candidates(
        &self,
        list_id: &CandidateListId,
    ) -> Option<Severity> {
        if let Some(list_problems) = self.lists.per_list.iter().find(|l| &l.entity.id == list_id) {
            let highest_list = list_problems
                .problems
                .iter()
                .map(PotentialProblems::severity)
                .max();
            let highest_candidate = self
                .candidates
                .iter()
                .filter(|c| list_problems.entity.candidates.contains(&c.entity.id))
                .flat_map(|c| c.problems.iter().map(PotentialProblems::severity))
                .max();
            highest_list.max(highest_candidate)
        } else {
            None
        }
    }

    /// Tests if a list or its candidates have an equal or higher severity than provided
    pub fn has_severity_or_higher_for_list_and_candidates(
        &self,
        list_id: &CandidateListId,
        severity: Severity,
    ) -> bool {
        self.max_severity_for_list_and_candidates(list_id)
            .map(|s| s >= severity)
            .unwrap_or(false)
    }
}

impl HasSeverity for AllProblems {
    fn highest_severity(&self) -> Option<Severity> {
        self.flatten_problems()
            .map(PotentialProblems::severity)
            .max()
            .or_else(|| (!self.info_problems.is_empty()).then_some(Severity::Info))
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq, Clone))]
pub struct GeneralProblems {
    pub general: Vec<PotentialProblems>,
    pub name_authorisations: Vec<EntityProblems<NameAuthorisation>>,
    pub list_submitter: Option<EntityProblems<ListSubmitter>>,
    pub substitute_submitters: Vec<EntityProblems<ListSubmitter>>,
}

impl GeneralProblems {
    pub fn flatten(&self) -> Vec<&PotentialProblems> {
        let mut result = Vec::new();

        result.extend(&self.general);
        result.extend(self.name_authorisations.iter().flat_map(|na| &na.problems));
        result.extend(
            self.substitute_submitters
                .iter()
                .flat_map(|ss| &ss.problems),
        );
        if let Some(submitter_problems) = &self.list_submitter {
            result.extend(&submitter_problems.problems);
        }
        result
    }

    pub fn get_general_name_authorisation_problems_combined(&self) -> Vec<PotentialProblems> {
        self.general
            .iter()
            .chain(self.name_authorisations.iter().flat_map(|na| &na.problems))
            .cloned()
            .collect()
    }

    pub fn get_substitute_submitter_problems(
        &self,
        submitter_id: ListSubmitterId,
    ) -> Option<Vec<PotentialProblems>> {
        self.substitute_submitters
            .iter()
            .find(|ss| ss.entity.id == submitter_id)
            .map(|ss| ss.problems.clone())
    }
}

impl HasSeverity for GeneralProblems {
    fn highest_severity(&self) -> Option<Severity> {
        self.flatten().iter().map(|p| p.severity()).max()
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq, Clone))]
pub struct EntityProblems<T> {
    pub entity: T,
    pub problems: Vec<PotentialProblems>,
}

impl<T: Problematic<()>> EntityProblems<T> {
    pub fn new(entity: T) -> (Self, Vec<InfoProblems>) {
        let problems = entity.get_problems(());
        (
            EntityProblems {
                entity,
                problems: problems.potential_problems,
            },
            problems.info_problems,
        )
    }
}

pub type PersonProblems = EntityProblems<Person>;

impl PersonProblems {
    pub fn get_highest_severity_for_person(
        problems: &[Self],
        person_id: PersonId,
    ) -> Option<Severity> {
        problems
            .iter()
            .find(|pss| pss.entity.id == person_id)
            .and_then(|ps| ps.problems.iter().map(PotentialProblems::severity).max())
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq, Clone))]
pub struct ListProblems {
    pub general: Vec<PotentialProblems>,
    pub per_list: Vec<EntityProblems<CandidateList>>,
}

impl ListProblems {
    fn flatten(&self) -> Vec<&PotentialProblems> {
        let mut result = self.general.iter().collect::<Vec<_>>();
        result.extend(
            self.per_list
                .iter()
                .flat_map(|l| &l.problems)
                .collect::<Vec<_>>(),
        );
        result
    }

    pub fn is_empty(&self) -> bool {
        self.flatten().is_empty()
    }

    pub fn highest_severity(&self) -> Option<Severity> {
        self.flatten().iter().map(|p| p.severity()).max()
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq, Clone))]
pub enum EntityInfoProblems {
    AnyProblem(InfoProblems),
    Submitter {
        problem: InfoProblems,
    },
    SubstituteSubmitter {
        submitter: ListSubmitter,
        problem: InfoProblems,
    },
    Person {
        person: Box<Person>,
        problem: InfoProblems,
    },
    NameAuthorisation {
        name_authorisation: NameAuthorisation,
        problem: InfoProblems,
    },
}

impl EntityInfoProblems {
    pub fn translate(&self, locale: &Locale) -> String {
        match self {
            EntityInfoProblems::AnyProblem(problem) => problem.translate(locale),
            EntityInfoProblems::Submitter { problem, .. } => problem.translate(locale),
            EntityInfoProblems::SubstituteSubmitter { problem, .. } => problem.translate(locale),
            EntityInfoProblems::Person { problem, .. } => problem.translate(locale),
            EntityInfoProblems::NameAuthorisation { problem, .. } => problem.translate(locale),
        }
    }

    pub fn severity(&self) -> Severity {
        match self {
            EntityInfoProblems::AnyProblem(problem) => problem.severity(),
            EntityInfoProblems::Submitter { problem, .. } => problem.severity(),
            EntityInfoProblems::SubstituteSubmitter { problem, .. } => problem.severity(),
            EntityInfoProblems::Person { problem, .. } => problem.severity(),
            EntityInfoProblems::NameAuthorisation { problem, .. } => problem.severity(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        structs::{candidate_lists::CandidateListId, persons::PersonId},
        test_utils::{sample_candidate_list, sample_person},
    };

    use super::*;

    fn empty_general() -> GeneralProblems {
        GeneralProblems {
            general: Vec::new(),
            name_authorisations: Vec::new(),
            list_submitter: None,
            substitute_submitters: Vec::new(),
        }
    }

    #[test]
    fn is_printable() {
        assert!(
            AllProblems {
                general: empty_general(),
                candidates: Vec::new(),
                lists: ListProblems {
                    general: Vec::new(),
                    per_list: Vec::new()
                },
                info_problems: Vec::new()
            }
            .models_downloadable()
        );

        assert!(
            AllProblems {
                general: empty_general(),
                candidates: vec![],
                lists: ListProblems {
                    general: Vec::new(),
                    per_list: vec![EntityProblems {
                        entity: sample_candidate_list(CandidateListId::new()),
                        problems: vec![PotentialProblems::TooManyCandidates { count: 1 }],
                    }]
                },
                info_problems: Vec::new()
            }
            .models_downloadable()
        );

        assert!(
            !AllProblems {
                general: empty_general(),
                candidates: vec![PersonProblems {
                    entity: sample_person(PersonId::new()),
                    problems: vec![PotentialProblems::NoCandidates]
                }],
                lists: ListProblems {
                    general: Vec::new(),
                    per_list: Vec::new(),
                },
                info_problems: Vec::new()
            }
            .models_downloadable()
        );
    }

    #[test]
    fn highest_severity_none() {
        let problems = AllProblems {
            general: empty_general(),
            candidates: Vec::new(),
            lists: ListProblems {
                general: Vec::new(),
                per_list: Vec::new(),
            },
            info_problems: Vec::new(),
        };
        assert_eq!(problems.highest_severity(), None);
    }

    #[test]
    fn highest_severity_info() {
        let problems = AllProblems {
            general: empty_general(),
            candidates: Vec::new(),
            lists: ListProblems {
                general: Vec::new(),
                per_list: Vec::new(),
            },
            info_problems: vec![EntityInfoProblems::AnyProblem(
                InfoProblems::NoSubstituteSubmitter,
            )],
        };
        assert_eq!(problems.highest_severity(), Some(Severity::Info));
    }

    #[test]
    fn highest_severity_error() {
        let problems = AllProblems {
            general: empty_general(),
            candidates: vec![PersonProblems {
                entity: sample_person(PersonId::new()),
                problems: vec![PotentialProblems::NoCandidates], // error
            }],
            lists: ListProblems {
                general: Vec::new(),
                per_list: vec![EntityProblems {
                    entity: sample_candidate_list(CandidateListId::new()),
                    problems: vec![PotentialProblems::TooManyCandidates { count: 1 }], // warning
                }],
            },
            info_problems: vec![EntityInfoProblems::AnyProblem(
                InfoProblems::NoSubstituteSubmitter,
            )],
        };
        assert_eq!(problems.highest_severity(), Some(Severity::Error));
    }

    #[test]
    fn highest_severity_warn() {
        let problems = AllProblems {
            general: empty_general(),
            candidates: Vec::new(),
            lists: ListProblems {
                general: Vec::new(),
                per_list: vec![EntityProblems {
                    entity: sample_candidate_list(CandidateListId::new()),
                    problems: vec![PotentialProblems::TooManyCandidates { count: 1 }], // warning
                }],
            },
            info_problems: vec![EntityInfoProblems::AnyProblem(
                InfoProblems::NoSubstituteSubmitter,
            )],
        };
        assert_eq!(problems.highest_severity(), Some(Severity::Warn));
    }
}
