use crate::{
    ElectionConfig, ElectoralDistrict, id_newtype,
    structs::{common::UtcDateTime, persons::PersonId},
};
use serde::{Deserialize, Serialize};

id_newtype!(pub struct CandidateListId);

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CandidateList {
    pub id: CandidateListId,
    pub electoral_districts: Vec<ElectoralDistrict>,
    pub candidates: Vec<PersonId>,
    pub created_at: UtcDateTime,
}

impl CandidateList {
    /// One-based position of the candidate on this list.
    pub fn position_of(&self, person_id: PersonId) -> Option<usize> {
        self.candidates
            .iter()
            .position(|candidate| *candidate == person_id)
            .map(|index| index + 1)
    }

    pub fn districts_name(&self) -> String {
        self.electoral_districts
            .iter()
            .map(ElectoralDistrict::title)
            .collect::<Vec<&str>>()
            .join(", ")
    }

    pub fn districts_codes(&self) -> String {
        self.electoral_districts
            .iter()
            .map(|d| d.code().to_lowercase())
            .collect::<Vec<_>>()
            .join("-")
    }

    /// Every election district is on the list; repeats do not count.
    pub fn contains_all_districts(&self, election: &ElectionConfig) -> bool {
        election
            .electoral_districts()
            .iter()
            .all(|district| self.electoral_districts.contains(district))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppError, MAX_CANDIDATES, PgStore,
        structs::{
            candidate_lists::{CandidateListSummary, FullCandidateList},
            persons::PersonId,
        },
        test_utils::{sample_candidate_list, sample_person, sample_person_with_last_name},
    };
    use std::collections::BTreeSet;
    fn base_candidate_list(electoral_districts: Vec<ElectoralDistrict>) -> CandidateList {
        CandidateList {
            electoral_districts,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn districts_formats_titles_in_order() {
        let list = base_candidate_list(vec![
            ElectoralDistrict::Utrecht,
            ElectoralDistrict::NoordHolland,
            ElectoralDistrict::Drenthe,
        ]);

        assert_eq!(list.districts_name(), "Utrecht, Noord-Holland, Drenthe");
    }

    #[tokio::test]
    async fn contains_all_districts_requires_every_election_district() {
        let election = ElectionConfig::EK27;
        let list = base_candidate_list(election.electoral_districts().to_vec());
        assert!(list.contains_all_districts(&election));

        let list = base_candidate_list(vec![
            ElectoralDistrict::Utrecht,
            ElectoralDistrict::NoordHolland,
        ]);
        assert!(!list.contains_all_districts(&election));

        // same length, one district repeated
        let mut districts = election.electoral_districts().to_vec();
        districts.pop();
        districts.push(ElectoralDistrict::Utrecht);
        assert_eq!(districts.len(), election.electoral_districts().len());
        assert!(!base_candidate_list(districts).contains_all_districts(&election));
    }

    async fn insert_list(
        store: &PgStore,
        electoral_districts: Vec<ElectoralDistrict>,
    ) -> Result<CandidateList, AppError> {
        let list = CandidateList {
            electoral_districts,
            ..Default::default()
        };

        list.create(store).await?;

        Ok(list)
    }

    #[tokio::test]
    async fn duplicate_districts_returns_empty_for_single_list() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list = insert_list(
            &store,
            vec![ElectoralDistrict::Utrecht, ElectoralDistrict::Drenthe],
        )
        .await?;

        assert_eq!(list.duplicate_districts(&store), vec![]);

        Ok(())
    }

    #[tokio::test]
    async fn duplicate_districts_returns_shared_districts() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list = insert_list(
            &store,
            vec![
                ElectoralDistrict::Utrecht,
                ElectoralDistrict::Drenthe,
                ElectoralDistrict::NoordHolland,
            ],
        )
        .await?;
        insert_list(
            &store,
            vec![ElectoralDistrict::Utrecht, ElectoralDistrict::Groningen],
        )
        .await?;
        insert_list(
            &store,
            vec![ElectoralDistrict::Limburg, ElectoralDistrict::Groningen],
        )
        .await?;
        insert_list(
            &store,
            vec![ElectoralDistrict::Gelderland, ElectoralDistrict::Drenthe],
        )
        .await?;

        assert_eq!(
            list.duplicate_districts(&store),
            vec![ElectoralDistrict::Utrecht, ElectoralDistrict::Drenthe] // but not Groningen!
        );

        Ok(())
    }

