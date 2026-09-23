use crate::{
    AppError, CsbStream,
    structs::{
        candidate_lists::CandidateListId,
        common::Severity,
        persons::PersonId,
        problems::{GeneralProblems, ListProblems, PersonProblems},
    },
};

pub struct RestorationStatus {
    has_omissions: bool,
    has_corrections: bool,
    has_problems: Option<Severity>,
}

impl RestorationStatus {
    pub fn for_political_group(store: &CsbStream, problems: &GeneralProblems) -> Self {
        RestorationStatus {
            has_omissions: !store.get_political_group_omissions().is_empty(),
            has_corrections: store.get_political_group_csb_corrections_count() > 0,
            has_problems: problems.flatten().iter().map(|p| p.severity()).max(),
        }
    }

    pub fn for_candidate_list(
        store: &CsbStream,
        list_id: CandidateListId,
        problems: &ListProblems,
    ) -> Result<Self, AppError> {
        Ok(RestorationStatus {
            has_omissions: store.has_candidate_list_omissions(list_id)?,
            has_corrections: store.has_candidate_list_csb_corrections(list_id)?,
            has_problems: problems
                .per_list
                .iter()
                .find(|l| l.entity.id == list_id)
                .map(|ps| ps.problems.iter().map(|p| p.severity()).max())
                .flatten(),
        })
    }

    pub fn for_candidate(
        store: &CsbStream,
        person_id: PersonId,
        list_id: CandidateListId,
        problems: &Vec<PersonProblems>,
    ) -> Self {
        RestorationStatus {
            has_omissions: store.has_candidate_omissions(person_id, list_id),
            has_corrections: store.has_candidate_csb_corrections(person_id),
            has_problems: problems
                .iter()
                .find(|p| p.entity.id == person_id)
                .map(|ps| ps.problems.iter().map(|p| p.severity()).max())
                .flatten(),
        }
    }

    pub fn has_omissions(&self) -> bool {
        self.has_omissions
    }

    pub fn has_corrections(&self) -> bool {
        self.has_corrections
    }

    pub fn problems_severity(&self) -> Option<Severity> {
        self.has_problems
    }

    pub fn has_changes(&self) -> bool {
        self.has_omissions || self.has_corrections || self.has_problems.is_some()
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::{
        CsbAction, CsbStore,
        structs::{
            common::{Appellation, Initials, PotentialProblems},
            csb::{Correction, OmissionCategory, PersonCorrection, sample_omission},
            problems::{AllProblems, EntityProblems},
        },
        test_utils::{sample_candidate_list, sample_person},
    };

    use super::*;

    fn no_problems() -> AllProblems {
        AllProblems {
            general: GeneralProblems {
                general: Vec::new(),
                name_authorisations: Vec::new(),
                list_submitter: None,
                substitute_submitters: Vec::new(),
            },
            candidates: Vec::new(),
            lists: ListProblems {
                general: Vec::new(),
                per_list: Vec::new(),
            },
            info_problems: Vec::new(),
        }
    }

    #[test]
    fn for_political_group_no_changes() {
        let store = CsbStore::new_for_test();

        let status = RestorationStatus::for_political_group(&store, &no_problems().general);

        assert!(!status.has_omissions());
        assert!(!status.has_corrections());
        assert!(status.problems_severity().is_none());
    }

    #[tokio::test]
    async fn for_political_group_omission_and_correction() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();
        let mut problems = no_problems();
        problems.general.general = vec![PotentialProblems::NoAppellation];

        store
            .update(CsbAction::CreateOmission(sample_omission(
                OmissionCategory::PoliticalGroup,
            )))
            .await?;
        store
            .update(CsbAction::UpdateCorrection(Correction::Appellation(
                Appellation::from_str("Correction Party").unwrap(),
            )))
            .await?;

        let status = RestorationStatus::for_political_group(&store, &problems.general);

        assert!(status.has_omissions());
        assert!(status.has_corrections());
        assert!(status.problems_severity().is_some());

        Ok(())
    }

    #[tokio::test]
    async fn for_candidate_list_no_changes() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();

        let list_id1 = CandidateListId::new();
        let list_id2 = CandidateListId::new();

        let list2 = sample_candidate_list(list_id2);
        store.add_candidate_list(sample_candidate_list(list_id1));
        store.add_candidate_list(list2.clone());

        // add omission and problems belonging to another list
        store
            .update(CsbAction::CreateOmission(sample_omission(
                OmissionCategory::CandidateList(vec![list_id2]),
            )))
            .await?;

