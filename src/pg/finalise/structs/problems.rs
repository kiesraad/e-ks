use axum_extra::routing::TypedPath as _;

use crate::{
    AppError, Locale, PgStore, QueryParamState,
    common::PgIndexPath,
    finalise::FinalisePath,
    structs::{
        candidate_lists::{CandidateList, CandidateListSummary},
        common::{HasSeverity, InfoProblems, PotentialProblems, Problematic, Severity},
        list_designation::ListDesignation,
        list_submitters::ListSubmitter,
        name_authorisations::NameAuthorisation,
        persons::Person,
        political_groups::PoliticalGroup,
    },
};

impl PotentialProblems {
    pub fn candidate_list_fix_path(&self, list: &CandidateList) -> String {
        match self {
            PotentialProblems::NoCandidates => list.view_path().to_string(),
            PotentialProblems::TooManyCandidates { count } => list
                .view_path()
                .with_query_params(QueryParamState::highlight_last(*count))
                .to_string(),
            PotentialProblems::DuplicateDistricts => CandidateList::list_path().to_string(),
            _ => list.view_path().to_string(),
        }
    }

    pub fn person_fix_path(&self, person: &Person) -> String {
        let finalise = FinalisePath {}.to_string();
        match self {
            PotentialProblems::IncompleteAddress { .. } | PotentialProblems::UnknownAddress => {
                person
                    .update_address_path()
                    .with_query_params(QueryParamState::redirect_to(finalise))
                    .to_string()
            }
            PotentialProblems::NoRepresentative | PotentialProblems::RepresentativeProblem(_) => {
                person
                    .update_representative_path()
                    .with_query_params(QueryParamState::redirect_to(finalise))
                    .to_string()
            }
            _ => person
                .update_path()
                .with_query_params(QueryParamState::redirect_to(finalise))
                .to_string(),
        }
    }

