use std::cmp;

use crate::structs::{common::Severity, problems::{AllProblems, EntityProblems}};

impl AllProblems {
    pub fn sort_problems_by_severity(&mut self) {
        // general sorting
        self.general
            .general
            .sort_by_key(|p| cmp::Reverse(p.severity()));
        self.general
            .name_authorisations
            .iter_mut()
            .for_each(|ep| ep.problems.sort_by_key(|p| cmp::Reverse(p.severity())));
        if let Some(ls) = self.general.list_submitter.as_mut() {
            ls.problems.sort_by_key(|p| cmp::Reverse(p.severity()))
        }
        self.general
            .substitute_submitters
            .iter_mut()
            .for_each(|ss| ss.problems.sort_by_key(|p| cmp::Reverse(p.severity())));

        // candidate sorting
        self.candidates
            .iter_mut()
            .for_each(|c| c.problems.sort_by_key(|p| cmp::Reverse(p.severity())));

        // list sorting
        self.lists
            .general
            .sort_by_key(|p| cmp::Reverse(p.severity()));

        let mut list_error_problems = Vec::new();
        let mut list_other_problems = Vec::new();
        self.lists.per_list.iter().for_each(|l| {
            list_error_problems.push(EntityProblems {
                entity: l.entity.clone(),
                problems: l
                    .problems
                    .iter()
                    .filter(|p| p.severity() == Severity::Error)
                    .cloned()
                    .collect(),
            });
            list_other_problems.push(EntityProblems {
                entity: l.entity.clone(),
                problems: l
                    .problems
                    .iter()
                    .filter(|p| p.severity() != Severity::Error)
                    .cloned()
                    .collect(),
            });
        });
        list_error_problems.extend(list_other_problems);
        list_error_problems.retain(|p| !p.problems.is_empty());
        self.lists.per_list = list_error_problems;
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        structs::{
            candidate_lists::CandidateListId, common::{
                InfoProblems::{self},
                PotentialProblems,
            }, list_submitters::ListSubmitterId, name_authorisations::NameAuthorisationId, persons::PersonId, problems::{EntityInfoProblems, GeneralProblems, ListProblems},
        }, test_utils::{
            sample_candidate_list, sample_list_submitter, sample_name_authorisation, sample_person,
        },
    };

    use super::*;

    fn problem_vec_unordered() -> Vec<PotentialProblems> {
        vec![
            PotentialProblems::NoInitials(Severity::Warn),
            PotentialProblems::NoInitials(Severity::Error),
        ]
    }

    fn problem_vec_ordered() -> Vec<PotentialProblems> {
        vec![
            PotentialProblems::NoInitials(Severity::Error),
            PotentialProblems::NoInitials(Severity::Warn),
        ]
    }

    /// Unordered problems attached to two fresh sample entities.
    fn unordered_pair<T>(sample: impl Fn() -> T) -> Vec<EntityProblems<T>> {
        (0..2)
            .map(|_| EntityProblems {
                entity: sample(),
                problems: problem_vec_unordered(),
            })
            .collect()
    }

    /// An `AllProblems` with unordered problems for every entity kind.
    fn unsorted_all_problems() -> AllProblems {
        AllProblems {
            general: GeneralProblems {
                general: problem_vec_unordered(),
                name_authorisations: unordered_pair(|| {
                    sample_name_authorisation(NameAuthorisationId::new())
                }),
                list_submitter: Some(EntityProblems {
                    entity: sample_list_submitter(ListSubmitterId::new()),
                    problems: problem_vec_unordered(),
                }),
                substitute_submitters: unordered_pair(|| {
                    sample_list_submitter(ListSubmitterId::new())
                }),
            },
            candidates: unordered_pair(|| sample_person(PersonId::new())),
            lists: ListProblems {
                general: problem_vec_unordered(),
                per_list: unordered_pair(|| sample_candidate_list(CandidateListId::new())),
            },
            info_problems: vec![
                EntityInfoProblems::Person {
                    person: Box::new(sample_person(PersonId::new())),
                    problem: InfoProblems::NoLastName,
                },
                EntityInfoProblems::Person {
                    person: Box::new(sample_person(PersonId::new())),
                    problem: InfoProblems::NoInitials,
                },
            ],
        }
    }

    /// The same entities with their problems in sorted order.
    fn with_ordered_problems<T: Clone>(source: &[EntityProblems<T>]) -> Vec<EntityProblems<T>> {
        source
            .iter()
            .map(|ep| EntityProblems {
                entity: ep.entity.clone(),
                problems: problem_vec_ordered(),
            })
            .collect()
    }

    #[test]
    fn sort_by_severity() {
        let mut all_problems = unsorted_all_problems();
        let original = all_problems.clone();

        all_problems.sort_problems_by_severity();

        // Per-list problems are regrouped: every list's errors first, then
        // every list's warnings.
        let expected_per_list: Vec<_> = [Severity::Error, Severity::Warn]
            .into_iter()
            .flat_map(|severity| {
                original
                    .lists
                    .per_list
                    .iter()
                    .map(move |ep| EntityProblems {
                        entity: ep.entity.clone(),
                        problems: vec![PotentialProblems::NoInitials(severity)],
                    })
            })
            .collect();

        assert_eq!(
            all_problems,
            AllProblems {
                general: GeneralProblems {
                    general: problem_vec_ordered(),
                    name_authorisations: with_ordered_problems(
                        &original.general.name_authorisations
                    ),
                    list_submitter: original.general.list_submitter.as_ref().map(|ls| {
                        EntityProblems {
                            entity: ls.entity.clone(),
                            problems: problem_vec_ordered(),
                        }
                    }),
                    substitute_submitters: with_ordered_problems(
                        &original.general.substitute_submitters
                    ),
                },
                candidates: with_ordered_problems(&original.candidates),
                lists: ListProblems {
                    general: problem_vec_ordered(),
                    per_list: expected_per_list,
                },
                info_problems: original.info_problems.clone(),
            }
        )
    }

    #[test]
    fn sort_by_severity_is_idempotent() {
        let mut all_problems = unsorted_all_problems();
        let original = all_problems.clone();

        all_problems.sort_problems_by_severity();
        let after_sort1 = all_problems.clone();

        all_problems.sort_problems_by_severity();
        let after_sort2 = all_problems;

        assert_ne!(original, after_sort1);
        assert_eq!(after_sort1, after_sort2);
    }
}