        let problems = ListProblems {
            general: vec![PotentialProblems::DuplicateDistricts],
            per_list: vec![EntityProblems {
                entity: list2,
                problems: vec![PotentialProblems::TooManyCandidates { count: 5 }],
            }],
        };

        let status = RestorationStatus::for_candidate_list(&store, list_id1, &problems)?;

        assert!(!status.has_omissions());
        assert!(!status.has_corrections());
        assert!(status.problems_severity().is_none());

        Ok(())
    }

    #[tokio::test]
    async fn for_candidate_list_omission_and_problems() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();

        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        store.add_candidate_list(list.clone());

        store
            .update(CsbAction::CreateOmission(sample_omission(
                OmissionCategory::CandidateList(vec![list_id]),
            )))
            .await?;

        let problems = ListProblems {
            general: vec![PotentialProblems::DuplicateDistricts],
            per_list: vec![EntityProblems {
                entity: list,
                problems: vec![PotentialProblems::TooManyCandidates { count: 5 }],
            }],
        };

        let status = RestorationStatus::for_candidate_list(&store, list_id, &problems)?;

        assert!(status.has_omissions());
        assert!(!status.has_corrections());
        assert!(status.problems_severity().is_some());

        Ok(())
    }

    #[tokio::test]
    async fn for_candidate_list_containing_candidate_omission_and_correction()
    -> Result<(), AppError> {
        let store = CsbStore::new_for_test();

        let list_id = CandidateListId::new();
        let person_id = PersonId::new();

        let mut list = sample_candidate_list(list_id);

        list.candidates.push(person_id);

        store.add_candidate_list(list);
        store.add_person(sample_person(person_id));

        store
            .update(CsbAction::CreateOmission(sample_omission(
                OmissionCategory::Candidate {
                    person: person_id,
                    lists: vec![list_id],
                },
            )))
            .await?;

        store
            .update(CsbAction::UpdateCorrection(Correction::Person(
                person_id,
                PersonCorrection::Initials(Initials::from_str("A.B.").unwrap()),
            )))
            .await?;

        let status =
            RestorationStatus::for_candidate_list(&store, list_id, &no_problems().lists).unwrap();

        assert!(status.has_omissions());
        assert!(status.has_corrections());
        assert!(status.problems_severity().is_none());

        Ok(())
    }

    #[tokio::test]
    async fn for_candidate_no_changes() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();

        let list_id1 = CandidateListId::new();
        let list_id2 = CandidateListId::new();

        let person_id = PersonId::new();

        let mut list1 = sample_candidate_list(list_id1);
        let mut list2 = sample_candidate_list(list_id2);

        list1.candidates.push(person_id);
        list2.candidates.push(person_id);

        store.add_person(sample_person(person_id));
        store.add_candidate_list(list1);
        store.add_candidate_list(list2);

        // add omission and problem for only 1 of the lists
        store
            .update(CsbAction::CreateOmission(sample_omission(
                OmissionCategory::Candidate {
                    person: person_id,
                    lists: vec![list_id1],
                },
            )))
            .await?;

        // retrieve status for the other list
        let status = RestorationStatus::for_candidate(
            &store,
            person_id,
            list_id2,
            &no_problems().candidates,
        );

        assert!(!status.has_omissions());
        assert!(!status.has_corrections());
        assert!(status.problems_severity().is_none());

        Ok(())
    }

    #[tokio::test]
    async fn for_candidate_omission_and_correction_and_problem() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();

        let list_id = CandidateListId::new();

        let person_id = PersonId::new();

        let mut list = sample_candidate_list(list_id);

        list.candidates.push(person_id);

        let person = sample_person(person_id);
        store.add_person(person.clone());
        store.add_candidate_list(list);

        store
            .update(CsbAction::CreateOmission(sample_omission(
                OmissionCategory::Candidate {
                    person: person_id,
                    lists: vec![list_id],
                },
            )))
            .await?;

        store
            .update(CsbAction::UpdateCorrection(Correction::Person(
                person_id,
                PersonCorrection::Initials(Initials::from_str("A.B.").unwrap()),
            )))
            .await?;
        let problems = vec![PersonProblems {
            entity: person,
            problems: vec![PotentialProblems::NoBsn],
        }];

        let status = RestorationStatus::for_candidate(&store, person_id, list_id, &problems);

        assert!(status.has_omissions());
        assert!(status.has_corrections());
        assert!(status.problems_severity().is_some());

        Ok(())
    }
}
