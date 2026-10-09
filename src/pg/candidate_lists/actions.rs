//! Store-backed operations for candidate lists.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    AppError, ElectionConfig, ElectoralDistrict, OrNotFound, PgEvent, PgStore, PgStoreData,
    structs::{
        candidate_lists::{
            CandidateList, CandidateListId, CandidateListSummary, FullCandidateList,
        },
        candidates::{Candidate, CandidateWithProblems},
        common::{HasSeverity, Problematic, Severity},
        persons::{Person, PersonId},
    },
};

impl CandidateList {
    pub fn used_districts(data: &PgStoreData) -> Vec<ElectoralDistrict> {
        let used: BTreeSet<ElectoralDistrict> = data
            .candidate_lists()
            .into_iter()
            .flat_map(|list| list.electoral_districts.iter().copied())
            .collect();

        used.into_iter().collect()
    }

    pub fn available_districts(
        data: &PgStoreData,
        election: &ElectionConfig,
    ) -> Vec<ElectoralDistrict> {
        election.available_districts(CandidateList::used_districts(data))
    }

    pub async fn update_order(
        &mut self,
        store: &PgStore,
        person_ids: &[PersonId],
    ) -> Result<(), AppError> {
        let existing_person_ids = store
            .snapshot()
            .persons()
            .cloned()
            .collect::<Vec<_>>()
            .iter()
            .map(|p| p.id)
            .collect::<BTreeSet<_>>();

        // never allow a list to grow beyond the store's hard maximum
        if person_ids.len() > store.candidate_limit() {
            return Err(AppError::TooManyCandidates {
                max: store.candidate_limit(),
            });
        }

        // a person holds at most one position on a list
        let distinct = person_ids.iter().collect::<BTreeSet<_>>();
        if distinct.len() != person_ids.len() {
            return Err(AppError::DuplicateCandidate);
        }

        // check all new ids exist
        if !person_ids.iter().all(|id| existing_person_ids.contains(id)) {
            return Err(AppError::GenericNotFound);
        }

        store
            .snapshot()
            .candidate_list(self.id)
            .cloned()
            .or_not_found()?;

        store
            .update(PgEvent::UpdateCandidateListOrder {
                list_id: self.id,
                candidates: person_ids.to_vec(),
            })
            .await?;

        *self = store
            .snapshot()
            .candidate_list(self.id)
            .cloned()
            .or_not_found()?;

        Ok(())
    }

    pub async fn update_position(
        &mut self,
        store: &PgStore,
        id: PersonId,
        position: usize,
    ) -> Result<(), AppError> {
        let Some(current_index) = self.candidates.iter().position(|&pid| pid == id) else {
            return Ok(());
        };

        let moved = self.candidates.remove(current_index);

        // convert the position (1, 2, 3...) to an index (0, 1, 2,..) and clamp it to the valid range
        let target_index = position.saturating_sub(1).min(self.candidates.len());

        self.candidates.insert(target_index, moved);

        self.update_order(store, &self.candidates.clone()).await?;

        Ok(())
    }

    pub async fn append_candidate(
        &mut self,
        store: &PgStore,
        person_id: PersonId,
    ) -> Result<(), AppError> {
        let person = store.snapshot().person(person_id).cloned().or_not_found()?;

        if !self.candidates.contains(&person.id) {
            // never allow a list to grow beyond the store's hard maximum
            if self.candidates.len() >= store.candidate_limit() {
                return Err(AppError::TooManyCandidates {
                    max: store.candidate_limit(),
                });
            }

            store
                .update(PgEvent::AddCandidateToCandidateList {
                    list_id: self.id,
                    person_id: person.id,
                })
                .await?;

            *self = store
                .snapshot()
                .candidate_list(self.id)
                .cloned()
                .or_not_found()?;
        }

        Ok(())
    }

    pub async fn remove_candidate(
        &mut self,
        store: &PgStore,
        person_id: PersonId,
    ) -> Result<(), AppError> {
        if self.candidates.contains(&person_id) {
            store
                .update(PgEvent::RemoveCandidateFromCandidateList {
                    list_id: self.id,
                    person_id,
                })
                .await?;

            *self = store
                .snapshot()
                .candidate_list(self.id)
                .cloned()
                .or_not_found()?;
        }

        Ok(())
    }

    pub async fn get_candidate(
        &self,
        store: &PgStore,
        person_id: PersonId,
    ) -> Result<Candidate, AppError> {
        let list = store
            .snapshot()
            .candidate_list(self.id)
            .cloned()
            .or_not_found()?;

        let position = list
            .position_of(person_id)
            .ok_or(AppError::GenericNotFound)?;

        let person = store.snapshot().person(person_id).cloned().or_not_found()?;

        Ok(Candidate {
            list_id: self.id,
            position,
            person,
        })
    }

    pub fn persons_not_on_list(
        &self,
        store: &PgStore,
        include: &[PersonId],
    ) -> Result<Vec<Person>, AppError> {
        let list = store
            .snapshot()
            .candidate_list(self.id)
            .cloned()
            .or_not_found()?;
        let existing: BTreeMap<PersonId, ()> =
            list.candidates.into_iter().map(|id| (id, ())).collect();

        Ok(store
            .snapshot()
            .sorted_persons()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .filter(|person| !existing.contains_key(&person.id) || include.contains(&person.id))
            .collect())
    }

    pub async fn create(&self, store: &PgStore) -> Result<(), AppError> {
        store
            .update(PgEvent::CreateCandidateList(self.clone()))
            .await
    }

    pub async fn update_districts(&self, store: &PgStore) -> Result<(), AppError> {
        store
            .update(PgEvent::UpdateCandidateListDistricts {
                list_id: self.id,
                electoral_districts: self.electoral_districts.clone(),
            })
            .await
    }

    pub async fn delete(&self, store: &PgStore) -> Result<(), AppError> {
        store.update(PgEvent::DeleteCandidateList(self.id)).await
    }

    pub(crate) fn build_full_candidate_list(
        data: &PgStoreData,
        election: ElectionConfig,
        list: CandidateList,
    ) -> Result<FullCandidateList, AppError> {
        let candidates = list
            .candidates
            .iter()
            .enumerate()
            .map(|(index, person_id)| {
                let person = data.person(*person_id).cloned().or_not_found()?;
                Ok(CandidateWithProblems {
                    problems: person.get_problems(election),
                    data: Candidate {
                        list_id: list.id,
                        position: index + 1,
                        person,
                    },
                })
            })
            .collect::<Result<Vec<CandidateWithProblems>, AppError>>()?;

        Ok(FullCandidateList { list, candidates })
    }
}

impl FullCandidateList {
    pub fn get(
        data: &PgStoreData,
        election: ElectionConfig,
        list_id: CandidateListId,
    ) -> Result<FullCandidateList, AppError> {
        let list = data.candidate_list(list_id).cloned().or_not_found()?;

        CandidateList::build_full_candidate_list(data, election, list)
    }
}

impl CandidateListSummary {
    /// A list is usable once it holds at least one candidate and neither the
    /// list nor any of its candidates have errors
    pub fn is_usable(&self, data: &PgStoreData, election: ElectionConfig) -> bool {
        self.candidate_count() > 0
            && !self
                .get_problems(())
                .has_severity_or_higher(Severity::Error)
            && !self.list.candidates.iter().any(|id| {
                data.person(*id).is_some_and(|person| {
                    person
                        .get_problems(election)
                        .has_severity_or_higher(Severity::Error)
                })
            })
    }
}
