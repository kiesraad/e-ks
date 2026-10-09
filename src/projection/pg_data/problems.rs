//! Collecting the [`AllProblems`] of a political group from its store.

use crate::{
    AppError, ElectionConfig, PgStoreData,
    structs::{
        candidate_lists::CandidateListSummary,
        common::{InfoProblems, PotentialProblems, Problematic},
        list_designation::ListDesignation,
        list_submitters::ListSubmitter,
        name_authorisations::NameAuthorisation,
        problems::{
            AllProblems, EntityInfoProblems, EntityProblems, GeneralProblems, ListProblems,
            PersonProblems,
        },
    },
};

impl AllProblems {
    pub fn find_all(data: &PgStoreData, election: ElectionConfig) -> Result<Self, AppError> {
        let candidate_lists = CandidateListSummary::list(data, election);
        let (general, general_info) = Self::find_general_problems(data);
        let (candidates, candidates_info) =
            Self::find_candidate_problems(data, election, &candidate_lists);
        let mut lists = Self::find_list_problems(&candidate_lists, data);

        // candidate problems are already listed per candidate
        for list in &mut lists.per_list {
            list.problems
                .retain(|p| !matches!(p, PotentialProblems::CandidatesWithProblems { .. }));
        }
        lists.per_list.retain(|list| !list.problems.is_empty());

        let mut all_problems = Self {
            general,
            candidates,
            lists,
            info_problems: [general_info, candidates_info]
                .into_iter()
                .flatten()
                .collect(),
        };

        all_problems.sort_problems_by_severity();

        Ok(all_problems)
    }

    pub fn find_general_problems(data: &PgStoreData) -> (GeneralProblems, Vec<EntityInfoProblems>) {
        let mut info_problems = Vec::new();
        let mut general = Vec::new();

        let political_group = data.political_group();

        let pg_problems = political_group.get_problems(());
        info_problems.extend(
            pg_problems
                .info_problems
                .into_iter()
                .map(EntityInfoProblems::AnyProblem)
                .collect::<Vec<_>>(),
        );
        general.extend(pg_problems.potential_problems);

        let name_authorisations: Vec<NameAuthorisation> =
            data.name_authorisations().into_iter().cloned().collect();
        let name_authorisations = match political_group.list_designation {
            Some(ListDesignation::Blank) => Vec::new(),
            list_designation => {
                general.extend(NameAuthorisation::get_size_problems(
                    list_designation,
                    name_authorisations.len(),
                ));
                let (problems, infos) = Self::find_name_authorisation_problems(name_authorisations);
                info_problems.extend(infos);
                problems
            }
        };

        let list_submitter =
            Self::find_list_submitter_problems(data, &mut general, &mut info_problems);
        let substitute_submitters =
            Self::find_substitute_submitter_problems(data, &mut info_problems);

        (
            GeneralProblems {
                general,
                name_authorisations,
                list_submitter,
                substitute_submitters,
            },
            info_problems,
        )
    }

    /// Problems of the list submitter; a missing submitter is pushed onto
    /// `general` and info problems onto `info_problems`.
    fn find_list_submitter_problems(
        data: &PgStoreData,
        general: &mut Vec<PotentialProblems>,
        info_problems: &mut Vec<EntityInfoProblems>,
    ) -> Option<EntityProblems<ListSubmitter>> {
        let list_submitter = data.list_submitter();
        if list_submitter.is_empty() {
            general.push(PotentialProblems::NoListSubmitter);
        }

        let problems = list_submitter.get_problems(());
        info_problems.extend(
            problems
                .info_problems
                .into_iter()
                .map(|problem| EntityInfoProblems::Submitter { problem }),
        );

        if problems.potential_problems.is_empty() {
            return None;
        }
        Some(EntityProblems {
            entity: list_submitter.clone(),
            problems: problems.potential_problems,
        })
    }

    /// Problems per substitute submitter; info problems are pushed onto
    /// `info_problems`, including one when there is no substitute at all.
    fn find_substitute_submitter_problems(
        data: &PgStoreData,
        info_problems: &mut Vec<EntityInfoProblems>,
    ) -> Vec<EntityProblems<ListSubmitter>> {
        let submitters = data.substitute_submitters();
        if submitters.is_empty() {
            info_problems.push(EntityInfoProblems::AnyProblem(
                InfoProblems::NoSubstituteSubmitter,
            ));
        }

        let mut substitute_submitters = Vec::new();
        for ss in submitters {
            let (ss_problems, infos) = EntityProblems::new(ss.clone());
            if !ss_problems.problems.is_empty() {
                substitute_submitters.push(ss_problems)
            }
            info_problems.extend(infos.into_iter().map(|problem| {
                EntityInfoProblems::SubstituteSubmitter {
                    submitter: ss.clone(),
                    problem,
                }
            }));
        }
        substitute_submitters
    }