    pub fn general_fix_path(&self) -> String {
        let finalise = FinalisePath {}.to_string();
        match self {
            PotentialProblems::NoAuthorisedAgent | PotentialProblems::NoLegalName => {
                NameAuthorisation::list_path().to_string()
            }
            PotentialProblems::NoListSubmitter => ListSubmitter::update_path()
                .with_query_params(QueryParamState::redirect_to(finalise))
                .to_string(),
            PotentialProblems::NoCandidateList => CandidateList::list_path().to_string(),

            PotentialProblems::TooFewAuthorizedNames { .. } => NameAuthorisation::create_path()
                .with_query_params(QueryParamState::redirect_to(finalise))
                .to_string(),
            PotentialProblems::TooManyAuthorizedNames { .. } => {
                NameAuthorisation::list_path().to_string()
            }
            _ => PoliticalGroup::update_path().to_string(),
        }
    }
}

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
    pub fn find_all(store: &PgStore) -> Result<Self, AppError> {
        let candidate_lists = CandidateListSummary::list(store);
        let (general, general_info) = Self::find_general_problems(store);
        let (candidates, candidates_info) = Self::find_candidate_problems(store, &candidate_lists);
        let lists = Self::find_list_problems(&candidate_lists, store);

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

    pub fn find_general_problems(store: &PgStore) -> (GeneralProblems, Vec<EntityInfoProblems>) {
        let mut info_problems = Vec::new();
        let mut general = Vec::new();

        let political_group = store.get_political_group();

        let pg_problems = political_group.get_problems(());
        info_problems.extend(
            pg_problems
                .info_problems
                .into_iter()
                .map(EntityInfoProblems::AnyProblem)
                .collect::<Vec<_>>(),
        );
        general.extend(pg_problems.potential_problems);

        let name_authorisations = store.get_name_authorisations();
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
            Self::find_list_submitter_problems(store, &mut general, &mut info_problems);
        let substitute_submitters =
            Self::find_substitute_submitter_problems(store, &mut info_problems);

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
        store: &PgStore,
        general: &mut Vec<PotentialProblems>,
        info_problems: &mut Vec<EntityInfoProblems>,
    ) -> Option<EntityProblems<ListSubmitter>> {
        let list_submitter = store.get_list_submitter();
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
            entity: list_submitter,
            problems: problems.potential_problems,
        })
    }

    /// Problems per substitute submitter; info problems are pushed onto
    /// `info_problems`, including one when there is no substitute at all.
    fn find_substitute_submitter_problems(
        store: &PgStore,
        info_problems: &mut Vec<EntityInfoProblems>,
    ) -> Vec<EntityProblems<ListSubmitter>> {
        let submitters = store.get_substitute_submitters();
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
        store: &PgStore,
        candidate_lists: &[CandidateListSummary],
    ) -> (Vec<PersonProblems>, Vec<EntityInfoProblems>) {
        let mut info_problems = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let problems = candidate_lists
            .iter()
            .flat_map(|list| list.list.candidates.iter())
            .filter(|id| seen.insert(*id))
            .filter_map(|id| store.get_person(*id).ok())
            .filter_map(|person| {
                let problems = person.get_problems(store.election);
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
        store: &PgStore,
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

        let general = if store.get_candidate_list_count() == 0 {
            vec![PotentialProblems::NoCandidateList]
        } else {
            Vec::new()
        };

        ListProblems {
            general,
            per_list: list_problems,
        }
    }

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
}

#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq, Clone))]
pub struct EntityProblems<T> {
    pub entity: T,
    pub problems: Vec<PotentialProblems>,
}

impl<T: Problematic<()>> EntityProblems<T> {
    fn new(entity: T) -> (Self, Vec<InfoProblems>) {
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
    pub fn fix_path(&self) -> String {
        let finalise = FinalisePath {}.to_string();
        match self {
            EntityInfoProblems::AnyProblem(InfoProblems::NoSubstituteSubmitter) => {
                ListSubmitter::substitute_create_path()
                    .with_query_params(QueryParamState::redirect_to(finalise))
                    .to_string()
            }
            EntityInfoProblems::AnyProblem(InfoProblems::NoListDesignation) => {
                ListDesignation::update_path().to_string()
            }
            EntityInfoProblems::AnyProblem(InfoProblems::NoPreviousElectionResults) => {
                PoliticalGroup::update_path().to_string()
            }
            EntityInfoProblems::AnyProblem(..) => PgIndexPath.to_string(),

            EntityInfoProblems::SubstituteSubmitter { submitter, .. } => submitter
                .substitute_update_path()
                .with_query_params(QueryParamState::redirect_to(finalise))
                .to_string(),
            EntityInfoProblems::Submitter { .. } => ListSubmitter::update_path()
                .with_query_params(QueryParamState::redirect_to(finalise))
                .to_string(),
            EntityInfoProblems::Person { person, .. } => person
                .update_path()
                .with_query_params(QueryParamState::redirect_to(finalise))
                .to_string(),
            EntityInfoProblems::NameAuthorisation {
                name_authorisation, ..
            } => name_authorisation
                .update_path()
                .with_query_params(QueryParamState::redirect_to(finalise))
                .to_string(),
        }
    }
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
        AppError, ElectoralDistrict,
        structs::{
            candidate_lists::CandidateListId,
            common::{EmptyAddressProblems, HasSeverity},
            list_submitters::ListSubmitterId,
            name_authorisations::NameAuthorisationId,
            persons::PersonId,
        },
        test_utils::{
            sample_candidate_list, sample_list_submitter, sample_name_authorisation, sample_person,
        },
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

    #[tokio::test]
    async fn no_candidate_list_added() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        let problems = AllProblems::find_list_problems(&[], &store);

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

        let (problems, _) = AllProblems::find_general_problems(&store);

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

        let (problems, _) = AllProblems::find_general_problems(&store);

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

        let (problems, _) = AllProblems::find_general_problems(&store);

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

        let (problems, _) = AllProblems::find_general_problems(&store);

        assert!(problems.general.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn max_one_duplicate_district_problem() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        for _ in 0..10 {
            let mut list1 = sample_candidate_list(CandidateListId::new());
            list1.electoral_districts =
                vec![ElectoralDistrict::Utrecht, ElectoralDistrict::Groningen];
            list1.create(&store).await?;
        }

        let problems = AllProblems::find_all(&store)?;
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

    #[test]
    fn person_fix_path_points_at_the_form_that_holds_the_field() {
        let person = sample_person(PersonId::new());

        // Both address problems are fixed on the correspondence address form.
        for problem in [
            PotentialProblems::UnknownAddress,
            PotentialProblems::IncompleteAddress {
                severity: Severity::Warn,
                problems: vec![EmptyAddressProblems::PostalCode],
            },
        ] {
            assert!(
                problem
                    .person_fix_path(&person)
                    .starts_with(&person.update_address_path().to_string()),
                "{problem:?} should link to the address form"
            );
        }

        assert!(
            PotentialProblems::RepresentativeProblem(Box::new(PotentialProblems::UnknownAddress))
                .person_fix_path(&person)
                .starts_with(&person.update_representative_path().to_string())
        );

        assert!(
            PotentialProblems::NoBsn
                .person_fix_path(&person)
                .starts_with(&person.update_path().to_string())
        );
    }
}
