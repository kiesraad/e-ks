//! Reads on a political group's projection. The methods on [`PgStoreData`]
//! work on any value of it: a live snapshot, a CSB view at a correction level,
//! an audit-log replay or a test fixture. The ones on the store handle take a
//! snapshot and clone, for callers that read once.

use crate::{
    AppError, ElectoralDistrict, PgStoreData,
    structs::{
        candidate_lists::{CandidateList, CandidateListId},
        common::FullName,
        list_submitters::{ListSubmitter, ListSubmitterId},
        name_authorisations::{NameAuthorisation, NameAuthorisationId},
        persons::{Person, PersonId},
        political_groups::PoliticalGroup,
    },
};

impl PgStoreData {
    pub fn get_political_group(&self) -> &PoliticalGroup {
        &self.political_group
    }

    pub fn get_persons(&self) -> Vec<&Person> {
        self.persons.values().collect()
    }

    pub fn get_sorted_persons(&self) -> Vec<&Person> {
        let mut persons = self.get_persons();
        persons.sort();
        persons
    }

    pub fn get_person(&self, person_id: PersonId) -> Result<&Person, AppError> {
        self.persons
            .get(&person_id)
            .ok_or(AppError::GenericNotFound)
    }

    pub fn get_person_count(&self) -> usize {
        self.persons.len()
    }

    pub fn get_name_authorisations(&self) -> Vec<&NameAuthorisation> {
        self.name_authorisations.values().collect()
    }

    pub fn get_name_authorisation(
        &self,
        authorisation_id: NameAuthorisationId,
    ) -> Result<&NameAuthorisation, AppError> {
        self.name_authorisations
            .get(&authorisation_id)
            .ok_or(AppError::GenericNotFound)
    }

    pub fn get_list_submitter(&self) -> &ListSubmitter {
        &self.list_submitter
    }

    pub fn get_substitute_submitters(&self) -> &[ListSubmitter] {
        &self.substitute_submitters
    }

    pub fn get_substitute_submitter(
        &self,
        substitute_submitter_id: ListSubmitterId,
    ) -> Result<&ListSubmitter, AppError> {
        self.substitute_submitters
            .iter()
            .find(|submitter| submitter.id == substitute_submitter_id)
            .ok_or(AppError::GenericNotFound)
    }

    pub fn get_candidate_list(&self, list_id: CandidateListId) -> Result<&CandidateList, AppError> {
        self.candidate_lists
            .get(&list_id)
            .ok_or(AppError::GenericNotFound)
    }

    /// The candidate lists in creation order.
    pub fn get_candidate_lists(&self) -> Vec<&CandidateList> {
        let mut lists: Vec<&CandidateList> = self.candidate_lists.values().collect();
        lists.sort_unstable_by_key(|list| list.created_at);
        lists
    }

    pub fn get_candidate_list_count(&self) -> usize {
        self.candidate_lists.len()
    }

    /// The candidate lists in the order the pages show them: by their lowest
    /// district number.
    pub fn get_candidate_lists_in_page_order(&self) -> Vec<&CandidateList> {
        let mut lists = self.get_candidate_lists();
        lists.sort_by_key(|list| {
            list.electoral_districts
                .iter()
                .map(ElectoralDistrict::region_number)
                .min()
                .unwrap_or_default()
        });
        lists
    }

    /// The first list (in creation order) the person appears on.
    pub fn get_first_list(&self, person_id: PersonId) -> Option<&CandidateList> {
        self.get_candidate_lists()
            .into_iter()
            .find(|list| list.candidates.contains(&person_id))
    }

    /// One-based position of the candidate on the given list.
    pub fn get_candidate_position(
        &self,
        list_id: CandidateListId,
        person_id: PersonId,
    ) -> Option<usize> {
        self.candidate_lists.get(&list_id)?.position_of(person_id)
    }

    /// How many lists the person is a candidate on.
    pub fn count_candidate_lists(&self, person_id: PersonId) -> usize {
        self.candidate_lists
            .values()
            .filter(|list| list.candidates.contains(&person_id))
            .count()
    }

    /// Every candidate with the list it is on, lists in creation order and
    /// candidates in list order.
    pub fn candidates_in_list_order(
        &self,
    ) -> impl Iterator<Item = (CandidateListId, PersonId)> + '_ {
        self.get_candidate_lists()
            .into_iter()
            .flat_map(|list| list.candidates.iter().map(move |&person| (list.id, person)))
    }

    /// The name of the first candidate on the first list, or `None` when
    /// there are no candidates.
    pub fn get_first_candidate_name(&self) -> Option<&FullName> {
        self.candidates_in_list_order()
            .find_map(|(_, person_id)| self.get_person(person_id).ok())
            .map(|person| &person.name)
    }

    /// The appellation of the political group, including the special names
    /// for blank lists.
    pub fn get_appellation(&self) -> String {
        self.political_group
            .csb_appellation(self.get_first_candidate_name())
    }

    /// We show a warning after the user has downloaded the documents.
    ///
    /// If the user closes this warning, an `HideDownloadWarning` event is
    /// stored and we should no longer show the warning.
    pub fn should_show_download_warning(&self) -> bool {
        self.events
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

        for (i, s) in store
            .snapshot()
            .get_substitute_submitters()
            .iter()
            .enumerate()
        {
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

        assert!(!store.snapshot().get_list_submitter().clone().is_substitute);
        assert!(store.snapshot().get_substitute_submitters()[0].is_substitute);
        assert!(
            store
                .snapshot()
                .get_substitute_submitter(substitute_submitter.id)
                .cloned()
                .unwrap()
                .is_substitute
        );
    }

    #[tokio::test]
    async fn should_show_download_warning_tracks_download_and_hide_events() {
        let store = PgStore::new_for_test();

        // start without warning
        assert!(!store.snapshot().should_show_download_warning());

        // after download, the warning should show
        store
            .update(PgEvent::DownloadFile {
                file_name: "documents.zip".to_string(),
                download_path: "/download".to_string(),
            })
            .await
            .unwrap();
        assert!(store.snapshot().should_show_download_warning());

        // after hiding, the warning should no longer show
        store.update(PgEvent::HideDownloadWarning).await.unwrap();
        assert!(!store.snapshot().should_show_download_warning());

        // after downloading again, the warning should show again
        store
            .update(PgEvent::DownloadFile {
                file_name: "documents.zip".to_string(),
                download_path: "/download".to_string(),
            })
            .await
            .unwrap();
        assert!(store.snapshot().should_show_download_warning());
    }
}
