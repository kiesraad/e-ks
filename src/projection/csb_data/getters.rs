use std::collections::{HashMap, HashSet};

use parking_lot::{
    RawRwLock,
    lock_api::{MappedRwLockReadGuard, RwLockReadGuard},
};

use crate::{
    AppError, CsbStream, ElectoralDistrict, Locale, PgStoreData,
    structs::{
        brp::{BrpFinding, BrpStatus},
        candidate_lists::{CandidateList, CandidateListId},
        csb::{Omission, OmissionCategory, OmissionId},
        list_submitters::ListSubmitter,
        name_authorisations::NameAuthorisation,
        persons::{Person, PersonId},
        political_groups::PoliticalGroup,
    },
    trans,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WithCorrections {
    /// Only the original imported data
    None,
    /// Include corrections made in paper correction mode
    Paper,
    /// Also include CSB ("ambtshalve") corrections
    All,
}

impl CsbStream {
    pub fn read(
        &self,
        corrections: WithCorrections,
    ) -> MappedRwLockReadGuard<'_, RawRwLock, PgStoreData> {
        let data = self.data.read();

        match corrections {
            WithCorrections::None => RwLockReadGuard::map(data, |data| &data.imported_data),
            WithCorrections::Paper | WithCorrections::All => {
                // CSB corrections are applied on top of the paper corrected data by the appropriate getters
                RwLockReadGuard::map(data, |data| &data.paper_corrected_data)
            }
        }
    }

    pub fn is_examination_finished(&self) -> bool {
        let data = self.data.read();

        data.is_examination_finished
    }

    pub fn is_deleted(&self) -> bool {
        let data = self.data.read();

        data.is_deleted
    }

    pub fn has_paper_corrections(&self) -> bool {
        let data = self.data.read();

        data.events.iter().any(|event| {
            matches!(
                event.payload.action,
                crate::CsbAction::PaperCorrectedUpdate(_)
            )
        })
    }

    pub fn get_omission(&self, omission_id: OmissionId) -> Result<Omission, AppError> {
        let data = self.data.read();

        data.omissions
            .get(&omission_id)
            .cloned()
            .ok_or(AppError::GenericNotFound)
    }

    pub fn get_omission_count(&self) -> usize {
        let data = self.data.read();

        data.omissions.len()
    }

    /// get the total number of CSB corrections added
    pub fn get_correction_count(&self) -> usize {
        let data = self.data.read();

        data.csb_corrected_persons
            .values()
            .map(|p| p.get_corrections().len())
            .sum::<usize>()
            + data.csb_corrected_appellation.as_ref().map_or(0, |_| 1)
    }

    /// get the total number of CSB corrections and omissions
    pub fn get_restoration_count(&self) -> usize {
        self.get_omission_count() + self.get_correction_count()
    }

    /// The number of omissions that still need a recovered / not-recovered
    /// decision in the "Herstelde lijsten" phase.
    pub fn get_pending_omission_count(&self) -> usize {
        let data = self.data.read();

        data.omissions.values().filter(|o| o.is_pending()).count()
    }

    /// The number of omissions that can be assessed in the "Herstelde lijsten"
    /// phase (irreparable omissions cannot).
    pub fn get_actionable_omission_count(&self) -> usize {
        let data = self.data.read();

        data.omissions
            .values()
            .filter(|o| o.is_actionable())
            .count()
    }

    /// Whether the candidate is scrapped from this list: an unresolved omission
    /// (irreparable or marked not recovered) references the candidate on this
    /// list. Group-level omissions deliberately do not cascade down here; they
    /// are surfaced at the political-group level instead.
    pub fn is_candidate_scrapped(&self, person_id: PersonId, list_id: CandidateListId) -> bool {
        self.get_candidate_omissions(person_id).iter().any(|o| {
            o.is_unresolved()
                && matches!(&o.category, OmissionCategory::Candidate { lists, .. }
                    if lists.contains(&list_id))
        })
    }

    /// The candidate's number in the recovery ("Herstelde lijsten") phase.
    /// A scrapped candidate keeps its place in the order but loses its number,
    /// so the candidates below it move up: the numbering runs over the
    /// candidates that are not scrapped. `None` for a scrapped candidate, and
    /// for a candidate that is not on the list.
    pub fn get_recovery_position(
        &self,
        list_id: CandidateListId,
        person_id: PersonId,
    ) -> Option<usize> {
        let candidates = self
            .read(WithCorrections::All)
            .candidate_lists
            .get(&list_id)?
            .candidates
            .clone();

        let mut position = 0;
        for candidate in candidates {
            if self.is_candidate_scrapped(candidate, list_id) {
                if candidate == person_id {
                    return None;
                }
                continue;
            }

            position += 1;
            if candidate == person_id {
                return Some(position);
            }
        }

        None
    }

    /// Whether the whole candidate list is scrapped: an unresolved list-level
    /// omission references it, or every electoral district it was submitted in
    /// is scrapped. Unresolved omissions of individual candidates scrap only
    /// those candidates, not the list.
    pub fn is_candidate_list_scrapped(&self, list_id: CandidateListId) -> Result<bool, AppError> {
        if self
            .get_candidate_list_omissions(list_id)?
            .iter()
            .any(Omission::is_unresolved)
        {
            return Ok(true);
        }

        let districts = self.get_candidate_list_districts(list_id);

        Ok(!districts.is_empty()
            && districts.len() == self.get_candidate_list_scrapped_districts(list_id).len())
    }

    /// The electoral districts of a candidate list that are scrapped, in the
    /// list's own district order.
    pub fn get_candidate_list_scrapped_districts(
        &self,
        list_id: CandidateListId,
    ) -> Vec<ElectoralDistrict> {
        let scrapped = self.get_scrapped_districts();

        self.get_candidate_list_districts(list_id)
            .into_iter()
            .filter(|district| scrapped.contains(district))
            .collect()
    }

    /// The electoral districts a candidate list was submitted in, corrections
    /// applied. Empty when the list is unknown.
    fn get_candidate_list_districts(&self, list_id: CandidateListId) -> Vec<ElectoralDistrict> {
        self.read(WithCorrections::All)
            .candidate_lists
            .get(&list_id)
            .map(|list| list.electoral_districts.clone())
            .unwrap_or_default()
    }

    /// The districts scrapped by unresolved declarations-of-support omissions,
    /// in the election's district order. An omission without districts covers
    /// all of the election's districts (matching the convention of
    /// `format_districts`).
    pub fn get_scrapped_districts(&self) -> Vec<ElectoralDistrict> {
        let data = self.data.read();

        let scrapped: Vec<ElectoralDistrict> = data
            .omissions
            .values()
            .filter(|o| o.is_unresolved())
            .filter_map(|o| match &o.category {
                OmissionCategory::DeclarationsOfSupport(districts) => Some(districts),
                _ => None,
            })
            .flat_map(|districts| {
                if districts.is_empty() {
                    self.election.electoral_districts().to_vec()
                } else {
                    districts.clone()
                }
            })
            .collect();

        self.election
            .electoral_districts()
            .iter()
            .filter(|district| scrapped.contains(district))
            .copied()
            .collect()
    }

    pub fn get_recoverable_omissions(&self) -> Vec<Omission> {
        let data = self.data.read();

        data.omissions
            .values()
            .filter(|o| o.recoverable)
            .cloned()
            .collect()
    }

    pub fn get_political_group_omissions(&self) -> Vec<Omission> {
        let data = self.data.read();

        data.omissions
            .values()
            .filter(|o| matches!(o.category, OmissionCategory::PoliticalGroup))
            .cloned()
            .collect()
    }

    pub fn get_political_group_csb_corrections_count(&self) -> usize {
        let data = self.data.read();

        match data.csb_corrected_appellation {
            Some(_) => 1,
            None => 0,
        }
    }

    pub fn get_candidate_omissions(&self, person_id: PersonId) -> Vec<Omission> {
        let data = self.data.read();

        data.omissions
            .values()
            .filter(|o| matches!(&o.category, OmissionCategory::Candidate { person, .. } if *person == person_id))
            .cloned()
            .collect()
    }

    /// Whether a candidate has omissions for a specific list
    pub fn has_candidate_omissions(&self, person_id: PersonId, list_id: CandidateListId) -> bool {
        self.get_candidate_omissions(person_id).iter().any(|o| {
            if let OmissionCategory::Candidate {
                person: _person,
                lists,
            } = o.category.clone()
            {
                lists.contains(&list_id)
            } else {
                false
            }
        })
    }

    /// Whether a candidate has csb corrections
    pub fn has_candidate_csb_corrections(&self, person_id: PersonId) -> bool {
        self.get_all_csb_corrected_persons().contains(&person_id)
    }

    /// Return all candidate-list omissions that reference the given list.
    pub fn get_candidate_list_omissions(
        &self,
        list_id: CandidateListId,
    ) -> Result<Vec<Omission>, AppError> {
        if self
            .get_candidate_list(list_id, WithCorrections::All)
            .is_none()
        {
            return Err(AppError::GenericNotFound);
        }

        let data = self.data.read();

        Ok(data
            .omissions
            .values()
            .filter(|o| {
                matches!(&o.category, OmissionCategory::CandidateList(lists)
                    if lists.contains(&list_id))
            })
            .cloned()
            .collect())
    }

    /// Returns if the candidate list or any of its candidates has omissions
    pub fn has_candidate_list_omissions(&self, list_id: CandidateListId) -> Result<bool, AppError> {
        if !self.get_candidate_list_omissions(list_id)?.is_empty() {
            return Ok(true);
        }
        for candidate in self
            .get_candidate_list(list_id, WithCorrections::All)
            .ok_or(AppError::GenericNotFound)?
            .candidates
        {
            if self.has_candidate_omissions(candidate, list_id) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Returns if the candidate list has any candidates with csb corrections
    pub fn has_candidate_list_csb_corrections(
        &self,
        list_id: CandidateListId,
    ) -> Result<bool, AppError> {
        let candidates_on_list = self
            .get_candidate_list(list_id, WithCorrections::All)
            .ok_or(AppError::GenericNotFound)?
            .candidates
            .into_iter()
            .collect::<HashSet<_>>();
        let candidates_with_correction = self
            .get_all_csb_corrected_persons()
            .into_iter()
            .collect::<HashSet<_>>();
        Ok(!candidates_on_list.is_disjoint(&candidates_with_correction))
    }

    pub fn get_all_declarations_of_support_omissions(&self) -> Vec<Omission> {
        let data = self.data.read();

        data.omissions
            .values()
            .filter(|o| matches!(o.category, OmissionCategory::DeclarationsOfSupport(_)))
            .cloned()
            .collect()
    }

    pub fn get_political_group(&self, corrections: WithCorrections) -> PoliticalGroup {
        let mut pg = self.read(corrections).political_group.clone();

        if corrections == WithCorrections::All
            && let Some(correction) = self.data.read().csb_corrected_appellation.clone()
        {
            pg.appellation = Some(correction);
        }

        pg
    }

    pub fn get_candidate_lists(&self, corrections: WithCorrections) -> Vec<CandidateList> {
        self.read(corrections)
            .candidate_lists
            .values()
            .cloned()
            .collect()
    }

    /// The candidate list with this id, if any.
    pub fn get_candidate_list(
        &self,
        list_id: CandidateListId,
        corrections: WithCorrections,
    ) -> Option<CandidateList> {
        self.read(corrections)
            .candidate_lists
            .get(&list_id)
            .cloned()
    }

    /// The person (candidate) with this id, if any.
    pub fn get_person(&self, person_id: PersonId, corrections: WithCorrections) -> Option<Person> {
        let mut person = self.read(corrections).persons.get(&person_id).cloned()?;

        if corrections == WithCorrections::All
            && let Some(delta) = self
                .data
                .read()
                .csb_corrected_persons
                .get(&person_id)
                .cloned()
        {
            delta.apply(&mut person);
        }

        Some(person)
    }

    pub fn get_all_csb_corrected_persons(&self) -> Vec<PersonId> {
        self.data
            .read()
            .csb_corrected_persons
            .keys()
            .cloned()
            .collect()
    }

    /// The name of the first candidate across all candidate lists, sorted by list
    /// creation date. Returns `None` when no candidates are available.
    pub fn get_first_candidate_name(
        &self,
        corrections: WithCorrections,
    ) -> Option<crate::structs::common::FullName> {
        let mut lists = self.get_candidate_lists(corrections);
        lists.sort_unstable_by_key(|l| l.created_at);
        lists
            .into_iter()
            .flat_map(|list| list.candidates.into_iter())
            .next()
            .and_then(|id| self.get_person(id, corrections))
            .map(|p| p.name)
    }

    /// Short-hand to get the appellation of the political group (including special names for blank lists)
    pub fn get_appellation(&self, corrections: WithCorrections) -> String {
        let political_group = self.get_political_group(corrections);
        political_group.csb_appellation(self.get_first_candidate_name(corrections).as_ref())
    }

    /// Short-hand to get the appellation of the political group (including special names for blank lists).
    /// Additionally includes a deleted label when the political group has been deleted
    pub fn get_appellation_with_deleted_label(
        &self,
        corrections: WithCorrections,
        locale: Locale,
    ) -> String {
        let political_group = self.get_political_group(corrections);
        let appellation =
            political_group.csb_appellation(self.get_first_candidate_name(corrections).as_ref());
        if self.is_deleted() {
            format!(
                "{appellation} ({})",
                trans!("csb.group.deleted_label", locale)
            )
        } else {
            appellation
        }
    }

    /// One-based position of the candidate on the given list
    pub fn get_candidate_position(
        &self,
        list_id: CandidateListId,
        person_id: PersonId,
        corrections: WithCorrections,
    ) -> Option<usize> {
        self.read(corrections)
            .candidate_lists
            .get(&list_id)?
            .position_of(person_id)
    }

    pub fn get_list_submitter(&self, corrections: WithCorrections) -> ListSubmitter {
        self.read(corrections).list_submitter.clone()
    }

    pub fn get_substitute_submitters(&self, corrections: WithCorrections) -> Vec<ListSubmitter> {
        ListSubmitter::clone_as_substitutes(&self.read(corrections).substitute_submitters)
    }

    pub fn get_name_authorisations(&self, corrections: WithCorrections) -> Vec<NameAuthorisation> {
        self.read(corrections)
            .name_authorisations
            .values()
            .cloned()
            .collect()
    }

    /// Every candidate, with the corrections `corrections` asks for. Routed
    /// through [`Self::get_person`], which applies the ambtshalve corrections
    /// that live beside the projection.
    pub fn get_persons(&self, corrections: WithCorrections) -> Vec<Person> {
        // The read guard is dropped before `get_person` takes it again.
        let ids: Vec<PersonId> = self.read(corrections).persons.keys().copied().collect();

        ids.into_iter()
            .filter_map(|id| self.get_person(id, corrections))
            .collect()
    }

    /// Per checked candidate; candidates absent from the map were not checked.
    pub fn get_brp_findings(&self) -> HashMap<PersonId, Vec<BrpFinding>> {
        self.data.read().brp_findings.clone()
    }

    /// An empty list covers both "checked, nothing found" and "not checked";
    /// use [`Self::get_brp_findings`] when the difference matters.
    pub fn get_brp_findings_for_person(&self, person_id: PersonId) -> Vec<BrpFinding> {
        self.data
            .read()
            .brp_findings
            .get(&person_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Whether this candidate has been checked, findings or not.
    pub fn is_brp_checked(&self, person_id: PersonId) -> bool {
        self.data.read().brp_findings.contains_key(&person_id)
    }

    /// How far the BRP sweep for this stream got.
    pub fn get_brp_status(&self) -> BrpStatus {
        self.data.read().brp_validation_status.clone()
    }

    /// Return the single stored omission. Test-only helper for asserting on
    /// omissions whose category has no dedicated getter (e.g. candidate lists).
    #[cfg(test)]
    pub fn get_omission_for_test(&self) -> Omission {
        self.data
            .read()
            .omissions
            .values()
            .next()
            .cloned()
            .expect("expected exactly one stored omission")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CsbStore, CsbStream, ElectoralDistrict,
        structs::{
            candidate_lists::CandidateList,
            csb::{
                OmissionCategory, OmissionStatus, PersonCorrection, PersonCorrectionDelta,
                sample_omission,
            },
            list_designation::ListDesignation,
        },
        test_utils::{sample_candidate_list, sample_person, sample_person_with},
    };

    fn insert(store: &CsbStream, category: OmissionCategory) {
        let omission = sample_omission(category);
        store.data.write().omissions.insert(omission.id, omission);
    }

    fn insert_list(store: &CsbStream, list_id: CandidateListId, districts: Vec<ElectoralDistrict>) {
        let list = CandidateList {
            id: list_id,
            electoral_districts: districts,
            ..Default::default()
        };
        store
            .data
            .write()
            .imported_data
            .candidate_lists
            .insert(list_id, list.clone());
        store
            .data
            .write()
            .paper_corrected_data
            .candidate_lists
            .insert(list_id, list);
    }

    fn insert_with_status(
        store: &CsbStore,
        category: OmissionCategory,
        recoverable: bool,
        status: crate::structs::csb::OmissionStatus,
    ) {
        let mut omission = sample_omission(category);
        omission.recoverable = recoverable;
        omission.status = status;
        store.data.write().omissions.insert(omission.id, omission);
    }

    #[test]
    fn pending_and_actionable_counts_skip_irreparable_omissions() {
        let store = CsbStore::new_for_test();
        insert(&store, OmissionCategory::PoliticalGroup);
        insert_with_status(
            &store,
            OmissionCategory::PoliticalGroup,
            true,
            OmissionStatus::Recovered,
        );
        insert_with_status(
            &store,
            OmissionCategory::PoliticalGroup,
            false,
            OmissionStatus::Pending,
        );

        // The irreparable omission needs no decision and is not actionable.
        assert_eq!(store.get_pending_omission_count(), 1);
        assert_eq!(store.get_actionable_omission_count(), 2);
    }

    #[test]
    fn is_candidate_scrapped_only_for_unresolved_omissions_on_that_list() {
        let person = PersonId::new();
        let list_a = CandidateListId::new();
        let list_b = CandidateListId::new();
        let store = CsbStore::new_for_test();

        // A pending omission is not (yet) unresolved.
        insert(
            &store,
            OmissionCategory::Candidate {
                person,
                lists: vec![list_a],
            },
        );
        assert!(!store.is_candidate_scrapped(person, list_a));

        insert_with_status(
            &store,
            OmissionCategory::Candidate {
                person,
                lists: vec![list_a],
            },
            true,
            OmissionStatus::NotRecovered,
        );

        assert!(store.is_candidate_scrapped(person, list_a));
        // Scoped to the lists the omission references.
        assert!(!store.is_candidate_scrapped(person, list_b));
        assert!(!store.is_candidate_scrapped(PersonId::new(), list_a));
    }

    #[test]
    fn get_recovery_position_renumbers_around_scrapped_candidates() {
        let store = CsbStore::new_for_test();
        let list_id = CandidateListId::new();
        let (first, scrapped, last) = (PersonId::new(), PersonId::new(), PersonId::new());
        store.add_candidate_list(CandidateList {
            id: list_id,
            candidates: vec![first, scrapped, last],
            electoral_districts: vec![ElectoralDistrict::Groningen],
            ..Default::default()
        });

        insert_with_status(
            &store,
            OmissionCategory::Candidate {
                person: scrapped,
                lists: vec![list_id],
            },
            true,
            OmissionStatus::NotRecovered,
        );

        assert_eq!(store.get_recovery_position(list_id, first), Some(1));
        // The scrapped candidate keeps its place in the order but loses its
        // number, so the candidate below moves up.
        assert_eq!(store.get_recovery_position(list_id, scrapped), None);
        assert_eq!(store.get_recovery_position(list_id, last), Some(2));

        assert_eq!(store.get_recovery_position(list_id, PersonId::new()), None);
    }

    #[test]
    fn candidate_list_is_scrapped_once_all_its_districts_are_scrapped() {
        let store = CsbStore::new_for_test();
        let list_id = CandidateListId::new();
        insert_list(
            &store,
            list_id,
            vec![ElectoralDistrict::Groningen, ElectoralDistrict::Drenthe],
        );

        insert_with_status(
            &store,
            OmissionCategory::DeclarationsOfSupport(vec![ElectoralDistrict::Groningen]),
            true,
            OmissionStatus::NotRecovered,
        );

        assert_eq!(
            store.get_candidate_list_scrapped_districts(list_id),
            vec![ElectoralDistrict::Groningen]
        );
        // One of the two districts is gone, the list itself is not.
        assert!(!store.is_candidate_list_scrapped(list_id).unwrap());

        insert_with_status(
            &store,
            OmissionCategory::DeclarationsOfSupport(vec![ElectoralDistrict::Drenthe]),
            true,
            OmissionStatus::NotRecovered,
        );

        assert_eq!(
            store.get_candidate_list_scrapped_districts(list_id),
            vec![ElectoralDistrict::Groningen, ElectoralDistrict::Drenthe]
        );
        assert!(store.is_candidate_list_scrapped(list_id).unwrap());
    }

    #[test]
    fn is_candidate_scrapped_by_an_irreparable_omission() {
        let person = PersonId::new();
        let list = CandidateListId::new();
        let store = CsbStore::new_for_test();
        insert_with_status(
            &store,
            OmissionCategory::Candidate {
                person,
                lists: vec![list],
            },
            false,
            OmissionStatus::Pending,
        );

        assert!(store.is_candidate_scrapped(person, list));
    }

    #[test]
    fn is_candidate_list_scrapped_ignores_recovered_and_candidate_omissions() {
        let list_id = CandidateListId::new();
        let store = CsbStore::new_for_test();
        insert_list(&store, list_id, vec![ElectoralDistrict::Groningen]);

        insert_with_status(
            &store,
            OmissionCategory::CandidateList(vec![list_id]),
            true,
            OmissionStatus::Recovered,
        );
        // An unresolved omission of a candidate scraps the candidate, not the list.
        insert_with_status(
            &store,
            OmissionCategory::Candidate {
                person: PersonId::new(),
                lists: vec![list_id],
            },
            true,
            OmissionStatus::NotRecovered,
        );
        assert!(!store.is_candidate_list_scrapped(list_id).unwrap());

        insert_with_status(
            &store,
            OmissionCategory::CandidateList(vec![list_id]),
            true,
            OmissionStatus::NotRecovered,
        );
        assert!(store.is_candidate_list_scrapped(list_id).unwrap());

        assert!(
            store
                .is_candidate_list_scrapped(CandidateListId::new())
                .is_err()
        );
    }

    #[test]
    fn get_scrapped_districts_sorts_and_expands_empty_to_all() {
        let store = CsbStore::new_for_test();
        insert_with_status(
            &store,
            OmissionCategory::DeclarationsOfSupport(vec![
                ElectoralDistrict::Drenthe,
                ElectoralDistrict::Groningen,
            ]),
            true,
            OmissionStatus::NotRecovered,
        );
        // Recovered and pending omissions scrap nothing.
        insert_with_status(
            &store,
            OmissionCategory::DeclarationsOfSupport(vec![ElectoralDistrict::Bonaire]),
            true,
            OmissionStatus::Recovered,
        );
        insert(
            &store,
            OmissionCategory::DeclarationsOfSupport(vec![ElectoralDistrict::Utrecht]),
        );

        assert_eq!(
            store.get_scrapped_districts(),
            vec![ElectoralDistrict::Groningen, ElectoralDistrict::Drenthe]
        );

        // An unresolved omission without districts covers all districts.
        insert_with_status(
            &store,
            OmissionCategory::DeclarationsOfSupport(vec![]),
            false,
            OmissionStatus::Pending,
        );
        assert_eq!(
            store.get_scrapped_districts(),
            store.election.electoral_districts().to_vec()
        );
    }

    #[test]
    fn get_political_group_omissions_returns_only_political_group() {
        let store = CsbStore::new_for_test();
        insert(&store, OmissionCategory::PoliticalGroup);
        insert(
            &store,
            OmissionCategory::CandidateList(vec![CandidateListId::new()]),
        );

        let result = store.get_political_group_omissions();

        assert_eq!(result.len(), 1);
        assert!(matches!(
            result[0].category,
            OmissionCategory::PoliticalGroup
        ));
    }

    #[test]
    fn get_political_group_omissions_returns_empty_when_none() {
        let store = CsbStore::new_for_test();
        insert(
            &store,
            OmissionCategory::CandidateList(vec![CandidateListId::new()]),
        );

        assert!(store.get_political_group_omissions().is_empty());
    }

    #[test]
    fn get_candidate_omissions_returns_only_omissions_for_the_given_person() {
        let person_a = PersonId::new();
        let person_b = PersonId::new();
        let store = CsbStore::new_for_test();
        insert(
            &store,
            OmissionCategory::Candidate {
                person: person_a,
                lists: Vec::new(),
            },
        );
        insert(
            &store,
            OmissionCategory::Candidate {
                person: person_b,
                lists: Vec::new(),
            },
        );
        insert(&store, OmissionCategory::PoliticalGroup);

        let result = store.get_candidate_omissions(person_a);

        assert_eq!(result.len(), 1);
        assert!(
            matches!(result[0].category, OmissionCategory::Candidate { person, .. } if person == person_a)
        );
    }

    #[test]
    fn get_candidate_omissions_returns_empty_when_no_match() {
        let store = CsbStore::new_for_test();
        insert(
            &store,
            OmissionCategory::Candidate {
                person: PersonId::new(),
                lists: Vec::new(),
            },
        );

        assert!(store.get_candidate_omissions(PersonId::new()).is_empty());
    }

    #[test]
    fn get_candidate_list_omissions_returns_omissions_referencing_that_list() {
        let list_a = CandidateListId::new();
        let list_b = CandidateListId::new();
        let store = CsbStore::new_for_test();
        insert_list(&store, list_a, vec![ElectoralDistrict::Groningen]);
        insert_list(&store, list_b, vec![ElectoralDistrict::Drenthe]);
        insert(&store, OmissionCategory::CandidateList(vec![list_a]));
        insert(&store, OmissionCategory::CandidateList(vec![list_b]));
        insert(&store, OmissionCategory::PoliticalGroup);

        let result_a = store.get_candidate_list_omissions(list_a).unwrap();
        let result_b = store.get_candidate_list_omissions(list_b).unwrap();

        assert_eq!(result_a.len(), 1);
        assert!(
            matches!(&result_a[0].category, OmissionCategory::CandidateList(ids) if ids == &[list_a])
        );
        assert_eq!(result_b.len(), 1);
        assert!(
            matches!(&result_b[0].category, OmissionCategory::CandidateList(ids) if ids == &[list_b])
        );
    }

    #[test]
    fn get_candidate_list_prefers_the_paper_corrected_version() {
        let list_id = CandidateListId::new();
        let store = CsbStore::new_for_test();
        insert_list(&store, list_id, vec![ElectoralDistrict::Utrecht]);
        store.set_paper_corrected_candidate_list(CandidateList {
            id: list_id,
            electoral_districts: vec![ElectoralDistrict::Groningen],
            ..Default::default()
        });

        let list = store
            .get_candidate_list(list_id, WithCorrections::None)
            .unwrap();
        assert_eq!(list.electoral_districts, vec![ElectoralDistrict::Utrecht]);
        let list = store
            .get_candidate_list(list_id, WithCorrections::Paper)
            .unwrap();
        assert_eq!(list.electoral_districts, vec![ElectoralDistrict::Groningen]);
    }

    #[test]
    fn get_person_falls_back_to_a_paper_added_person() {
        let store = CsbStore::new_for_test();
        let person_id = PersonId::new();
        let person = sample_person_with(person_id, None, "Jansen", None, "A.B.");
        store
            .data
            .write()
            .paper_corrected_data
            .persons
            .insert(person_id, person);

        assert!(store.get_person(person_id, WithCorrections::None).is_none());
        assert!(
            store
                .get_person(person_id, WithCorrections::Paper)
                .is_some()
        );
    }

    #[test]
    fn get_candidate_list_omissions_errors_for_unknown_list() {
        let store = CsbStore::new_for_test();
        insert(
            &store,
            OmissionCategory::CandidateList(vec![CandidateListId::new()]),
        );

        assert!(
            store
                .get_candidate_list_omissions(CandidateListId::new())
                .is_err()
        );
    }

    #[test]
    fn get_substitute_submitters_marks_each_as_substitute() {
        let store = CsbStore::new_for_test();
        store
            .data
            .write()
            .imported_data
            .substitute_submitters
            .push(ListSubmitter::default());

        let result = store.get_substitute_submitters(WithCorrections::None);

        assert_eq!(result.len(), 1);
        assert!(result[0].is_substitute);
    }

    #[test]
    fn get_substitute_submitters_returns_empty_when_none() {
        let store = CsbStore::new_for_test();

        assert!(
            store
                .get_substitute_submitters(WithCorrections::All)
                .is_empty()
        );
    }

    #[test]
    fn csb_appellation_standalone_list_uses_appellation() {
        let store = CsbStore::new_for_test();
        store.set_political_group(PoliticalGroup {
            appellation: Some("Kiesraad Demo".parse().unwrap()),
            list_designation: Some(ListDesignation::Standalone),
            ..Default::default()
        });

        assert_eq!(store.get_appellation(WithCorrections::All), "Kiesraad Demo");
    }

    #[test]
    fn csb_appellation_blank_list_with_candidate_uses_first_candidate_name() {
        let store = CsbStore::new_for_test();
        store.set_political_group(PoliticalGroup {
            list_designation: Some(ListDesignation::Blank),
            ..Default::default()
        });

        let person_id = PersonId::new();
        let person = sample_person_with(person_id, None, "Jansen", None, "A.B.");
        store.add_person(person);

        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates.push(person_id);
        store.add_candidate_list(list);

        assert_eq!(
            store.get_appellation(WithCorrections::All),
            "Blanco (Jansen, A.B.)"
        );
    }

    #[test]
    fn csb_appellation_blank_list_without_candidates_uses_blanco_fallback() {
        let store = CsbStore::new_for_test();
        store.set_political_group(PoliticalGroup {
            list_designation: Some(ListDesignation::Blank),
            ..Default::default()
        });

        assert_eq!(store.get_appellation(WithCorrections::All), "Blanco");
    }

    #[test]
    fn get_persons_applies_the_committees_own_corrections() {
        let store = CsbStore::new_for_test();
        let person = sample_person(PersonId::new());
        let person_id = person.id;
        store.add_person(person);

        // An ambtshalve correction is a delta beside the projection, and the
        // BRP check examines the corrected data.
        let mut delta = PersonCorrectionDelta::default();
        delta.add_correction(PersonCorrection::LastName("Gecorrigeerd".parse().unwrap()));
        store
            .data
            .write()
            .csb_corrected_persons
            .insert(person_id, delta);

        let corrected = store.get_persons(WithCorrections::All);
        assert_eq!(corrected.len(), 1);
        assert_eq!(corrected[0].name.last_name.to_string(), "Gecorrigeerd");

        // The imported data is untouched.
        let imported = store.get_persons(WithCorrections::None);
        assert_eq!(imported[0].name.last_name.to_string(), "Jansen");
    }
}
