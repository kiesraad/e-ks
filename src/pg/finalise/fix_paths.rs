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
}

#[cfg(test)]
mod tests {
    use crate::{
        structs::{
            common::{EmptyAddressProblems, Severity},
            persons::PersonId,
        },
        test_utils::sample_person,
    };

    use super::*;

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
