use crate::{
    AppError, ElectionConfig, PgStoreData,
    structs::{
        candidate_lists::{CandidateList, CandidateListId},
        list_submitters::{ListSubmitter, ListSubmitterId},
        name_authorisations::{NameAuthorisation, NameAuthorisationId},
        persons::{Person, PersonId},
        political_groups::PoliticalGroup,
    },
};

use crate::store::{Store, StoreEvent};

impl Store<PgStoreData> {
    pub fn get_election(&self) -> ElectionConfig {
        self.election
    }

    pub fn get_political_group(&self) -> PoliticalGroup {
        let data = self.snapshot();

        data.political_group.clone()
    }

    pub fn get_persons(&self) -> Vec<Person> {
        let data = self.snapshot();

        data.persons.values().cloned().collect()
    }

    pub fn get_sorted_persons(&self) -> Vec<Person> {
        let data = self.snapshot();

        let mut persons: Vec<Person> = data.persons.values().cloned().collect();

        persons.sort();

        persons
    }

    pub fn get_name_authorisations(&self) -> Vec<NameAuthorisation> {
        let data = self.snapshot();

        data.name_authorisations.values().cloned().collect()
    }

    pub fn get_substitute_submitters(&self) -> Vec<ListSubmitter> {
        let data = self.snapshot();

        data.substitute_submitters.clone()
    }

    pub fn get_person_count(&self) -> usize {
        let data = self.snapshot();

        data.persons.len()
    }

    pub fn get_candidate_list_count(&self) -> usize {
        let data = self.snapshot();

        data.candidate_lists.len()
    }

    pub fn get_candidate_list(&self, list_id: CandidateListId) -> Result<CandidateList, AppError> {
        let data = self.snapshot();

        data.candidate_lists
            .get(&list_id)
            .cloned()
            .ok_or(AppError::GenericNotFound)
    }

    pub fn get_candidate_lists(&self) -> Vec<CandidateList> {
        let data = self.snapshot();

        let mut lists: Vec<CandidateList> = data.candidate_lists.values().cloned().collect();

        lists.sort_unstable_by_key(|l| l.created_at);

        lists
    }

    pub fn get_person(&self, person_id: PersonId) -> Result<Person, AppError> {
        let data = self.snapshot();

        data.persons
            .get(&person_id)
            .cloned()
            .ok_or(AppError::GenericNotFound)
    }

    /// One-based position of the candidate on the given list.
    pub fn get_candidate_position(
        &self,
        list_id: CandidateListId,
        person_id: PersonId,
    ) -> Option<usize> {
        let data = self.snapshot();

        data.candidate_lists.get(&list_id)?.position_of(person_id)
    }

    /// The name of the first candidate across all candidate lists (already
    /// sorted by creation date), or `None` when there are no candidates.
    pub fn get_first_candidate_name(&self) -> Option<crate::structs::common::FullName> {
        self.get_candidate_lists()
            .into_iter()
            .flat_map(|list| list.candidates.into_iter())
            .next()
            .and_then(|id| self.get_person(id).ok())
            .map(|p| p.name)
    }

    pub fn get_name_authorisation(
        &self,
        authorisation_id: NameAuthorisationId,
    ) -> Result<NameAuthorisation, AppError> {
        let data = self.snapshot();

        data.name_authorisations
            .get(&authorisation_id)
            .cloned()
            .ok_or(AppError::GenericNotFound)
    }

    pub fn get_list_submitter(&self) -> ListSubmitter {
        let data = self.snapshot();

        data.list_submitter.clone()
    }

    pub fn get_substitute_submitter(
        &self,
        substitute_submitter_id: ListSubmitterId,
    ) -> Result<ListSubmitter, AppError> {
        let data = self.snapshot();

        data.substitute_submitters
            .iter()
            .find(|submitter| submitter.id == substitute_submitter_id)
            .cloned()
            .ok_or(AppError::GenericNotFound)
    }

    pub fn count_candidate_lists(&self, person_id: PersonId) -> usize {
        let data = self.snapshot();

        data.candidate_lists
            .values()
            .filter(|list| list.candidates.contains(&person_id))
            .count()
    }

    pub fn get_events(&self) -> Vec<StoreEvent<crate::PgEvent>> {
        let data = self.snapshot();
        data.events.clone()
    }

    /// We show a warning after the user has downloaded the documents.
    ///
    /// If the user closes this warning, an `HideDownloadWarning` event is stored and we should no longer show the warning.
    pub fn should_show_download_warning(&self) -> bool {
        let data = self.snapshot();
        data.events
            .iter()
            .rev()
            .find(|e| {
                matches!(
                    e.payload,
                    crate::PgEvent::DownloadFile { .. } | crate::PgEvent::HideDownloadWarning
                )
            })
            .is_some_and(|e| matches!(e.payload, crate::PgEvent::DownloadFile { .. }))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        PgEvent, PgStore, structs::list_submitters::ListSubmitterId,
        test_utils::sample_list_submitter,
    };

    #[tokio::test]
    async fn substitute_submitters_remain_in_order() {
        let store = PgStore::new_for_test();

        for i in 0..100 {
            let mut sub_submitter = sample_list_submitter(ListSubmitterId::new());
            sub_submitter.name.last_name = i.to_string().parse().unwrap();
            sub_submitter.create_substitute(&store).await.unwrap();
        }

        for (i, s) in store.get_substitute_submitters().iter().enumerate() {
            assert_eq!(s.name.last_name.to_string(), i.to_string());
        }
    }

    #[tokio::test]
    async fn getters_set_substitute_flag() {
        let store = PgStore::new_for_test();

        let main_submitter = sample_list_submitter(ListSubmitterId::new());
        let substitute_submitter = sample_list_submitter(ListSubmitterId::new());

        main_submitter.update(&store).await.unwrap();
        substitute_submitter
            .create_substitute(&store)
            .await
            .unwrap();

        assert!(!store.get_list_submitter().is_substitute);
        assert!(store.get_substitute_submitters()[0].is_substitute);
        assert!(
            store
                .get_substitute_submitter(substitute_submitter.id)
                .unwrap()
                .is_substitute
        );
    }

    #[tokio::test]
    async fn should_show_download_warning_tracks_download_and_hide_events() {
        let store = PgStore::new_for_test();

        // start without warning
        assert!(!store.should_show_download_warning());

        // after download, the warning should show
        store
            .update(PgEvent::DownloadFile {
                file_name: "documents.zip".to_string(),
                download_path: "/download".to_string(),
            })
            .await
            .unwrap();
        assert!(store.should_show_download_warning());

        // after hiding, the warning should no longer show
        store.update(PgEvent::HideDownloadWarning).await.unwrap();
        assert!(!store.should_show_download_warning());

        // after downloading again, the warning should show again
        store
            .update(PgEvent::DownloadFile {
                file_name: "documents.zip".to_string(),
                download_path: "/download".to_string(),
            })
            .await
            .unwrap();
        assert!(store.should_show_download_warning());
    }
}
