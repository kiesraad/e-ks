//! Where the finalise page sends each problem to be fixed.

use axum_extra::routing::TypedPath as _;

use crate::{
    QueryParamState,
    common::PgIndexPath,
    finalise::FinalisePath,
    structs::{
        candidate_lists::CandidateList,
        common::{InfoProblems, PotentialProblems},
        list_designation::ListDesignation,
        list_submitters::ListSubmitter,
        name_authorisations::NameAuthorisation,
        persons::Person,
        political_groups::PoliticalGroup,
        problems::EntityInfoProblems,
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
        match self {
            PotentialProblems::IncompleteAddress { .. } | PotentialProblems::UnknownAddress => {
                person
                    .update_address_path()
                    .with_query_params(QueryParamState::redirect_to(FinalisePath.to_string()))
                    .to_string()
            }
            PotentialProblems::NoRepresentative | PotentialProblems::RepresentativeProblem(_) => {
                person
                    .update_representative_path()
                    .with_query_params(QueryParamState::redirect_to(FinalisePath.to_string()))
                    .to_string()
            }
            _ => person.update_path_from(FinalisePath).to_string(),
        }
    }

    pub fn general_fix_path(&self) -> String {
        match self {
            PotentialProblems::NoAuthorisedAgent | PotentialProblems::NoLegalName => {
                NameAuthorisation::list_path().to_string()
            }
            PotentialProblems::NoListSubmitter => {
                ListSubmitter::update_path_from(FinalisePath).to_string()
            }
            PotentialProblems::NoCandidateList
            | PotentialProblems::BlankWithDifferentFirstCandidates => {
                CandidateList::list_path().to_string()
            }

            PotentialProblems::NoAuthorisedName
            | PotentialProblems::TooFewAuthorizedNames { .. } => NameAuthorisation::create_path()
                .with_query_params(QueryParamState::redirect_to(FinalisePath.to_string()))
                .to_string(),
            PotentialProblems::TooManyAuthorizedNames { .. } => {
                NameAuthorisation::list_path().to_string()
            }
            _ => PoliticalGroup::update_path().to_string(),
        }
    }
}

impl EntityInfoProblems {
    pub fn fix_path(&self) -> String {
        match self {
            EntityInfoProblems::AnyProblem(InfoProblems::NoSubstituteSubmitter) => {
                ListSubmitter::substitute_create_path()
                    .with_query_params(QueryParamState::redirect_to(FinalisePath.to_string()))
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
                .substitute_update_path_from(FinalisePath)
                .to_string(),
            EntityInfoProblems::Submitter { .. } => {
                ListSubmitter::update_path_from(FinalisePath).to_string()
            }
            EntityInfoProblems::Person { person, .. } => {
                person.update_path_from(FinalisePath).to_string()
            }
            EntityInfoProblems::NameAuthorisation {
                name_authorisation, ..
            } => name_authorisation
                .update_path_from(FinalisePath)
                .to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        structs::{
            common::{EmptyAddressProblems, Severity},
            list_submitters::ListSubmitterId,
            name_authorisations::NameAuthorisationId,
            persons::PersonId,
        },
        test_utils::{sample_list_submitter, sample_name_authorisation, sample_person},
    };

    use super::*;

    #[test]
    fn entity_info_fix_paths_lead_to_the_expected_destinations() {
        use EntityInfoProblems::{
            AnyProblem, NameAuthorisation, Person, Submitter, SubstituteSubmitter,
        };

        fn from_finalise(path: impl axum_extra::routing::TypedPath) -> String {
            path.with_query_params(QueryParamState::redirect_to("/finalise".into()))
                .to_string()
        }

        let substitute = sample_list_submitter(ListSubmitterId::new());
        let person = sample_person(PersonId::new());
        let name_authorisation = sample_name_authorisation(NameAuthorisationId::new());
        for (expected, problem) in [
            (
                from_finalise(ListSubmitter::substitute_create_path()),
                AnyProblem(InfoProblems::NoSubstituteSubmitter),
            ),
            (
                ListDesignation::update_path().to_string(),
                AnyProblem(InfoProblems::NoListDesignation),
            ),
            (
                PoliticalGroup::update_path().to_string(),
                AnyProblem(InfoProblems::NoPreviousElectionResults),
            ),
            (
                PgIndexPath.to_string(),
                AnyProblem(InfoProblems::NoInitials),
            ),
            (
                from_finalise(ListSubmitter::update_path()),
                Submitter {
                    problem: InfoProblems::NoLastName,
                },
            ),
            (
                from_finalise(substitute.substitute_update_path()),
                SubstituteSubmitter {
                    submitter: substitute,
                    problem: InfoProblems::NoInitials,
                },
            ),
            (
                from_finalise(person.update_path()),
                Person {
                    person: Box::new(person),
                    problem: InfoProblems::VeryOldDateOfBirth,
                },
            ),
            (
                from_finalise(name_authorisation.update_path()),
                NameAuthorisation {
                    name_authorisation,
                    problem: InfoProblems::NoInitials,
                },
            ),
        ] {
            assert_eq!(problem.fix_path(), expected, "{problem:?}");
        }
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
