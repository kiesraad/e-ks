//! Read accessors over an imported political group's CSB projection.
//!
//! The pure getters on [`CsbStoreData`] borrow from a snapshot; the getters on
//! [`CsbStream`] are the older per-call wrappers that clone out of one and are
//! being phased out.

use std::collections::HashMap;

use super::{CsbStoreData, Scrapped};
use crate::{
    AppError, ElectionConfig, ElectoralDistrict, Locale,
    structs::{
        brp::{BrpFinding, BrpStatus},
        candidate_lists::CandidateListId,
        common::{Appellation, FullName},
        csb::{
            Omission, OmissionCategory, OmissionId, OmissionTitle, PersonCorrectionDelta,
            RecoveryProgress,
        },
        list_designation::ListDesignation,
        persons::PersonId,
        problems::AllProblems,
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

impl CsbStoreData {
    pub fn is_examination_finished(&self) -> bool {
        self.is_examination_finished
    }

    pub fn is_deleted(&self) -> bool {
        self.is_deleted
    }

    pub fn has_paper_corrections(&self) -> bool {
        self.events.iter().any(|event| {
            matches!(
                event.payload.action,
                crate::CsbAction::PaperCorrectedUpdate(_)
            )
        })
    }

    pub fn omission(&self, omission_id: OmissionId) -> Option<&Omission> {
        self.omissions.get(&omission_id)
    }

    /// Every omission, in no particular order.
    pub fn omissions(&self) -> impl Iterator<Item = &Omission> {
        self.omissions.values()
    }

    pub fn omission_count(&self) -> usize {
        self.omissions.len()
    }

    /// The total number of CSB corrections added.
    pub fn correction_count(&self) -> usize {
        self.csb_corrected_persons
            .values()
            .map(|p| p.get_corrections().len())
            .sum::<usize>()
            + usize::from(self.csb_corrected_appellation.is_some())
    }

    /// The total number of CSB corrections and omissions.
    pub fn restoration_count(&self) -> usize {
        self.omission_count() + self.correction_count()
    }

    /// How far the group is through the "Herstelde lijsten" phase, counted in
    /// recovery decisions (see [`RecoveryProgress`]).
    pub fn recovery_progress(&self, election: ElectionConfig) -> RecoveryProgress {
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
    pub fn scrapped(&self) -> &Scrapped {
        &self.scrapped
    }

    /// The districts scrapped by unresolved declarations-of-support omissions,
    /// in the election's district order.
    pub fn scrapped_districts(&self, election: ElectionConfig) -> Vec<ElectoralDistrict> {
        self.scrapped.districts(&election)
    }

    /// The candidate's number in the recovery ("Herstelde lijsten") phase.
    /// A scrapped candidate keeps its place in the order but loses its number,
    /// so the candidates below it move up: the numbering runs over the
    /// candidates that are not scrapped. `None` for a scrapped candidate, and
    /// for a candidate that is not on the list.
    pub fn recovery_position(
        &self,
        list_id: CandidateListId,
        person_id: PersonId,
    ) -> Option<usize> {
        let candidates = &self
            .view(WithCorrections::All)
            .candidate_list(list_id)?
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

    pub fn political_group_omissions(&self) -> Vec<&Omission> {
        self.omissions
            .values()
            .filter(|o| matches!(o.category, OmissionCategory::PoliticalGroup))
            .collect()
    }

    pub fn appellation_omissions(&self) -> Vec<&Omission> {
        self.omissions
            .values()
            .filter(|o| matches!(o.category, OmissionCategory::Appellation))
            .collect()
    }

    pub fn political_group_csb_corrections_count(&self) -> usize {
        usize::from(self.csb_corrected_appellation.is_some())
    }

    /// The candidate's omissions, in title order.
    pub fn candidate_omissions(
        &self,
        election: ElectionConfig,
        person_id: PersonId,
    ) -> Vec<&Omission> {
        let mut omissions: Vec<&Omission> = self
            .omissions
            .values()
            .filter(|o| matches!(&o.category, OmissionCategory::Candidate { person, .. } if *person == person_id))
            .collect();

        omissions.sort_by_cached_key(|omission| self.title_order(election, omission));
        omissions
    }

    /// Whether a candidate has omissions for a specific list.
    pub fn has_candidate_omissions(&self, person_id: PersonId, list_id: CandidateListId) -> bool {
        self.omissions.values().any(|o| {
            matches!(&o.category, OmissionCategory::Candidate { person, lists }
                if *person == person_id && lists.contains(&list_id))
        })
    }

    /// Whether a candidate has CSB corrections.
    pub fn has_candidate_csb_corrections(&self, person_id: PersonId) -> bool {
        self.csb_corrected_persons.contains_key(&person_id)
    }

    /// The candidate-list omissions that reference the given list, in title
    /// order; `None` when the list is unknown.
    pub fn candidate_list_omissions(
        &self,
        election: ElectionConfig,
        list_id: CandidateListId,
    ) -> Option<Vec<&Omission>> {
        self.view(WithCorrections::All).candidate_list(list_id)?;

        let mut omissions: Vec<&Omission> = self
            .omissions
            .values()
            .filter(|o| {
                matches!(&o.category, OmissionCategory::CandidateList(lists)
                    if lists.contains(&list_id))
            })
            .collect();

        omissions.sort_by_cached_key(|omission| self.title_order(election, omission));
        Some(omissions)
    }

    /// Whether the candidate list or any of its candidates has omissions;
    /// `None` when the list is unknown.
    pub fn has_candidate_list_omissions(&self, list_id: CandidateListId) -> Option<bool> {
        let list = self.view(WithCorrections::All).candidate_list(list_id)?;

        Some(self.omissions.values().any(|o| match &o.category {
            OmissionCategory::CandidateList(lists) => lists.contains(&list_id),
            OmissionCategory::Candidate { person, lists } => {
                lists.contains(&list_id) && list.candidates.contains(person)
            }
            _ => false,
        }))
    }

    /// Whether the candidate list has any candidates with CSB corrections;
    /// `None` when the list is unknown.
    pub fn has_candidate_list_csb_corrections(&self, list_id: CandidateListId) -> Option<bool> {
        let list = self.view(WithCorrections::All).candidate_list(list_id)?;

        Some(
            list.candidates
                .iter()
                .any(|candidate| self.csb_corrected_persons.contains_key(candidate)),
        )
    }

    /// In title order.
    pub fn declarations_of_support_omissions(&self, election: ElectionConfig) -> Vec<&Omission> {
        let mut omissions: Vec<&Omission> = self
            .omissions
            .values()
            .filter(|o| matches!(o.category, OmissionCategory::DeclarationsOfSupport(_)))
            .collect();

        omissions.sort_by_cached_key(|omission| self.title_order(election, omission));
        omissions
    }

    /// Sort key putting omissions in title order, then district order, so the
    /// parts of a split stay together.
    pub(crate) fn title_order(
        &self,
        election: ElectionConfig,
        omission: &Omission,
    ) -> (OmissionTitle, usize) {
        let order = election.electoral_districts();
        let first_district = self
            .omission_districts(election, omission)
            .first()
            .and_then(|district| order.iter().position(|d| d == district))
            .unwrap_or(usize::MAX);

        (omission.title.to_owned(), first_district)
    }

    /// The districts an omission touches, directly or through its lists.
    fn omission_districts(
        &self,
        election: ElectionConfig,
        omission: &Omission,
    ) -> Vec<ElectoralDistrict> {
        let districts = omission.electoral_districts(&election);
        if !districts.is_empty() {
            return districts.to_vec();
        }

        let corrected = self.view(WithCorrections::All);
        omission
            .candidate_lists()
            .iter()
            .filter_map(|list_id| corrected.candidate_list(*list_id))
            .flat_map(|list| list.electoral_districts.iter().copied())
            .collect()
    }

    /// The CSB corrections recorded on a person.
    pub fn person_corrections(&self, person_id: PersonId) -> Option<&PersonCorrectionDelta> {
        self.csb_corrected_persons.get(&person_id)
    }

    /// The persons with CSB corrections, in no particular order.
    pub fn csb_corrected_persons(&self) -> impl Iterator<Item = PersonId> + '_ {
        self.csb_corrected_persons.keys().copied()
    }

    pub fn corrected_appellation(&self) -> Option<&Appellation> {
        self.csb_corrected_appellation.as_ref()
    }

    /// The name of the first candidate across all candidate lists, oldest
    /// list first. With a [`Scrapped`] projection, the first unscrapped
    /// candidate on the first unscrapped list.
    pub fn first_candidate_name(
        &self,
        corrections: WithCorrections,
        scrapped: Option<&Scrapped>,
    ) -> Option<&FullName> {
        self.view(corrections)
            .first_candidate_where(|list, person| {
                scrapped.is_none_or(|scrapped| {
                    !scrapped.is_list_scrapped(list.id)
                        && !scrapped.is_candidate_scrapped(list.id, person)
                })
            })
            .map(|person| &person.name)
    }

    /// The appellation of the political group, including the special names
    /// for blank lists.
    pub fn appellation(&self, corrections: WithCorrections) -> String {
        self.view(corrections)
            .political_group()
            .csb_appellation(self.first_candidate_name(corrections, None))
    }

    pub fn appellation_with_scrapped(
        &self,
        corrections: WithCorrections,
        scrapped: &Scrapped,
    ) -> String {
        let mut political_group = self.view(corrections).political_group().clone();
        if scrapped.is_appellation_scrapped() {
            political_group.list_designation = Some(ListDesignation::Blank);
        }
        political_group.csb_appellation(self.first_candidate_name(corrections, Some(scrapped)))
    }

    /// [`Self::appellation`], with a deleted label when the political group
    /// has been deleted.
    pub fn appellation_with_deleted_label(
        &self,
        corrections: WithCorrections,
        locale: Locale,
    ) -> String {
        let appellation = self.appellation(corrections);
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
    pub fn brp_findings(&self) -> &HashMap<PersonId, Vec<BrpFinding>> {
        &self.brp_findings
    }

    /// An empty slice covers both "checked, nothing found" and "not checked";
    /// use [`Self::is_brp_checked`] when the difference matters.
    pub fn brp_findings_for_person(&self, person_id: PersonId) -> &[BrpFinding] {
        self.brp_findings.get(&person_id).map_or(&[], Vec::as_slice)
    }

    /// Whether this candidate has been checked, findings or not.
    pub fn is_brp_checked(&self, person_id: PersonId) -> bool {
        self.brp_findings.contains_key(&person_id)
    }

    /// How far the BRP sweep for this stream got.
    pub fn brp_status(&self) -> &BrpStatus {
        &self.brp_validation_status
    }

    /// The single stored omission. Test-only helper for asserting on
    /// omissions whose category has no dedicated getter.
    #[cfg(test)]
    pub fn omission_for_test(&self) -> &Omission {
        self.omissions
            .values()
            .next()
            .expect("expected exactly one stored omission")
    }

    /// All problems of the fully corrected data, excluding info problems.
    pub fn all_problems(&self, election: ElectionConfig) -> Result<AllProblems, AppError> {
        AllProblems::find_all(self.view(WithCorrections::All), election).map(|mut problems| {
            problems.info_problems = Vec::new();
            problems
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use crate::{
        CsbAction, CsbUser, ElectoralDistrict,
        projection::csb_data::scrapped::ScrappedList,
        store::{StoreData, StoreEvent},
        structs::{
            candidate_lists::CandidateList,
            common::UtcDateTime,
            csb::{
                Correction, OmissionCategory, OmissionStatus, PersonCorrection, sample_omission,
            },
            list_designation::ListDesignation,
            persons::Person,
            political_groups::PoliticalGroup,
        },
        test_utils::{sample_candidate_list, sample_person, sample_person_with},
    };

    const ELECTION: ElectionConfig = ElectionConfig::EK27;

    fn insert(data: &mut CsbStoreData, category: OmissionCategory) {
        let omission = sample_omission(category);
        data.omissions.insert(omission.id, omission);
        data.refresh_derived();
    }

    fn insert_with_status(
        data: &mut CsbStoreData,
        category: OmissionCategory,
        recoverable: bool,
        status: OmissionStatus,
    ) {
        let mut omission = sample_omission(category);
        omission.recoverable = recoverable;
        omission.status = status;
        data.omissions.insert(omission.id, omission);
        data.refresh_derived();
    }

    #[test]
    fn recovery_progress_skips_irreparable_omissions() {
        let mut data = CsbStoreData::default();
        insert(&mut data, OmissionCategory::PoliticalGroup);
        insert_with_status(
            &mut data,
            OmissionCategory::PoliticalGroup,
            true,
            OmissionStatus::Recovered,
        );
        insert_with_status(
            &mut data,
            OmissionCategory::PoliticalGroup,
            false,
            OmissionStatus::Pending,
        );

        // The irreparable omission needs no decision and is not actionable.
        assert_eq!(
            data.recovery_progress(ELECTION),
            RecoveryProgress {
                pending: 1,
                total: 2
            }
        );
    }

    #[test]
    fn recovery_position_renumbers_around_scrapped_candidates() {
        let mut data = CsbStoreData::default();
        let list_id = CandidateListId::new();
        let (first, scrapped, last) = (PersonId::new(), PersonId::new(), PersonId::new());
        data.add_candidate_list(CandidateList {
            id: list_id,
            candidates: vec![first, scrapped, last],
            electoral_districts: BTreeSet::from([ElectoralDistrict::Groningen]),
            ..Default::default()
        });

        insert_with_status(
            &mut data,
            OmissionCategory::Candidate {
                person: scrapped,
                lists: vec![list_id],
            },
            true,
            OmissionStatus::NotRecovered,
        );

        assert_eq!(data.recovery_position(list_id, first), Some(1));
        // The scrapped candidate keeps its place in the order but loses its
        // number, so the candidate below moves up.
        assert_eq!(data.recovery_position(list_id, scrapped), None);
        assert_eq!(data.recovery_position(list_id, last), Some(2));

        assert_eq!(data.recovery_position(list_id, PersonId::new()), None);
    }

    #[test]
    fn political_group_omissions_returns_only_political_group() {
        let mut data = CsbStoreData::default();
        insert(&mut data, OmissionCategory::PoliticalGroup);
        insert(
            &mut data,
            OmissionCategory::CandidateList(vec![CandidateListId::new()]),
        );

        let result = data.political_group_omissions();

        assert_eq!(result.len(), 1);
        assert!(matches!(
            result[0].category,
            OmissionCategory::PoliticalGroup
        ));
    }

    #[test]
    fn political_group_omissions_returns_empty_when_none() {
        let mut data = CsbStoreData::default();
        insert(
            &mut data,
            OmissionCategory::CandidateList(vec![CandidateListId::new()]),
        );

        assert!(data.political_group_omissions().is_empty());
    }

    #[test]
    fn candidate_omissions_returns_only_omissions_for_the_given_person() {
        let person_a = PersonId::new();
        let person_b = PersonId::new();
        let mut data = CsbStoreData::default();
        insert(
            &mut data,
            OmissionCategory::Candidate {
                person: person_a,
                lists: Vec::new(),
            },
        );
        insert(
            &mut data,
            OmissionCategory::Candidate {
                person: person_b,
                lists: Vec::new(),
            },
        );
        insert(&mut data, OmissionCategory::PoliticalGroup);

        let result = data.candidate_omissions(ELECTION, person_a);

        assert_eq!(result.len(), 1);
        assert!(
            matches!(result[0].category, OmissionCategory::Candidate { person, .. } if person == person_a)
        );
    }

    #[test]
    fn candidate_omissions_returns_empty_when_no_match() {
        let mut data = CsbStoreData::default();
        insert(
            &mut data,
            OmissionCategory::Candidate {
                person: PersonId::new(),
                lists: Vec::new(),
            },
        );

        assert!(
            data.candidate_omissions(ELECTION, PersonId::new())
                .is_empty()
        );
    }

    #[test]
    fn candidate_list_omissions_returns_omissions_referencing_that_list() {
        let list_a = CandidateListId::new();
        let list_b = CandidateListId::new();
        let mut data = CsbStoreData::default();
        data.add_candidate_list(CandidateList {
            id: list_a,
            electoral_districts: BTreeSet::from([ElectoralDistrict::Groningen]),
            ..Default::default()
        });
        data.add_candidate_list(CandidateList {
            id: list_b,
            electoral_districts: BTreeSet::from([ElectoralDistrict::Drenthe]),
            ..Default::default()
        });
        insert(&mut data, OmissionCategory::CandidateList(vec![list_a]));
        insert(&mut data, OmissionCategory::CandidateList(vec![list_b]));
        insert(&mut data, OmissionCategory::PoliticalGroup);

        let result_a = data.candidate_list_omissions(ELECTION, list_a).unwrap();
        let result_b = data.candidate_list_omissions(ELECTION, list_b).unwrap();

        assert_eq!(result_a.len(), 1);
        assert!(
            matches!(&result_a[0].category, OmissionCategory::CandidateList(ids) if ids == &[list_a])
        );
        assert_eq!(result_b.len(), 1);
        assert!(
            matches!(&result_b[0].category, OmissionCategory::CandidateList(ids) if ids == &[list_b])
        );
    }

    /// A list has omissions through its own omissions and through those of
    /// the candidates on it, for that list.
    #[test]
    fn has_candidate_list_omissions_counts_the_candidates_on_it() {
        let (list_a, list_b) = (CandidateListId::new(), CandidateListId::new());
        let person = PersonId::new();
        let mut data = CsbStoreData::default();
        data.add_candidate_list(CandidateList {
            id: list_a,
            candidates: vec![person],
            ..Default::default()
        });
        data.add_candidate_list(CandidateList {
            id: list_b,
            ..Default::default()
        });
        assert_eq!(data.has_candidate_list_omissions(list_a), Some(false));

        insert(
            &mut data,
            OmissionCategory::Candidate {
                person,
                lists: vec![list_a],
            },
        );
        assert_eq!(data.has_candidate_list_omissions(list_a), Some(true));
        assert_eq!(data.has_candidate_list_omissions(list_b), Some(false));
        assert_eq!(
            data.has_candidate_list_omissions(CandidateListId::new()),
            None
        );
    }

    #[test]
    fn candidate_list_prefers_the_paper_corrected_version() {
        let list_id = CandidateListId::new();
        let mut data = CsbStoreData::default();
        data.add_candidate_list(CandidateList {
            id: list_id,
            electoral_districts: BTreeSet::from([ElectoralDistrict::Utrecht]),
            ..Default::default()
        });
        data.set_paper_corrected_candidate_list(CandidateList {
            id: list_id,
            electoral_districts: BTreeSet::from([ElectoralDistrict::Groningen]),
            ..Default::default()
        });

        let list = data
            .view(WithCorrections::None)
            .candidate_list(list_id)
            .unwrap();
        assert_eq!(
            list.electoral_districts,
            BTreeSet::from([ElectoralDistrict::Utrecht])
        );
        let list = data
            .view(WithCorrections::Paper)
            .candidate_list(list_id)
            .unwrap();
        assert_eq!(
            list.electoral_districts,
            BTreeSet::from([ElectoralDistrict::Groningen])
        );
    }

    #[test]
    fn person_falls_back_to_a_paper_added_person() {
        let mut data = CsbStoreData::default();
        let person_id = PersonId::new();
        let person = sample_person_with(person_id, None, "Jansen", None, "A.B.");
        data.paper_corrected_mut().persons.insert(person_id, person);
        data.refresh_derived();

        assert!(data.view(WithCorrections::None).person(person_id).is_none());
        assert!(
            data.view(WithCorrections::Paper)
                .person(person_id)
                .is_some()
        );
    }

    #[test]
    fn candidate_list_omissions_is_none_for_unknown_list() {
        let mut data = CsbStoreData::default();
        insert(
            &mut data,
            OmissionCategory::CandidateList(vec![CandidateListId::new()]),
        );

        assert!(
            data.candidate_list_omissions(ELECTION, CandidateListId::new())
                .is_none()
        );
    }

    #[test]
    fn csb_appellation_standalone_list_uses_appellation() {
        let mut data = CsbStoreData::default();
        data.set_political_group(PoliticalGroup {
            appellation: Some("Kiesraad Demo".parse().unwrap()),
            list_designation: Some(ListDesignation::Standalone),
            ..Default::default()
        });

        assert_eq!(data.appellation(WithCorrections::All), "Kiesraad Demo");
    }

    #[test]
    fn csb_appellation_blank_list_with_candidate_uses_first_candidate_name() {
        let mut data = CsbStoreData::default();
        data.set_political_group(PoliticalGroup {
            list_designation: Some(ListDesignation::Blank),
            ..Default::default()
        });

        let person_id = PersonId::new();
        let person = sample_person_with(person_id, None, "Jansen", None, "A.B.");
        data.add_person(person);

        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates.push(person_id);
        data.add_candidate_list(list);

        assert_eq!(
            data.appellation(WithCorrections::All),
            "Blanco (Jansen, A.B.)"
        );
    }

    #[test]
    fn first_candidate_name_honours_scrappings() {
        let mut data = CsbStoreData::default();

        // create candidates
        let scrapped_person_id = PersonId::new();
        let scrapped_person = sample_person_with(scrapped_person_id, None, "Geschrapt", None, "C.");
        data.add_person(scrapped_person);
        let scrapped_list_person_id = PersonId::new();
        let scrapped_list_person =
            sample_person_with(scrapped_list_person_id, None, "Geschrapt", None, "L.");
        data.add_person(scrapped_list_person);
        let present_person_id = PersonId::new();
        let present_person = sample_person_with(present_person_id, None, "Present", None, "P.");
        data.add_person(present_person.clone());

        // create lists
        let scrapped_list_id = CandidateListId::new();
        let mut scrapped_list = sample_candidate_list(scrapped_list_id);
        scrapped_list.created_at = UtcDateTime::now();
        scrapped_list.candidates.push(scrapped_person_id);
        scrapped_list.candidates.push(scrapped_list_person_id);
        data.add_candidate_list(scrapped_list);
        let present_list_id = CandidateListId::new();
        let mut present_list = sample_candidate_list(present_list_id);
        present_list.created_at = UtcDateTime::now();
        present_list.candidates.push(scrapped_person_id);
        present_list.candidates.push(present_person_id);
        data.add_candidate_list(present_list);

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

        let name = data
            .first_candidate_name(WithCorrections::All, Some(&scrapped))
            .unwrap();

        assert_eq!(*name, present_person.name);
    }

    #[test]
    fn csb_appellation_blank_list_without_candidates_uses_blanco_fallback() {
        let mut data = CsbStoreData::default();
        data.set_political_group(PoliticalGroup {
            list_designation: Some(ListDesignation::Blank),
            ..Default::default()
        });

        assert_eq!(data.appellation(WithCorrections::All), "Blanco");
    }

    /// An ambtshalve correction is folded into the corrected projection; the
    /// imported and paper data are untouched.
    #[test]
    fn persons_applies_the_committees_own_corrections() {
        let mut data = CsbStoreData::default();
        let person = sample_person(PersonId::new());
        let person_id = person.id;
        data.add_person(person);

        data.apply(StoreEvent::new(
            1,
            CsbAction::UpdateCorrection(Correction::Person(
                person_id,
                PersonCorrection::LastName("Gecorrigeerd".parse().unwrap()),
            ))
            .by(CsbUser::new_test()),
        ));

        let corrected: Vec<&Person> = data.view(WithCorrections::All).persons().collect();
        assert_eq!(corrected.len(), 1);
        assert_eq!(corrected[0].name.last_name.to_string(), "Gecorrigeerd");
        assert!(data.has_candidate_csb_corrections(person_id));
        assert_eq!(data.correction_count(), 1);

        let imported: Vec<&Person> = data.view(WithCorrections::None).persons().collect();
        assert_eq!(imported[0].name.last_name.to_string(), "Jansen");
    }
}
