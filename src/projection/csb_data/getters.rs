//! Reads on a CSB stream's projection. The political group's data at a
//! correction level is reached through [`CsbStoreData::pg`], which hands out
//! a [`PgStoreData`](crate::PgStoreData) with the getters shared with the
//! political group app.

use std::collections::{HashMap, HashSet};

use super::{CsbStoreData, Scrapped, WithCorrections};
use crate::{
    AppError, CsbAction, ElectionConfig, ElectoralDistrict, Locale, PgEvent,
    structs::{
        brp::{BrpFinding, BrpStatus},
        candidate_lists::CandidateListId,
        common::{Appellation, FullName, PotentialProblems},
        csb::{
            Omission, OmissionCategory, OmissionId, OmissionTitle, PersonCorrection,
            PersonCorrectionDelta, RecoveryProgress,
        },
        list_designation::ListDesignation,
        persons::{Person, PersonId},
        problems::AllProblems,
    },
    trans,
};

/// A candidate at the three correction levels; a level the candidate is
/// absent from (added or deleted on paper) is `None`.
#[derive(Clone, Copy)]
pub struct PersonVersions<'a> {
    pub imported: Option<&'a Person>,
    pub paper: Option<&'a Person>,
    pub csb: Option<&'a Person>,
}

impl CsbStoreData {
    /// The candidate as imported, as corrected on paper and as corrected by
    /// the committee.
    pub fn get_person_versions(&self, person_id: PersonId) -> PersonVersions<'_> {
        let at = |corrections| self.pg(corrections).get_person(person_id).ok();
        PersonVersions {
            imported: at(WithCorrections::None),
            paper: at(WithCorrections::Paper),
            csb: at(WithCorrections::All),
        }
    }

    pub fn is_examination_finished(&self) -> bool {
        self.is_examination_finished
    }

    pub fn is_deleted(&self) -> bool {
        self.is_deleted
    }

    /// The paper corrections, as the political group's events they wrap.
    fn paper_corrections(&self) -> impl Iterator<Item = &PgEvent> {
        self.events
            .iter()
            .filter_map(|event| match &event.payload.action {
                CsbAction::PaperCorrectedUpdate(payload) => Some(&**payload),
                _ => None,
            })
    }

    pub fn has_paper_corrections(&self) -> bool {
        self.paper_corrections().next().is_some()
    }

    /// Whether paper corrections changed more than which lists, districts and
    /// candidates there are: that much shows by comparing the projections.
    pub fn has_paper_corrected_values(&self) -> bool {
        self.paper_corrections()
            .any(|event| !only_adds_or_removes(event))
    }

    pub fn get_omission(&self, omission_id: OmissionId) -> Result<&Omission, AppError> {
        self.omissions
            .get(&omission_id)
            .ok_or(AppError::GenericNotFound)
    }

    pub fn get_omission_count(&self) -> usize {
        self.omissions.len()
    }

    /// get the total number of CSB corrections added
    pub fn get_correction_count(&self) -> usize {
        self.csb_corrected_persons
            .values()
            .map(|p| p.get_corrections().len())
            .sum::<usize>()
            + self.get_political_group_csb_corrections_count()
    }

    /// get the total number of CSB corrections and omissions
    pub fn get_restoration_count(&self) -> usize {
        self.get_omission_count() + self.get_correction_count()
    }

    /// How far the group is through the "Herstelde lijsten" phase, counted in
    /// recovery decisions (see [`RecoveryProgress`]).
    pub fn get_recovery_progress(&self, election: ElectionConfig) -> RecoveryProgress {
        let mut progress = RecoveryProgress::default();
        for omission in self.omissions.values().filter(|o| o.is_actionable()) {
            let decisions = omission.decision_count(&election);
            progress.total += decisions;
            if omission.is_pending() {
                progress.pending += decisions;
            }
        }

        progress
    }

    /// What the unresolved omissions scrap, as derived after the last event.
    pub fn get_scrapped(&self) -> &Scrapped {
        &self.scrapped
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
        let candidates = &self
            .csb_corrected_data
            .get_candidate_list(list_id)
            .ok()?
            .candidates;

        let mut position = 0;
        for &candidate in candidates {
            if self.scrapped.is_candidate_scrapped(list_id, candidate) {
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

    pub fn get_omissions(&self) -> Vec<Omission> {
        self.omissions.values().cloned().collect()
    }

    fn omissions_where(&self, matches: impl Fn(&Omission) -> bool) -> Vec<Omission> {
        self.omissions
            .values()
            .filter(|omission| matches(omission))
            .cloned()
            .collect()
    }

    pub fn get_political_group_omissions(&self) -> Vec<Omission> {
        self.omissions_where(|o| matches!(o.category, OmissionCategory::PoliticalGroup))
    }

    pub fn get_appellation_omissions(&self) -> Vec<Omission> {
        self.omissions_where(|o| matches!(o.category, OmissionCategory::Appellation))
    }

    pub fn get_political_group_csb_corrections_count(&self) -> usize {
        usize::from(self.csb_corrected_appellation.is_some())
    }

    /// The candidate's omissions in title order, only those on `list_id` when
    /// given.
    pub fn get_candidate_omissions(
        &self,
        person_id: PersonId,
        list_id: Option<CandidateListId>,
        election: ElectionConfig,
    ) -> Vec<Omission> {
        let mut omissions =
            self.omissions_where(|o| o.category.is_for_candidate(person_id, list_id));
        omissions.sort_by_cached_key(|omission| self.title_order(omission, election));
        omissions
    }

    /// Whether a candidate has omissions for a specific list
    pub fn has_candidate_omissions(&self, person_id: PersonId, list_id: CandidateListId) -> bool {
        self.omissions
            .values()
            .any(|o| o.category.is_for_candidate(person_id, Some(list_id)))
    }

    /// Whether a candidate has csb corrections
    pub fn has_candidate_csb_corrections(&self, person_id: PersonId) -> bool {
        self.csb_corrected_persons.contains_key(&person_id)
    }

    /// Whether the paper corrections changed or added the candidate.
    pub fn is_person_paper_corrected(&self, person_id: PersonId) -> bool {
        let versions = self.get_person_versions(person_id);
        versions.imported != versions.paper
    }

    fn candidate_list_omissions(&self, list_id: CandidateListId) -> Vec<Omission> {
        self.omissions_where(|o| {
            matches!(&o.category, OmissionCategory::CandidateList(lists) if lists.contains(&list_id))
        })
    }

    /// All candidate-list omissions that reference the given list, in title
    /// order.
    pub fn get_candidate_list_omissions(
        &self,
        list_id: CandidateListId,
        election: ElectionConfig,
    ) -> Result<Vec<Omission>, AppError> {
        self.csb_corrected_data.get_candidate_list(list_id)?;

        let mut omissions = self.candidate_list_omissions(list_id);
        omissions.sort_by_cached_key(|omission| self.title_order(omission, election));
        Ok(omissions)
    }

    /// Whether the candidate list or any of its candidates has omissions
    pub fn has_candidate_list_omissions(&self, list_id: CandidateListId) -> Result<bool, AppError> {
        let list = self.csb_corrected_data.get_candidate_list(list_id)?;

        Ok(!self.candidate_list_omissions(list_id).is_empty()
            || list
                .candidates
                .iter()
                .any(|&candidate| self.has_candidate_omissions(candidate, list_id)))
    }

    /// Whether the candidate list has any candidates with csb corrections
    pub fn has_candidate_list_csb_corrections(
        &self,
        list_id: CandidateListId,
    ) -> Result<bool, AppError> {
        let list = self.csb_corrected_data.get_candidate_list(list_id)?;

        Ok(list
            .candidates
            .iter()
            .any(|candidate| self.has_candidate_csb_corrections(*candidate)))
    }

    pub fn get_all_declarations_of_support_omissions(
        &self,
        election: ElectionConfig,
    ) -> Vec<Omission> {
        let mut omissions = self
            .omissions_where(|o| matches!(o.category, OmissionCategory::DeclarationsOfSupport(_)));
        omissions.sort_by_cached_key(|omission| self.title_order(omission, election));
        omissions
    }

    /// Sort key putting omissions in title order, then district order, so the
    /// parts of a split stay together.
    pub(crate) fn title_order(
        &self,
        omission: &Omission,
        election: ElectionConfig,
    ) -> (OmissionTitle, usize) {
        let order = election.electoral_districts();
        let first_district = self
            .omission_districts(omission, election)
            .first()
            .and_then(|district| order.iter().position(|d| d == district))
            .unwrap_or(usize::MAX);

        (omission.title.to_owned(), first_district)
    }

    /// The districts an omission touches, directly or through its lists.
    fn omission_districts(
        &self,
        omission: &Omission,
        election: ElectionConfig,
    ) -> Vec<ElectoralDistrict> {
        let districts = omission.electoral_districts(&election);
        if !districts.is_empty() {
            return districts.to_vec();
        }

        omission
            .candidate_lists()
            .iter()
            .filter_map(|list_id| self.csb_corrected_data.get_candidate_list(*list_id).ok())
            .flat_map(|list| list.electoral_districts.iter().copied())
            .collect()
    }

    pub fn get_all_csb_corrected_persons(&self) -> Vec<PersonId> {
        self.csb_corrected_persons.keys().copied().collect()
    }

    /// The committee's corrections on a candidate, if any.
    pub fn get_person_corrections(&self, person_id: PersonId) -> HashSet<PersonCorrection> {
        self.csb_corrected_persons
            .get(&person_id)
            .map(PersonCorrectionDelta::get_corrections)
            .unwrap_or_default()
    }

    /// The committee's correction of the appellation, if any.
    pub fn get_csb_corrected_appellation(&self) -> Option<&Appellation> {
        self.csb_corrected_appellation.as_ref()
    }

    /// The name of the first unscrapped candidate on the first unscrapped
    /// list, in list creation order; `None` without candidates. Without
    /// scrapping, use [`crate::PgStoreData::get_first_candidate_name`].
    pub fn get_first_candidate_name(
        &self,
        corrections: WithCorrections,
        scrapped: &Scrapped,
    ) -> Option<&FullName> {
        let data = self.pg(corrections);
        data.candidates_in_list_order()
            .filter(|(list, person)| {
                !scrapped.is_list_scrapped(*list) && !scrapped.is_candidate_scrapped(*list, *person)
            })
            .find_map(|(_, person)| data.get_person(person).ok())
            .map(|person| &person.name)
    }

    pub fn get_appellation_with_scrapped(
        &self,
        corrections: WithCorrections,
        scrapped: &Scrapped,
    ) -> String {
        let mut political_group = self.pg(corrections).get_political_group().clone();
        if scrapped.is_appellation_scrapped() {
            political_group.list_designation = Some(ListDesignation::Blank);
        }
        political_group.csb_appellation(self.get_first_candidate_name(corrections, scrapped))
    }

    /// The appellation of the political group (including special names for
    /// blank lists), with a deleted label when the political group has been
    /// deleted.
    pub fn get_appellation_with_deleted_label(
        &self,
        corrections: WithCorrections,
        locale: Locale,
    ) -> String {
        let appellation = self.pg(corrections).get_appellation();
        if self.is_deleted {
            format!(
                "{appellation} ({})",
                trans!("csb.group.deleted_label", locale)
            )
        } else {
            appellation
        }
    }

    /// Per checked candidate; candidates absent from the map were not checked.
    pub fn get_brp_findings(&self) -> &HashMap<PersonId, Vec<BrpFinding>> {
        &self.brp_findings
    }

    /// An empty list covers both "checked, nothing found" and "not checked";
    /// use [`Self::get_brp_findings`] when the difference matters.
    pub fn get_brp_findings_for_person(&self, person_id: PersonId) -> &[BrpFinding] {
        self.brp_findings
            .get(&person_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// Whether this candidate has been checked, findings or not.
    pub fn is_brp_checked(&self, person_id: PersonId) -> bool {
        self.brp_findings.contains_key(&person_id)
    }

    /// How far the BRP sweep for this stream got.
    pub fn get_brp_status(&self) -> &BrpStatus {
        &self.brp_validation_status
    }

    /// Return the single stored omission. Test-only helper for asserting on
    /// omissions whose category has no dedicated getter (e.g. candidate lists).
    #[cfg(test)]
    pub fn get_omission_for_test(&self) -> Omission {
        self.omissions
            .values()
            .next()
            .cloned()
            .expect("expected exactly one stored omission")
    }

    /// Collect all problems of the fully corrected data, excluding info
    /// problems and candidate problems the CSB does not act on.
    pub fn get_all_problems(&self, election: ElectionConfig) -> Result<AllProblems, AppError> {
        AllProblems::find_all(&self.csb_corrected_data, election).map(|mut problems| {
            problems.info_problems = Vec::new();
            for candidate in &mut problems.candidates {
                candidate
                    .problems
                    .retain(PotentialProblems::is_relevant_for_csb);
            }
            problems
                .candidates
                .retain(|candidate| !candidate.problems.is_empty());
            problems
        })
    }
}

/// Whether an app event only adds or removes a list, district or candidate.
fn only_adds_or_removes(event: &PgEvent) -> bool {
    match event {
        PgEvent::CreateCandidateList(_)
        | PgEvent::DeleteCandidateList(_)
        | PgEvent::UpdateCandidateListDistricts { .. }
        | PgEvent::AddCandidateToCandidateList { .. }
        | PgEvent::RemoveCandidateFromCandidateList { .. }
        | PgEvent::CreatePerson(_)
        | PgEvent::CreatePersonPersonalData { .. }
        | PgEvent::DeletePerson { .. } => true,
        PgEvent::ImportCandidates(import) => import.updated_persons.is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        sync::Arc,
    };

    use super::*;
    use crate::{
        CsbAction, CsbStore, CsbStream, ElectoralDistrict,
        projection::csb_data::scrapped::ScrappedList,
        structs::{
            candidate_lists::CandidateList,
            common::UtcDateTime,
            csb::{
                Correction, OmissionCategory, OmissionStatus, PersonCorrection,
                PersonCorrectionDelta, sample_omission,
            },
            list_designation::ListDesignation,
            political_groups::PoliticalGroup,
        },
        test_utils::{sample_candidate_list, sample_person, sample_person_with},
    };

    fn insert(store: &CsbStream, category: OmissionCategory) {
        let omission = sample_omission(category);
        let mut data = store.write();
        data.omissions.insert(omission.id, omission);
        data.refresh_derived();
    }

    #[test]
    fn a_candidate_as_imported_is_not_paper_corrected() {
        let store = CsbStream::new_for_test();
        let person = sample_person(PersonId::new());
        store.add_person(person.clone());

        let data = store.snapshot();
        assert!(!data.is_person_paper_corrected(person.id));
        assert!(!data.is_person_paper_corrected(PersonId::new()));
    }

    #[test]
    fn a_candidate_changed_on_paper_is_paper_corrected() {
        let store = CsbStream::new_for_test();
        let person = sample_person(PersonId::new());
        store.add_person(person.clone());
        let mut on_paper = person.clone();
        on_paper.name.last_name = "Gewijzigd".parse().unwrap();
        store.set_paper_corrected_person(on_paper);

        assert!(store.snapshot().is_person_paper_corrected(person.id));
    }

    #[test]
    fn a_candidate_added_on_paper_is_paper_corrected() {
        let store = CsbStream::new_for_test();
        let person = sample_person(PersonId::new());
        store.set_paper_corrected_person(person.clone());

        assert!(store.snapshot().is_person_paper_corrected(person.id));
    }

    #[tokio::test]
    async fn a_csb_corrected_candidate_is_not_paper_corrected() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();
        let person = sample_person(PersonId::new());
        store.add_person(person.clone());
        store
            .update(CsbAction::UpdateCorrection(Correction::Person(
                person.id,
                PersonCorrection::LastName("Gecorrigeerd".parse().unwrap()),
            )))
            .await?;

        let data = store.snapshot();
        assert!(data.has_candidate_csb_corrections(person.id));
        assert!(!data.is_person_paper_corrected(person.id));

        Ok(())
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
        let mut data = store.write();
        data.omissions.insert(omission.id, omission);
        data.refresh_derived();
    }

    #[test]
    fn recovery_progress_skips_irreparable_omissions() {
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
        assert_eq!(
            store.snapshot().get_recovery_progress(store.election),
            RecoveryProgress {
                pending: 1,
                total: 2
            }
        );
    }

    #[test]
    fn get_recovery_position_renumbers_around_scrapped_candidates() {
        let store = CsbStore::new_for_test();
        let list_id = CandidateListId::new();
        let (first, scrapped, last) = (PersonId::new(), PersonId::new(), PersonId::new());
        store.add_candidate_list(CandidateList {
            id: list_id,
            candidates: vec![first, scrapped, last],
            electoral_districts: BTreeSet::from([ElectoralDistrict::Groningen]),
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

        assert_eq!(
            store.snapshot().get_recovery_position(list_id, first),
            Some(1)
        );
        // The scrapped candidate keeps its place in the order but loses its
        // number, so the candidate below moves up.
        assert_eq!(
            store.snapshot().get_recovery_position(list_id, scrapped),
            None
        );
        assert_eq!(
            store.snapshot().get_recovery_position(list_id, last),
            Some(2)
        );

        assert_eq!(
            store
                .snapshot()
                .get_recovery_position(list_id, PersonId::new()),
            None
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

        let result = store.snapshot().get_political_group_omissions();

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

        assert!(store.snapshot().get_political_group_omissions().is_empty());
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

        let data = store.snapshot();
        let result = data.get_candidate_omissions(person_a, None, store.election);

        assert_eq!(result.len(), 1);
        assert!(
            matches!(result[0].category, OmissionCategory::Candidate { person, .. } if person == person_a)
        );
    }

    #[test]
    fn get_candidate_omissions_filters_on_the_given_list() {
        let person = PersonId::new();
        let list_a = CandidateListId::new();
        let list_b = CandidateListId::new();
        let store = CsbStore::new_for_test();
        insert(
            &store,
            OmissionCategory::Candidate {
                person,
                lists: vec![list_a],
            },
        );
        insert(
            &store,
            OmissionCategory::Candidate {
                person,
                lists: vec![list_b],
            },
        );

        let data = store.snapshot();
        let result = data.get_candidate_omissions(person, Some(list_a), store.election);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].candidate_lists(), &[list_a]);
        assert_eq!(
            data.get_candidate_omissions(person, None, store.election)
                .len(),
            2
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

        assert!(
            store
                .snapshot()
                .get_candidate_omissions(PersonId::new(), None, store.election)
                .is_empty()
        );
    }

    #[test]
    fn get_candidate_list_omissions_returns_omissions_referencing_that_list() {
        let list_a = CandidateListId::new();
        let list_b = CandidateListId::new();
        let store = CsbStore::new_for_test();
        store.add_candidate_list(CandidateList {
            id: list_a,
            electoral_districts: BTreeSet::from([ElectoralDistrict::Groningen]),
            ..Default::default()
        });
        store.add_candidate_list(CandidateList {
            id: list_b,
            electoral_districts: BTreeSet::from([ElectoralDistrict::Drenthe]),
            ..Default::default()
        });
        insert(&store, OmissionCategory::CandidateList(vec![list_a]));
        insert(&store, OmissionCategory::CandidateList(vec![list_b]));
        insert(&store, OmissionCategory::PoliticalGroup);

        let data = store.snapshot();
        let result_a = data
            .get_candidate_list_omissions(list_a, store.election)
            .unwrap();
        let result_b = data
            .get_candidate_list_omissions(list_b, store.election)
            .unwrap();

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
        store.add_candidate_list(CandidateList {
            id: list_id,
            electoral_districts: BTreeSet::from([ElectoralDistrict::Utrecht]),
            ..Default::default()
        });
        store.set_paper_corrected_candidate_list(CandidateList {
            id: list_id,
            electoral_districts: BTreeSet::from([ElectoralDistrict::Groningen]),
            ..Default::default()
        });

        let data = store.snapshot();
        let list = data
            .pg(WithCorrections::None)
            .get_candidate_list(list_id)
            .unwrap();
        assert_eq!(
            list.electoral_districts,
            BTreeSet::from([ElectoralDistrict::Utrecht])
        );
        let list = data
            .pg(WithCorrections::Paper)
            .get_candidate_list(list_id)
            .unwrap();
        assert_eq!(
            list.electoral_districts,
            BTreeSet::from([ElectoralDistrict::Groningen])
        );
    }

    #[test]
    fn get_person_falls_back_to_a_paper_added_person() {
        let store = CsbStore::new_for_test();
        let person_id = PersonId::new();
        let person = sample_person_with(person_id, None, "Jansen", None, "A.B.");
        let mut data = store.write();
        Arc::make_mut(&mut data.paper_corrected_data)
            .persons
            .insert(person_id, person);
        data.refresh_derived();
        drop(data);

        assert!(
            store
                .snapshot()
                .pg(WithCorrections::None)
                .get_person(person_id)
                .is_err()
        );
        assert!(
            store
                .snapshot()
                .pg(WithCorrections::Paper)
                .get_person(person_id)
                .is_ok()
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
                .snapshot()
                .get_candidate_list_omissions(CandidateListId::new(), store.election)
                .is_err()
        );
    }

    #[test]
    fn get_substitute_submitters_returns_empty_when_none() {
        let store = CsbStore::new_for_test();

        assert!(
            store
                .snapshot()
                .pg(WithCorrections::All)
                .get_substitute_submitters()
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

        assert_eq!(
            store.snapshot().pg(WithCorrections::All).get_appellation(),
            "Kiesraad Demo"
        );
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
            store.snapshot().pg(WithCorrections::All).get_appellation(),
            "Blanco (Jansen, A.B.)"
        );
    }

    #[test]
    fn get_first_candidate_name_honours_scrappings() {
        let store = CsbStore::new_for_test();

        // create candidates
        let scrapped_person_id = PersonId::new();
        let scrapped_person = sample_person_with(scrapped_person_id, None, "Geschrapt", None, "C.");
        store.add_person(scrapped_person);
        let scrapped_list_person_id = PersonId::new();
        let scrapped_list_person =
            sample_person_with(scrapped_list_person_id, None, "Geschrapt", None, "L.");
        store.add_person(scrapped_list_person);
        let present_person_id = PersonId::new();
        let present_person = sample_person_with(present_person_id, None, "Present", None, "P.");
        store.add_person(present_person.clone());

        // create lists
        let scrapped_list_id = CandidateListId::new();
        let mut scrapped_list = sample_candidate_list(scrapped_list_id);
        scrapped_list.created_at = UtcDateTime::now();
        scrapped_list.candidates.push(scrapped_person_id);
        scrapped_list.candidates.push(scrapped_list_person_id);
        store.add_candidate_list(scrapped_list);
        let present_list_id = CandidateListId::new();
        let mut present_list = sample_candidate_list(present_list_id);
        present_list.created_at = UtcDateTime::now();
        present_list.candidates.push(scrapped_person_id);
        present_list.candidates.push(present_person_id);
        store.add_candidate_list(present_list);

        // do scrappings
        let scrapped = Scrapped::new_for_test(
            BTreeSet::new(),
            BTreeSet::from([
                (scrapped_list_id, scrapped_person_id),
                (present_list_id, scrapped_person_id),
            ]),
            BTreeMap::from([(scrapped_list_id, ScrappedList::whole_list())]),
        );

        assert!(scrapped.is_list_scrapped(scrapped_list_id));

        let data = store.snapshot();
        let name = data
            .get_first_candidate_name(WithCorrections::All, &scrapped)
            .unwrap();

        assert_eq!(*name, present_person.name);
    }

    #[test]
    fn csb_appellation_blank_list_without_candidates_uses_blanco_fallback() {
        let store = CsbStore::new_for_test();
        store.set_political_group(PoliticalGroup {
            list_designation: Some(ListDesignation::Blank),
            ..Default::default()
        });

        assert_eq!(
            store.snapshot().pg(WithCorrections::All).get_appellation(),
            "Blanco"
        );
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
        let mut data = store.write();
        data.csb_corrected_persons.insert(person_id, delta);
        data.refresh_derived();
        drop(data);

        let data = store.snapshot();
        let corrected = data.pg(WithCorrections::All).get_persons();
        assert_eq!(corrected.len(), 1);
        assert_eq!(corrected[0].name.last_name.to_string(), "Gecorrigeerd");

        // The imported data is untouched.
        let imported = data.pg(WithCorrections::None).get_persons();
        assert_eq!(imported[0].name.last_name.to_string(), "Jansen");
    }

    #[test]
    fn get_all_problems_skips_the_bsn_and_initials_of_candidates() {
        let store = CsbStore::new_for_test();
        let mut only_bsn = sample_person(PersonId::new());
        only_bsn.personal_data.bsn = None;
        let mut also_dob = sample_person(PersonId::new());
        also_dob.personal_data.bsn = None;
        also_dob.name.initials = None;
        also_dob.personal_data.date_of_birth = None;
        let mut list = sample_candidate_list(CandidateListId::new());
        list.candidates = vec![only_bsn.id, also_dob.id];
        store.add_person(only_bsn);
        store.add_person(also_dob.clone());
        store.add_candidate_list(list);

        let problems = store
            .snapshot()
            .get_all_problems(store.election)
            .expect("problems");

        assert_eq!(problems.candidates.len(), 1);
        assert_eq!(problems.candidates[0].entity.id, also_dob.id);
        assert_eq!(
            problems.candidates[0].problems,
            vec![PotentialProblems::NoDateOfBirth]
        );
    }
}
