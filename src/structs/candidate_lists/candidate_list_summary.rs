use serde::{Deserialize, Serialize};

use crate::{
    ElectoralDistrict,
    structs::{
        candidate_lists::CandidateList,
        common::{PotentialProblems, Problematic, Problems, WithProblems},
    },
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CandidateListSummary {
    pub list: CandidateList,
    pub max_count: usize,
    pub duplicate_districts: Vec<ElectoralDistrict>,
    /// Candidates on this list with at least one warning or error
    pub candidates_with_problems: usize,
}

pub type CandidateListWithProblems = WithProblems<CandidateListSummary>;

impl Problematic<()> for CandidateListSummary {
    fn get_problems(&self, _: ()) -> Problems {
        Problems {
            potential_problems: self.get_potential_problems(),
            info_problems: Vec::new(),
        }
    }
}

impl CandidateListSummary {
    fn get_potential_problems(&self) -> Vec<PotentialProblems> {
        let mut items = Vec::new();

        if !self.duplicate_districts.is_empty() {
            items.push(PotentialProblems::DuplicateDistricts);
        }

        if self.list.electoral_districts.is_empty() {
            items.push(PotentialProblems::NoDistricts)
        }

        if self.candidate_count() == 0 {
            items.push(PotentialProblems::NoCandidates);
        } else if self.candidate_count() > self.max_count {
            items.push(PotentialProblems::TooManyCandidates {
                count: self.candidate_count() - self.max_count,
            });
        }

        if self.candidates_with_problems > 0 {
            items.push(PotentialProblems::CandidatesWithProblems {
                count: self.candidates_with_problems,
            });
        }

        items
    }

    pub fn candidate_count(&self) -> usize {
        self.list.candidates.len()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        AppError,
        ElectoralDistrict::PsAmsterdam,
        PgStore,
        structs::{
            candidate_lists::CandidateListId,
            common::{HasSeverity, Severity},
            persons::PersonId,
        },
        test_utils::{sample_candidate_list, sample_person},
    };
    use std::collections::BTreeSet;

    use super::*;

    async fn make_store_with_candidates(
        persons: Vec<crate::structs::persons::Person>,
    ) -> Result<(PgStore, Vec<PersonId>), AppError> {
        let store = PgStore::new_for_test();
        let ids = persons.iter().map(|p| p.id).collect();
        for person in persons {
            person.create(&store).await?;
        }
        Ok((store, ids))
    }

    async fn create_summary_and_get_problems(
        ids: Vec<PersonId>,
        store: &PgStore,
        districts: Vec<ElectoralDistrict>,
        duplicate_districts: Vec<ElectoralDistrict>,
    ) -> Result<Problems, AppError> {
        let id = CandidateListId::new();
        let mut list = sample_candidate_list(id);
        list.candidates = ids;
        list.electoral_districts = districts.into_iter().collect();
        list.create(store).await?;
        Ok(CandidateListSummary {
            list,
            max_count: 20,
            duplicate_districts,
            candidates_with_problems: 0,
        }
        .get_problems(()))
    }

    #[test]
    fn candidates_with_problems_is_warning() {
        let mut list = sample_candidate_list(CandidateListId::new());
        list.candidates = vec![PersonId::new(), PersonId::new()];
        let problems = CandidateListSummary {
            list,
            max_count: 20,
            duplicate_districts: Vec::new(),
            candidates_with_problems: 2,
        }
        .get_problems(());

        assert_eq!(
            problems.potential_problems,
            vec![PotentialProblems::CandidatesWithProblems { count: 2 }]
        );
        assert_eq!(problems.highest_severity(), Some(Severity::Warn));
    }

    #[tokio::test]
    async fn no_problems() -> Result<(), AppError> {
        let (store, ids) = make_store_with_candidates(vec![sample_person(PersonId::new())]).await?;
        let problems = create_summary_and_get_problems(
            ids,
            &store,
            vec![ElectoralDistrict::PsAmsterdam],
            Vec::new(),
        )
        .await?;

        assert!(problems.info_problems.is_empty());
        assert!(problems.potential_problems.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn empty_list_problems() -> Result<(), AppError> {
        let (store, ids) = make_store_with_candidates(Vec::new()).await?;
        let problems = create_summary_and_get_problems(ids, &store, Vec::new(), Vec::new()).await?;

        assert_eq!(problems.potential_problems.len(), 2);
        assert!(
            problems
                .potential_problems
                .contains(&PotentialProblems::NoCandidates)
        );
        assert!(
            problems
                .potential_problems
                .contains(&PotentialProblems::NoDistricts)
        );

        assert!(problems.info_problems.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn list_problems_too_many() -> Result<(), AppError> {
        let persons: Vec<_> = (0..25).map(|_| sample_person(PersonId::new())).collect();
        let (store, ids) = make_store_with_candidates(persons).await?;
        let problems =
            create_summary_and_get_problems(ids, &store, vec![PsAmsterdam], vec![PsAmsterdam])
                .await?;

        // list with duplicate district
        let mut list = sample_candidate_list(CandidateListId::new());
        list.electoral_districts = BTreeSet::from([ElectoralDistrict::PsAmsterdam]);
        list.create(&store).await?;

        assert_eq!(problems.potential_problems.len(), 2);
        assert!(
            problems
                .potential_problems
                .contains(&PotentialProblems::TooManyCandidates { count: 5 })
        );
        assert!(
            problems
                .potential_problems
                .contains(&PotentialProblems::DuplicateDistricts)
        );

        assert!(problems.info_problems.is_empty());

        Ok(())
    }
}
