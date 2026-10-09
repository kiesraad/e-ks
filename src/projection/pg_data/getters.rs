//! Read accessors over a political group's projection.
//!
//! The pure getters on [`PgStoreData`] borrow from a snapshot; the getters on
//! [`Store<PgStoreData>`] are the older per-call wrappers that clone out of
//! one and are being phased out.

use crate::{
    AppError, ElectionConfig, ElectoralDistrict, OrNotFound, PgStoreData,
    store::{Store, StoreEvent},
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
    pub fn political_group(&self) -> &PoliticalGroup {
        &self.political_group
    }

    pub fn person(&self, person_id: PersonId) -> Option<&Person> {
        self.persons.get(&person_id)
    }

    /// Every person, in no particular order.
    pub fn persons(&self) -> impl Iterator<Item = &Person> {
        self.persons.values()
    }

    pub fn sorted_persons(&self) -> Vec<&Person> {
        let mut persons: Vec<&Person> = self.persons.values().collect();
        persons.sort();
        persons
    }

    pub fn person_count(&self) -> usize {
        self.persons.len()
    }

    pub fn candidate_list(&self, list_id: CandidateListId) -> Option<&CandidateList> {
        self.candidate_lists.get(&list_id)
    }

    /// Every candidate list, oldest first.
    pub fn candidate_lists(&self) -> Vec<&CandidateList> {
        let mut lists: Vec<&CandidateList> = self.candidate_lists.values().collect();
        lists.sort_by_key(|list| (list.created_at, list.id));
        lists
    }

    /// The candidate lists in the order the pages show them: by their lowest
    /// district number.
    pub fn candidate_lists_in_page_order(&self) -> Vec<&CandidateList> {
        let mut lists = self.candidate_lists();
        lists.sort_by_key(|list| {
            list.electoral_districts
                .iter()
                .map(ElectoralDistrict::region_number)
                .min()
                .unwrap_or_default()
        });
        lists
    }

    pub fn candidate_list_count(&self) -> usize {
        self.candidate_lists.len()
    }

    /// One-based position of the candidate on the given list.
    pub fn candidate_position(
        &self,
        list_id: CandidateListId,
        person_id: PersonId,
    ) -> Option<usize> {
        self.candidate_lists.get(&list_id)?.position_of(person_id)
    }

    /// How many lists the person stands on.
    pub fn count_candidate_lists(&self, person_id: PersonId) -> usize {
        self.candidate_lists
            .values()
            .filter(|list| list.candidates.contains(&person_id))
            .count()
    }

    /// The oldest list the person stands on.
    pub fn first_list(&self, person_id: PersonId) -> Option<&CandidateList> {
        self.candidate_lists()
            .into_iter()
            .find(|list| list.candidates.contains(&person_id))
    }

    /// The first candidate over the lists, oldest list first, skipping the
    /// lists and candidates `keep` rejects. `None` when there is none, or
    /// when the first one is unknown.
    pub fn first_candidate_where(
        &self,
        mut keep: impl FnMut(&CandidateList, PersonId) -> bool,
    ) -> Option<&Person> {
        self.candidate_lists()
            .into_iter()
            .flat_map(|list| list.candidates.iter().map(move |&person| (list, person)))
            .find(|(list, person)| keep(list, *person))
            .and_then(|(_, person)| self.person(person))
    }

    /// The name of the first candidate across all lists, oldest list first.
    pub fn first_candidate_name(&self) -> Option<&FullName> {
        self.first_candidate_where(|_, _| true)
            .map(|person| &person.name)
    }

    pub fn name_authorisation(&self, id: NameAuthorisationId) -> Option<&NameAuthorisation> {
        self.name_authorisations.get(&id)
    }

    /// Every name authorisation, in no particular order.
    pub fn name_authorisations(&self) -> Vec<&NameAuthorisation> {
        self.name_authorisations.values().collect()
    }

    pub fn list_submitter(&self) -> &ListSubmitter {
        &self.list_submitter
    }

    /// In the order they were added.
    pub fn substitute_submitters(&self) -> &[ListSubmitter] {
        &self.substitute_submitters
    }

    pub fn substitute_submitter(&self, id: ListSubmitterId) -> Option<&ListSubmitter> {
        self.substitute_submitters
            .iter()
            .find(|submitter| submitter.id == id)
    }

    /// We show a warning after the user has downloaded the documents, until
    /// they close it (a `HideDownloadWarning` event).
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

impl Store<PgStoreData> {
    pub fn get_election(&self) -> ElectionConfig {
        self.election
    }

    pub fn get_political_group(&self) -> PoliticalGroup {
        self.snapshot().political_group().clone()
    }

    pub fn get_persons(&self) -> Vec<Person> {
        self.snapshot().persons().cloned().collect()
    }

    pub fn get_sorted_persons(&self) -> Vec<Person> {
        self.snapshot()
            .sorted_persons()
            .into_iter()
            .cloned()
            .collect()
    }

    pub fn get_name_authorisations(&self) -> Vec<NameAuthorisation> {
        self.snapshot()
            .name_authorisations()
            .into_iter()
            .cloned()
            .collect()
    }

    pub fn get_substitute_submitters(&self) -> Vec<ListSubmitter> {
        self.snapshot().substitute_submitters().to_vec()
    }

    pub fn get_person_count(&self) -> usize {
        self.snapshot().person_count()
    }

    pub fn get_candidate_list_count(&self) -> usize {
        self.snapshot().candidate_list_count()
    }

    pub fn get_candidate_list(&self, list_id: CandidateListId) -> Result<CandidateList, AppError> {
        self.snapshot()
            .candidate_list(list_id)
            .cloned()
            .or_not_found()
    }

    pub fn get_candidate_lists(&self) -> Vec<CandidateList> {
        self.snapshot()
            .candidate_lists()
            .into_iter()
            .cloned()
            .collect()
    }

    pub fn get_person(&self, person_id: PersonId) -> Result<Person, AppError> {
        self.snapshot().person(person_id).cloned().or_not_found()
    }

    pub fn get_candidate_position(
        &self,
        list_id: CandidateListId,
        person_id: PersonId,
    ) -> Option<usize> {
        self.snapshot().candidate_position(list_id, person_id)
    }

    pub fn get_first_candidate_name(&self) -> Option<FullName> {
        self.snapshot().first_candidate_name().cloned()
    }

    pub fn get_name_authorisation(
        &self,
        authorisation_id: NameAuthorisationId,
    ) -> Result<NameAuthorisation, AppError> {
        self.snapshot()
            .name_authorisation(authorisation_id)
            .cloned()
            .or_not_found()
    }

    pub fn get_list_submitter(&self) -> ListSubmitter {
        self.snapshot().list_submitter().clone()
    }

    pub fn get_substitute_submitter(
        &self,
        substitute_submitter_id: ListSubmitterId,
    ) -> Result<ListSubmitter, AppError> {
        self.snapshot()
            .substitute_submitter(substitute_submitter_id)
            .cloned()
            .or_not_found()
    }

    pub fn count_candidate_lists(&self, person_id: PersonId) -> usize {
        self.snapshot().count_candidate_lists(person_id)
    }

    pub fn get_events(&self) -> Vec<StoreEvent<crate::PgEvent>> {
        self.snapshot().events.clone()
    }

    pub fn should_show_download_warning(&self) -> bool {
        self.snapshot().should_show_download_warning()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        PgEvent, store::StoreData, structs::list_submitters::ListSubmitterId,
        test_utils::sample_list_submitter,
    };

    /// A projection with `events` applied in order.
    fn replay(events: impl IntoIterator<Item = PgEvent>) -> PgStoreData {
        let mut data = PgStoreData::default();
        for (index, event) in events.into_iter().enumerate() {
            data.apply(StoreEvent::new(index + 1, event));
        }
        data
    }

    #[test]
    fn substitute_submitters_remain_in_order() {
        let data = replay((0..100).map(|i| {
            let mut submitter = sample_list_submitter(ListSubmitterId::new());
            submitter.name.last_name = i.to_string().parse().unwrap();
            PgEvent::CreateSubstituteSubmitter(submitter)
        }));

        for (i, s) in data.substitute_submitters().iter().enumerate() {
            assert_eq!(s.name.last_name.to_string(), i.to_string());
        }
    }

    /// The flag is set from the field a submitter lands in, whatever the
    /// event carried.
    #[test]
    fn apply_sets_the_substitute_flag() {
        let mut main_submitter = sample_list_submitter(ListSubmitterId::new());
        main_submitter.is_substitute = true;
        let mut substitute_submitter = sample_list_submitter(ListSubmitterId::new());
        substitute_submitter.is_substitute = false;
        let data = replay([
            PgEvent::UpdateListSubmitter(main_submitter),
            PgEvent::CreateSubstituteSubmitter(substitute_submitter.clone()),
            PgEvent::UpdateSubstituteSubmitter(substitute_submitter.clone()),
        ]);

        assert!(!data.list_submitter().is_substitute);
        assert!(data.substitute_submitters()[0].is_substitute);
        assert!(
            data.substitute_submitter(substitute_submitter.id)
                .unwrap()
                .is_substitute
        );
    }

    #[test]
    fn should_show_download_warning_tracks_download_and_hide_events() {
        let download = || PgEvent::DownloadFile {
            file_name: "documents.zip".to_string(),
            download_path: "/download".to_string(),
        };

        // start without warning
        assert!(!replay([]).should_show_download_warning());
        // after download, the warning should show
        assert!(replay([download()]).should_show_download_warning());
        // after hiding, the warning should no longer show
        assert!(!replay([download(), PgEvent::HideDownloadWarning]).should_show_download_warning());
        // after downloading again, the warning should show again
        assert!(
            replay([download(), PgEvent::HideDownloadWarning, download()])
                .should_show_download_warning()
        );
    }

    #[test]
    fn candidate_lists_are_oldest_first_and_pages_order_by_district() {
        use crate::{ElectoralDistrict, structs::common::UtcDateTime};
        use std::collections::BTreeSet;

        let mut data = PgStoreData::default();
        let (early, late) = (CandidateListId::new(), CandidateListId::new());
        let now = chrono::Utc::now();
        data.candidate_lists.insert(
            late,
            CandidateList {
                id: late,
                electoral_districts: BTreeSet::from([ElectoralDistrict::Groningen]),
                created_at: UtcDateTime::from(now),
                ..Default::default()
            },
        );
        data.candidate_lists.insert(
            early,
            CandidateList {
                id: early,
                electoral_districts: BTreeSet::from([ElectoralDistrict::Utrecht]),
                created_at: UtcDateTime::from(now - chrono::TimeDelta::days(1)),
                ..Default::default()
            },
        );

        let ids = |lists: Vec<&CandidateList>| lists.into_iter().map(|l| l.id).collect::<Vec<_>>();
        assert_eq!(ids(data.candidate_lists()), vec![early, late]);
        assert_eq!(ids(data.candidate_lists_in_page_order()), vec![late, early]);
    }
}