    fn find_name_authorisation_problems(
        name_authorisations: Vec<NameAuthorisation>,
    ) -> (
        Vec<EntityProblems<NameAuthorisation>>,
        Vec<EntityInfoProblems>,
    ) {
        let mut problems = Vec::new();
        let mut info_problems = Vec::new();
        for name_authorisation in name_authorisations {
            let (na_problems, na_info_problems) = EntityProblems::new(name_authorisation.clone());
            if !na_problems.problems.is_empty() {
                problems.push(na_problems)
            }
            info_problems.extend(
                na_info_problems
                    .into_iter()
                    .map(|problem| EntityInfoProblems::NameAuthorisation {
                        name_authorisation: name_authorisation.clone(),
                        problem,
                    })
                    .collect::<Vec<_>>(),
            );
        }
        (problems, info_problems)
    }

    pub fn find_candidate_problems(
        data: &PgStoreData,
        election: ElectionConfig,
        candidate_lists: &[CandidateListSummary],
    ) -> (Vec<PersonProblems>, Vec<EntityInfoProblems>) {
        let mut info_problems = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let problems = candidate_lists
            .iter()
            .flat_map(|list| list.list.candidates.iter())
            .filter(|id| seen.insert(*id))
            .filter_map(|id| data.person(*id).cloned())
            .filter_map(|person| {
                let problems = person.get_problems(election);
                info_problems.extend(
                    problems
                        .info_problems
                        .into_iter()
                        .map(|problem| EntityInfoProblems::Person {
                            person: Box::new(person.clone()),
                            problem,
                        })
                        .collect::<Vec<_>>(),
                );
                (!problems.potential_problems.is_empty()).then_some(PersonProblems {
                    entity: person,
                    problems: problems.potential_problems,
                })
            })
            .collect();
        (problems, info_problems)
    }

    pub fn find_list_problems(
        candidate_lists: &[CandidateListSummary],
        data: &PgStoreData,
    ) -> ListProblems {
        let mut list_problems = Vec::new();
        let mut seen_duplicate_district = false;
        for candidate_list in candidate_lists {
            let mut problems = candidate_list.get_problems(());
            if problems
                .potential_problems
                .contains(&PotentialProblems::DuplicateDistricts)
            {
                if seen_duplicate_district {
                    problems
                        .potential_problems
                        .retain(|problem| problem != &PotentialProblems::DuplicateDistricts)
                }
                seen_duplicate_district = true;
            }
            if !problems.potential_problems.is_empty() {
                list_problems.push(EntityProblems {
                    entity: candidate_list.list.clone(),
                    problems: problems.potential_problems,
                })
            }
        }

        let general = if data.candidate_list_count() == 0 {
            vec![PotentialProblems::NoCandidateList]
        } else {
            Vec::new()
        };

        ListProblems {
            general,
            per_list: list_problems,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::{
        ElectoralDistrict, PgStore,
        structs::{
            candidate_lists::CandidateListId, common::Severity, list_submitters::ListSubmitterId,
            name_authorisations::NameAuthorisationId, persons::PersonId,
        },
        test_utils::{
            sample_candidate_list, sample_list_submitter, sample_name_authorisation, sample_person,
        },
    };

    async fn add_submitters(store: &PgStore) -> Result<(), AppError> {
        sample_list_submitter(ListSubmitterId::new())
            .update(store)
            .await?;
        sample_list_submitter(ListSubmitterId::new())
            .create_substitute(store)
            .await?;
        Ok(())
    }

    async fn add_name_authorisations(store: &PgStore, count: usize) -> Result<(), AppError> {
        for _ in 0..count {
            sample_name_authorisation(NameAuthorisationId::new())
                .create(store)
                .await?;
        }
        Ok(())
    }

    async fn add_candidate_list(store: &PgStore) -> Result<(), AppError> {
        sample_candidate_list(CandidateListId::new())
            .create(store)
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn candidate_problems_propagate_to_list_but_not_on_finalise() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let mut person = sample_person(PersonId::new());
        person.personal_data.date_of_birth = None;
        person.create(&store).await?;

        let mut list = sample_candidate_list(CandidateListId::new());
        list.candidates = vec![person.id];
        list.create(&store).await?;

        let summaries = CandidateListSummary::list(&store.snapshot(), store.election);
        let list_problems = AllProblems::find_list_problems(&summaries, &store.snapshot());
        assert_eq!(list_problems.per_list.len(), 1);
        assert_eq!(
            list_problems.per_list[0].problems,
            vec![PotentialProblems::CandidatesWithProblems { count: 1 }]
        );
        assert_eq!(list_problems.highest_severity(), Some(Severity::Warn));

        let all = AllProblems::find_all(&store.snapshot(), store.election)?;
        assert!(all.lists.per_list.is_empty());
        assert_eq!(all.candidates.len(), 1);

        Ok(())
    }

    #[tokio::test]
    async fn no_candidate_list_added() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        let problems = AllProblems::find_list_problems(&[], &store.snapshot());

        assert_eq!(problems.general.len(), 1);

        assert_eq!(problems.general[0], PotentialProblems::NoCandidateList);

        Ok(())
    }

    #[tokio::test]
    async fn standalone_no_name_authorisations() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        // make sure no other general errors occur
        add_submitters(&store).await?;
        add_candidate_list(&store).await?;

        // make political group standalone
        let mut group = store.get_political_group();
        group.list_designation = Some(ListDesignation::Standalone);
        group.update(&store).await?;

        let (problems, _) = AllProblems::find_general_problems(&store.snapshot());

        assert_eq!(problems.general.len(), 1);
        assert_eq!(
            problems.general[0],
            PotentialProblems::TooFewAuthorizedNames { count: 1 }
        );

        Ok(())
    }

