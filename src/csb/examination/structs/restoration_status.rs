use crate::{
    AppError, CsbStream,
    structs::{candidate_lists::CandidateListId, persons::PersonId},
};

pub struct RestorationStatus {
    has_omissions: bool,
    has_corrections: bool,
}

impl RestorationStatus {
    pub fn for_political_group(store: &CsbStream) -> Self {
        RestorationStatus {
            has_omissions: !store.get_political_group_omissions().is_empty(),
            has_corrections: store.get_political_group_csb_corrections_count() > 0,
        }
    }

    pub fn for_candidate_list(
        store: &CsbStream,
        list_id: CandidateListId,
    ) -> Result<Self, AppError> {
        Ok(RestorationStatus {
            has_omissions: store.has_candidate_list_omissions(list_id)?,
            has_corrections: store.has_candidate_list_csb_corrections(list_id)?,
        })
    }

    pub fn for_candidate(store: &CsbStream, person_id: PersonId, list_id: CandidateListId) -> Self {
        RestorationStatus {
            has_omissions: store.has_candidate_omissions(person_id, list_id),
            has_corrections: store.has_candidate_csb_corrections(person_id),
        }
    }

    pub fn has_omissions(&self) -> bool {
        self.has_omissions
    }

    pub fn has_corrections(&self) -> bool {
        self.has_corrections
    }

    pub fn has_changes(&self) -> bool {
        self.has_omissions || self.has_corrections
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::{
        CsbAction, CsbStore,
        structs::{
            common::{Appellation, Initials},
            csb::{Correction, OmissionCategory, PersonCorrection, sample_omission},
        },
        test_utils::{sample_candidate_list, sample_person},
    };

    use super::*;

    #[test]
    fn for_political_group_no_changes() {
        let store = CsbStore::new_for_test();

        let status = RestorationStatus::for_political_group(&store);

        assert!(!status.has_omissions());
        assert!(!status.has_corrections());
    }

    #[tokio::test]
    async fn for_political_group_omission_and_correction() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();

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

        let status = RestorationStatus::for_political_group(&store);

        assert!(status.has_omissions());
        assert!(status.has_corrections());

        Ok(())
    }

    #[tokio::test]
    async fn for_candidate_list_no_changes() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();

        let list_id1 = CandidateListId::new();
        let list_id2 = CandidateListId::new();

        store.add_candidate_list(sample_candidate_list(list_id1));
        store.add_candidate_list(sample_candidate_list(list_id2));

        // add omission belonging to another list
        store
            .update(CsbAction::CreateOmission(sample_omission(
                OmissionCategory::CandidateList(vec![list_id2]),
            )))
            .await?;

        let status = RestorationStatus::for_candidate_list(&store, list_id1).unwrap();

        assert!(!status.has_omissions());
        assert!(!status.has_corrections());

        Ok(())
    }

    #[tokio::test]
    async fn for_candidate_list_omission() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();

        let list_id = CandidateListId::new();

        store.add_candidate_list(sample_candidate_list(list_id));

        store
            .update(CsbAction::CreateOmission(sample_omission(
                OmissionCategory::CandidateList(vec![list_id]),
            )))
            .await?;

        let status = RestorationStatus::for_candidate_list(&store, list_id).unwrap();

        assert!(status.has_omissions());
        assert!(!status.has_corrections());

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

        let status = RestorationStatus::for_candidate_list(&store, list_id).unwrap();

        assert!(status.has_omissions());
        assert!(status.has_corrections());

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

        // add omission for only 1 of the lists
        store
            .update(CsbAction::CreateOmission(sample_omission(
                OmissionCategory::Candidate {
                    person: person_id,
                    lists: vec![list_id1],
                },
            )))
            .await?;

        // retrieve status for the other list
        let status = RestorationStatus::for_candidate(&store, person_id, list_id2);

        assert!(!status.has_omissions());
        assert!(!status.has_corrections());

        Ok(())
    }

    #[tokio::test]
    async fn for_candidate_omission_and_correction() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();

        let list_id = CandidateListId::new();

        let person_id = PersonId::new();

        let mut list = sample_candidate_list(list_id);

        list.candidates.push(person_id);

        store.add_person(sample_person(person_id));
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

        let status = RestorationStatus::for_candidate(&store, person_id, list_id);

        assert!(status.has_omissions());
        assert!(status.has_corrections());

        Ok(())
    }
}