    #[tokio::test]
    async fn duplicate_districts_excludes_non_overlapping_districts() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list = insert_list(
            &store,
            vec![ElectoralDistrict::Utrecht, ElectoralDistrict::Drenthe],
        )
        .await?;
        insert_list(
            &store,
            vec![ElectoralDistrict::Groningen, ElectoralDistrict::Overijssel],
        )
        .await?;
        insert_list(
            &store,
            vec![ElectoralDistrict::Limburg, ElectoralDistrict::Gelderland],
        )
        .await?;

        assert_eq!(list.duplicate_districts(&store), vec![]);

        Ok(())
    }

    #[tokio::test]
    async fn create_and_list_candidate_lists() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list = sample_candidate_list(CandidateListId::new());

        list.create(&store).await?;

        let lists = CandidateListSummary::list(&store);
        assert_eq!(1, lists.len());
        assert_eq!(list.id, lists[0].list.id);
        assert_eq!(0, lists[0].candidate_count());
        assert_eq!(0, lists[0].duplicate_districts.len());

        Ok(())
    }

    #[tokio::test]
    async fn get_candidate_list_summaries_with_duplicate_districts() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        // setup
        let list1 = insert_list(
            &store,
            vec![ElectoralDistrict::Utrecht, ElectoralDistrict::Drenthe],
        )
        .await?;
        let list2 = insert_list(
            &store,
            vec![ElectoralDistrict::Utrecht, ElectoralDistrict::Groningen],
        )
        .await?;

        let list3 = insert_list(
            &store,
            vec![ElectoralDistrict::Overijssel, ElectoralDistrict::Groningen],
        )
        .await?;

        // test
        let lists = CandidateListSummary::list(&store);

        // verification
        assert_eq!(3, lists.len());

        let list_summary1 = lists.iter().find(|list| list.list.id == list1.id).unwrap();
        let list_summary2 = lists.iter().find(|list| list.list.id == list2.id).unwrap();
        let list_summary3 = lists.iter().find(|list| list.list.id == list3.id).unwrap();

        // list 1 clashes on Utrecht with list 2
        assert_eq!(
            vec![ElectoralDistrict::Utrecht],
            list_summary1.duplicate_districts
        );

        // list 2 clashes on Utrecht with list 1 and on Groningen with list 3
        assert_eq!(2, list_summary2.duplicate_districts.len());
        assert!(
            list_summary2
                .duplicate_districts
                .contains(&ElectoralDistrict::Utrecht)
        );
        assert!(
            list_summary2
                .duplicate_districts
                .contains(&ElectoralDistrict::Groningen)
        );

        // list 3 clashes on Groningen with list 2
        assert_eq!(
            vec![ElectoralDistrict::Groningen],
            list_summary3.duplicate_districts
        );

        Ok(())
    }

    #[tokio::test]
    async fn list_candidate_list_orders_by_created_at() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_early = CandidateList {
            electoral_districts: vec![ElectoralDistrict::Utrecht],
            ..Default::default()
        };
        list_early.create(&store).await?;

        // sleep for a second to ensure a different created_at timestamp for the next list
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        let list_late = CandidateList {
            electoral_districts: vec![ElectoralDistrict::Overijssel],
            ..Default::default()
        };
        list_late.create(&store).await?;

        let lists = store.get_candidate_lists();
        assert_eq!(lists.len(), 2);
        assert_eq!(lists[0].id, list_early.id);
        assert_eq!(lists[1].id, list_late.id);

        Ok(())
    }

    #[tokio::test]
    async fn get_candidate_list_returns_list() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list = sample_candidate_list(CandidateListId::new());

        list.create(&store).await?;

        let loaded = store.get_candidate_list(list.id)?;

        assert_eq!(loaded.id, list.id);

        Ok(())
    }

    #[tokio::test]
    async fn update_candidate_list_updates_districts() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list = sample_candidate_list(CandidateListId::new());

        list.create(&store).await?;

        let updated_list = CandidateList {
            electoral_districts: vec![ElectoralDistrict::Drenthe, ElectoralDistrict::Overijssel],
            ..list.clone()
        };

        updated_list.update_districts(&store).await?;

        assert_eq!(updated_list.id, list.id);
        assert_eq!(
            updated_list.electoral_districts,
            vec![ElectoralDistrict::Drenthe, ElectoralDistrict::Overijssel]
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_get_used_districts() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        // setup
        let expected = BTreeSet::from([
            ElectoralDistrict::Utrecht,
            ElectoralDistrict::Drenthe,
            ElectoralDistrict::Overijssel,
        ]);

        insert_list(
            &store,
            vec![ElectoralDistrict::Utrecht, ElectoralDistrict::Drenthe],
        )
        .await?;
        insert_list(&store, vec![ElectoralDistrict::Overijssel]).await?;
        insert_list(&store, vec![]).await?;

        // test
        let result: BTreeSet<ElectoralDistrict> =
            CandidateList::used_districts(&store)?.into_iter().collect();

        // verify
        assert_eq!(expected, result);
        Ok(())
    }

    #[tokio::test]
    async fn get_used_districts_no_lists() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let result = CandidateList::used_districts(&store)?;

        assert_eq!(Vec::<ElectoralDistrict>::new(), result);

        Ok(())
    }

    #[tokio::test]
    async fn get_used_districts_double_districts() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let expected = BTreeSet::from([
            ElectoralDistrict::Utrecht,
            ElectoralDistrict::Drenthe,
            ElectoralDistrict::Overijssel,
        ]);

        // setup
        insert_list(
            &store,
            vec![ElectoralDistrict::Utrecht, ElectoralDistrict::Drenthe],
        )
        .await?;
        insert_list(
            &store,
            vec![ElectoralDistrict::Utrecht, ElectoralDistrict::Overijssel],
        )
        .await?;

        // test
        let result: BTreeSet<ElectoralDistrict> =
            CandidateList::used_districts(&store)?.into_iter().collect();

        // verify
        assert_eq!(expected, result);
        Ok(())
    }

    #[tokio::test]
    async fn get_used_district_with_exclude() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let expected = BTreeSet::from([
            ElectoralDistrict::Utrecht,
            ElectoralDistrict::Drenthe,
            ElectoralDistrict::Groningen,
            ElectoralDistrict::Overijssel,
        ]);

        // setup
        insert_list(
            &store,
            vec![ElectoralDistrict::Utrecht, ElectoralDistrict::Drenthe],
        )
        .await?;
        insert_list(
            &store,
            vec![ElectoralDistrict::Groningen, ElectoralDistrict::Overijssel],
        )
        .await?;

        // test
        let result: BTreeSet<ElectoralDistrict> =
            CandidateList::used_districts(&store)?.into_iter().collect();

        // verify
        assert_eq!(expected, result);
        Ok(())
    }

    #[tokio::test]
    async fn test_remove_candidate_list() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        // setup
        let list_a = sample_candidate_list(CandidateListId::new());
        let person_a = sample_person_with_last_name(PersonId::new(), "Jansen");
        let list_b = sample_candidate_list(CandidateListId::new());
        let person_b = sample_person_with_last_name(PersonId::new(), "Bakker");

        list_a.create(&store).await?;
        person_a.create(&store).await?;
        list_a.clone().update_order(&store, &[person_a.id]).await?;

        list_b.create(&store).await?;
        person_b.create(&store).await?;
        list_b.clone().update_order(&store, &[person_b.id]).await?;

        list_a.delete(&store).await?;

        let lists = CandidateListSummary::list(&store);
        let list_b_from_db = FullCandidateList::get(&store, list_b.id).unwrap();

        assert_eq!(1, lists.len());
        assert_eq!(list_b.id, lists[0].list.id);
        assert_eq!(1, lists[0].candidate_count());
        assert_eq!(person_b.id, list_b_from_db.candidates[0].data.person.id);
        assert_eq!(0, lists[0].duplicate_districts.len());

        Ok(())
    }

    #[tokio::test]
    async fn get_candidate_list_includes_candidates() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        let person_a = sample_person_with_last_name(PersonId::new(), "Jansen");
        let person_b = sample_person_with_last_name(PersonId::new(), "Bakker");

        list.create(&store).await?;
        person_a.create(&store).await?;
        person_b.create(&store).await?;
        list.clone()
            .update_order(&store, &[person_a.id, person_b.id])
            .await?;

        let detail = FullCandidateList::get(&store, list_id).expect("candidate list");
        assert_eq!(2, detail.candidates.len());
        assert_eq!(person_a.id, detail.candidates[0].data.person.id);
        assert_eq!(person_b.id, detail.candidates[1].data.person.id);

        Ok(())
    }

    #[tokio::test]
    async fn update_candidate_list_order_returns_not_found() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let mut missing_list = sample_candidate_list(CandidateListId::new());
        let err = missing_list.update_order(&store, &[]).await.unwrap_err();
        assert!(matches!(err, AppError::GenericNotFound));

        Ok(())
    }

    #[tokio::test]
    async fn get_full_candidate_list_returns_none_for_missing_list() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let missing = FullCandidateList::get(&store, CandidateListId::new());
        assert!(missing.is_err());

        Ok(())
    }

    #[tokio::test]
    async fn test_append_candidate_to_list() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        let person_a = sample_person_with_last_name(PersonId::new(), "Jansen");
        let person_b = sample_person_with_last_name(PersonId::new(), "Bakker");

        list.create(&store).await?;
        person_a.create(&store).await?;
        person_b.create(&store).await?;

        list.append_candidate(&store, person_a.id).await?;
        list.append_candidate(&store, person_b.id).await?;

        let detail = FullCandidateList::get(&store, list_id).expect("candidate list");

        assert_eq!(detail.candidates.len(), 2);
        assert_eq!(detail.candidates[0].data.person.id, person_a.id);
        assert_eq!(detail.candidates[0].data.position, 1);
        assert_eq!(detail.candidates[1].data.person.id, person_b.id);
        assert_eq!(detail.candidates[1].data.position, 2);

        Ok(())
    }

    #[tokio::test]
    async fn append_candidate_to_list_returns_not_found() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let mut missing_list = sample_candidate_list(CandidateListId::new());
        let err = missing_list
            .append_candidate(&store, PersonId::new())
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::GenericNotFound));

        Ok(())
    }

    #[tokio::test]
    async fn append_candidate_rejects_beyond_max() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.create(&store).await?;

        for _ in 0..MAX_CANDIDATES {
            let person = sample_person(PersonId::new());
            person.create(&store).await?;
            list.append_candidate(&store, person.id).await?;
        }
        assert_eq!(list.candidates.len(), MAX_CANDIDATES);

        let overflow = sample_person(PersonId::new());
        overflow.create(&store).await?;
        let err = list
            .append_candidate(&store, overflow.id)
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::TooManyCandidates { .. }));
        assert_eq!(
            store.get_candidate_list(list_id)?.candidates.len(),
            MAX_CANDIDATES
        );

        Ok(())
    }

    #[tokio::test]
    async fn update_order_rejects_beyond_max() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.create(&store).await?;

        let mut person_ids = Vec::new();
        for _ in 0..=MAX_CANDIDATES {
            let person = sample_person(PersonId::new());
            person.create(&store).await?;
            person_ids.push(person.id);
        }
        assert_eq!(person_ids.len(), MAX_CANDIDATES + 1);

        let err = list.update_order(&store, &person_ids).await.unwrap_err();

        assert!(matches!(err, AppError::TooManyCandidates { .. }));
        assert!(store.get_candidate_list(list_id)?.candidates.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn update_order_rejects_duplicates() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        let person = sample_person(PersonId::new());
        list.create(&store).await?;
        person.create(&store).await?;

        let err = list
            .update_order(&store, &[person.id, person.id])
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::DuplicateCandidate));
        assert!(store.get_candidate_list(list_id)?.candidates.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn remove_candidate_removes_from_list() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        let person_a = sample_person_with_last_name(PersonId::new(), "Jansen");
        let person_b = sample_person_with_last_name(PersonId::new(), "Bakker");

        list.create(&store).await?;
        person_a.create(&store).await?;
        person_b.create(&store).await?;
        let mut list = store.get_candidate_list(list_id)?;
        list.append_candidate(&store, person_a.id).await?;
        list.append_candidate(&store, person_b.id).await?;

        person_a.delete(&store).await?;

        let detail = FullCandidateList::get(&store, list_id).expect("candidate list");
        assert_eq!(detail.candidates.len(), 1);
        assert_eq!(detail.candidates[0].data.person.id, person_b.id);

        Ok(())
    }

    #[tokio::test]
    async fn get_candidate_returns_candidate() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        let person = sample_person_with_last_name(PersonId::new(), "Jansen");

        list.create(&store).await?;
        person.create(&store).await?;
        list.append_candidate(&store, person.id).await?;

        let candidate = store
            .get_candidate_list(list_id)?
            .get_candidate(&store, person.id)
            .await?;

        assert_eq!(candidate.list_id, list_id);
        assert_eq!(candidate.position, 1);
        assert_eq!(candidate.person.id, person.id);

        Ok(())
    }
}