    #[tokio::test]
    async fn standalone_two_name_authorisations() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        // make sure no other general errors occur
        add_submitters(&store).await?;
        add_candidate_list(&store).await?;

        // make political group standalone
        let mut group = store.get_political_group();
        group.list_designation = Some(ListDesignation::Standalone);
        group.update(&store).await?;

        add_name_authorisations(&store, 2).await?;

        let (problems, _) = AllProblems::find_general_problems(&store.snapshot());

        assert_eq!(problems.general.len(), 1);
        assert_eq!(
            problems.general[0],
            PotentialProblems::TooManyAuthorizedNames { count: 1 }
        );

        Ok(())
    }

    #[tokio::test]
    async fn combined_one_name_authorisations() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        // make sure no other general errors occur
        add_submitters(&store).await?;
        add_candidate_list(&store).await?;

        // make political group standalone
        let mut group = store.get_political_group();
        group.list_designation = Some(ListDesignation::Combined);
        group.update(&store).await?;

        add_name_authorisations(&store, 1).await?;

        let (problems, _) = AllProblems::find_general_problems(&store.snapshot());

        assert_eq!(problems.general.len(), 1);
        assert_eq!(
            problems.general[0],
            PotentialProblems::TooFewAuthorizedNames { count: 1 }
        );

        Ok(())
    }

    #[tokio::test]
    async fn blank_ten_name_authorisations() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        // make sure no other general errors occur
        add_submitters(&store).await?;
        add_candidate_list(&store).await?;

        // make political group standalone
        let mut group = store.get_political_group();
        group.list_designation = Some(ListDesignation::Blank);
        group.update(&store).await?;

        add_name_authorisations(&store, 10).await?;

        let (problems, _) = AllProblems::find_general_problems(&store.snapshot());

        assert!(problems.general.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn max_one_duplicate_district_problem() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        for _ in 0..10 {
            let mut list1 = sample_candidate_list(CandidateListId::new());
            list1.electoral_districts =
                BTreeSet::from([ElectoralDistrict::Utrecht, ElectoralDistrict::Groningen]);
            list1.create(&store).await?;
        }

        let problems = AllProblems::find_all(&store.snapshot(), store.election)?;
        assert_eq!(
            problems
                .lists
                .per_list
                .iter()
                .flat_map(|list_problems| &list_problems.problems)
                .filter(|problem| **problem == PotentialProblems::DuplicateDistricts)
                .count(),
            1
        );

        Ok(())
    }
}
