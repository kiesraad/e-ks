mod event;
mod getters;
mod scrapped;

pub use event::{CsbAction, CsbEvent};
pub use getters::WithCorrections;
pub use scrapped::Scrapped;

use std::collections::{HashMap, hash_map::Entry};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(test)]
use crate::structs::political_groups::PoliticalGroup;
use crate::{
    ElectoralDistrict, PgEvent, PgStoreData, Scope,
    store::{StoreData, StoreEvent},
    structs::{
        brp::{BrpFinding, BrpStatus},
        common::{Appellation, UtcDateTime},
        csb::{
            Correction, Omission, OmissionCategory, OmissionDecision, OmissionId, OmissionPart,
            OmissionStatus, PersonCorrection, PersonCorrectionDelta,
        },
        persons::PersonId,
    },
};

/// Event-sourced domain projection for a single (stream, election) pair on the
/// CSB side.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CsbStoreData {
    pub(crate) imported_data: PgStoreData,
    pub(crate) paper_corrected_data: PgStoreData,
    pub(crate) events: Vec<StoreEvent<CsbEvent>>,
    pub(crate) is_examination_finished: bool,
    pub(crate) is_deleted: bool,
    pub(crate) omissions: HashMap<OmissionId, Omission>,
    /// A checked candidate is present even with no findings, which keeps a
    /// repeated sweep from checking them again.
    pub(crate) brp_findings: HashMap<PersonId, Vec<BrpFinding>>,
    pub(crate) brp_validation_status: BrpStatus,
    pub(crate) csb_corrected_persons: HashMap<PersonId, PersonCorrectionDelta>,
    pub(crate) csb_corrected_appellation: Option<Appellation>,
    /// What the unresolved omissions scrap; derived again after every event.
    pub(crate) scrapped: Scrapped,
}

impl StoreData for CsbStoreData {
    type Event = CsbEvent;

    fn apply(&mut self, event: StoreEvent<CsbEvent>) {
        self.events.push(event.clone());

        let event_time = UtcDateTime::from(event.created_at);
        let StoreEvent {
            event_id,
            created_at,
            hash,
            ..
        } = event;

        match event.payload.action {
            CsbAction::Import {
                snapshot,
                hash: source_hash,
                ..
            } => self.apply_import(
                *snapshot,
                StoreEvent {
                    event_id,
                    payload: crate::PgEvent::Import { hash: source_hash },
                    created_at,
                    hash,
                },
            ),
            CsbAction::CreateEmpty => {}
            CsbAction::Delete => self.is_deleted = true,
            CsbAction::PaperCorrectedUpdate(payload) => self.apply_paper_correction(StoreEvent {
                event_id,
                payload: *payload,
                created_at,
                hash,
            }),
            CsbAction::SetFinished(value) => self.is_examination_finished = value,
            CsbAction::CreateOmission(omission) => self.create_omission(omission, event_time),
            CsbAction::UpdateOmission(omission) => self.update_omission(omission, event_time),
            CsbAction::DeleteOmission { omission_id } => {
                self.omissions.remove(&omission_id);
            }
            CsbAction::SetOmissionStatus {
                omission_id,
                status,
            } => self.set_omission_status(omission_id, status, event_time),
            CsbAction::SetOmissionPartStatus {
                omission_id,
                part,
                status,
            } => self.set_omission_part_status(omission_id, part, status, event_id, event_time),
            CsbAction::UpdateCorrection(correction) => {
                if let Correction::Person(person_id, _) = &correction {
                    self.forget_brp_check(*person_id);
                }
                self.apply_correction(correction)
            }
            CsbAction::BrpPersonChecked { person, findings } => {
                self.brp_findings.insert(person, findings);
            }
            CsbAction::SetBrpStatus(value) => self.brp_validation_status = value,
        }

        self.refresh_scrapped();
    }

    fn events(&self) -> &[StoreEvent<Self::Event>] {
        &self.events
    }

    fn scope() -> Scope {
        Scope::ImportedByCsb
    }
}

impl CsbStoreData {
    /// Derive what is scrapped from the current omissions and corrected data.
    pub(crate) fn refresh_scrapped(&mut self) {
        self.scrapped = Scrapped::derive(&self.paper_corrected_data, &self.omissions);
    }

    /// Take over an imported package as both projections. `import` becomes
    /// event #1 of the corrected one, starting its audit log.
    fn apply_import(&mut self, snapshot: PgStoreData, import: StoreEvent<crate::PgEvent>) {
        self.imported_data = snapshot;
        self.paper_corrected_data = self.imported_data.clone();
        self.paper_corrected_data.apply(import);
    }

    /// Forget what the BRP said about this candidate. Their data changed, so
    /// the findings are about values that are no longer on screen; dropping
    /// them puts the candidate back to "not checked" and lets a new check pick
    /// them up.
    ///
    /// The sweep status is deliberately left alone: most candidates were still
    /// checked, and [`crate::structs::brp::BrpStatus`] records how far the
    /// sweep got, not whether its outcome still holds.
    fn forget_brp_check(&mut self, person_id: PersonId) {
        self.brp_findings.remove(&person_id);
    }

    fn create_omission(&mut self, mut omission: Omission, event_time: UtcDateTime) {
        omission.updated_at = event_time;
        self.omissions.insert(omission.id, omission);
    }

    fn update_omission(&mut self, mut omission: Omission, event_time: UtcDateTime) {
        omission.updated_at = event_time;
        let omission_id = omission.id;
        self.omissions.entry(omission_id).and_modify(|existing| {
            *existing = omission;
        });
    }

    /// Record a decision on the omission, then read it as one with the
    /// omission that has the same details and holds the same decision.
    fn set_omission_status(
        &mut self,
        omission_id: OmissionId,
        status: OmissionStatus,
        event_time: UtcDateTime,
    ) {
        self.omissions.entry(omission_id).and_modify(|omission| {
            omission.status = status;
            omission.updated_at = event_time;
        });
        self.merge_omission(omission_id, event_time);
    }

    /// Record a decision on one part of the omission. While the omission
    /// covers other parts, the part is split off first: a copy under an id
    /// derived from the event, so replaying the stream is deterministic.
    fn set_omission_part_status(
        &mut self,
        omission_id: OmissionId,
        part: OmissionPart,
        status: OmissionStatus,
        event_id: usize,
        event_time: UtcDateTime,
    ) {
        let Some(omission) = self.omissions.get(&omission_id).cloned() else {
            return;
        };

        let decided = match omission.category.decide(part) {
            None => return,
            Some(OmissionDecision::Whole) => omission_id,
            Some(OmissionDecision::Split { remaining, split }) => {
                let split_id = split_off_id(omission_id, event_id);
                self.omissions.insert(
                    split_id,
                    Omission {
                        id: split_id,
                        category: split,
                        updated_at: event_time,
                        ..omission
                    },
                );
                self.omissions.entry(omission_id).and_modify(|omission| {
                    omission.category = remaining;
                    omission.updated_at = event_time;
                });
                split_id
            }
        };

        self.set_omission_status(decided, status, event_time);
    }

    /// Read the omission as one with the omission that has the same details
    /// and holds the same decision, if any: that one takes its parts over and
    /// the omission goes away. Pending omissions stay as they were reported.
    fn merge_omission(&mut self, omission_id: OmissionId, event_time: UtcDateTime) {
        let Some(omission) = self.omissions.get(&omission_id) else {
            return;
        };
        if omission.status == OmissionStatus::Pending {
            return;
        }
        let Some(holder) = self
            .omissions
            .values()
            .filter(|o| {
                o.id != omission_id && o.status == omission.status && o.has_same_details(omission)
            })
            .min_by_key(|o| o.id)
        else {
            return;
        };
        let Some(mut category) = holder.category.merged_with(&omission.category) else {
            return;
        };
        let holder_id = holder.id;

        self.sort_candidate_lists(&mut category);
        self.omissions.remove(&omission_id);
        self.omissions.entry(holder_id).and_modify(|holder| {
            holder.category = category;
            holder.updated_at = event_time;
        });
    }

    /// Put the lists of a category in district order, by their first
    /// district, the way omissions are read.
    fn sort_candidate_lists(&self, category: &mut OmissionCategory) {
        if let OmissionCategory::CandidateList(lists) | OmissionCategory::Candidate { lists, .. } =
            category
        {
            lists.sort_by_key(|list_id| {
                self.paper_corrected_data
                    .candidate_lists
                    .get(list_id)
                    .and_then(|list| list.electoral_districts.first())
                    .map_or(u16::MAX, ElectoralDistrict::region_number)
            });
        }
    }

    /// Replay an app event onto the corrected projection, keeping the CSB
    /// stream's own event metadata.
    fn apply_paper_correction(&mut self, event: StoreEvent<PgEvent>) {
        for person_id in candidates_changed_by(&event.payload) {
            self.forget_brp_check(person_id);
        }

        self.paper_corrected_data.apply(event);
    }

    /// Record a CSB correction. Correcting a value back to the one already in
    /// the paper-corrected projection undoes the correction instead.
    fn apply_correction(&mut self, correction: Correction) {
        match correction {
            Correction::Appellation(appellation) => {
                let appellation = Some(appellation);

                self.csb_corrected_appellation =
                    if self.paper_corrected_data.political_group.appellation == appellation {
                        None
                    } else {
                        appellation
                    };
            }
            Correction::Person(person_id, correction) => {
                self.apply_person_correction(person_id, correction)
            }
        }
    }

    fn apply_person_correction(&mut self, person_id: PersonId, correction: PersonCorrection) {
        // A correction on an unknown person is dropped: there is nothing to
        // correct it against.
        let changes = self
            .paper_corrected_data
            .persons
            .get(&person_id)
            .is_some_and(|person| correction.changes(person));

        if changes {
            self.csb_corrected_persons
                .entry(person_id)
                .or_default()
                .add_correction(correction);
        } else if let Entry::Occupied(mut entry) = self.csb_corrected_persons.entry(person_id) {
            entry.get_mut().remove_correction(&correction);
            if entry.get().get_corrections().is_empty() {
                entry.remove_entry();
            }
        }
    }
}

/// The id of the part split off `omission_id` by event `event_id`: the same on
/// every replay of the stream.
fn split_off_id(omission_id: OmissionId, event_id: usize) -> OmissionId {
    Uuid::new_v5(&Uuid::from(omission_id), &event_id.to_be_bytes()).into()
}

/// The candidates whose BRP-checked data an app event changes.
///
/// The correspondence address and the representative are absent on purpose:
/// the BRP check does not compare them, so changing one leaves its findings
/// standing.
fn candidates_changed_by(event: &PgEvent) -> Vec<PersonId> {
    match event {
        PgEvent::CreatePerson(person) | PgEvent::UpdatePerson(person) => vec![person.id],
        PgEvent::CreatePersonPersonalData { person_id, .. }
        | PgEvent::UpdatePersonPersonalData { person_id, .. }
        | PgEvent::DeletePerson { person_id } => vec![*person_id],
        PgEvent::ImportCandidates {
            created_persons,
            updated_persons,
            ..
        } => created_persons
            .iter()
            .chain(updated_persons)
            .map(|person| person.id)
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
impl crate::CsbStream {
    pub fn new_for_test() -> Self {
        use crate::StreamId;

        crate::store::Store {
            stream_id: StreamId::new(),
            election: crate::ElectionConfig::EK27,
            backend: crate::store::StoreBackend::Memory {
                store: crate::store::memory::MemoryStore::default(),
            },
            data: std::sync::Arc::new(parking_lot::RwLock::new(CsbStoreData::default())),
        }
    }

    /// Test setters write both projections, mirroring the state right after an
    /// import (which seeds `paper_corrected_data` from the imported snapshot).
    pub fn set_political_group(&self, political_group: PoliticalGroup) {
        let mut data = self.data.write();
        data.imported_data.political_group = political_group.clone();
        data.paper_corrected_data.political_group = political_group;
        data.refresh_scrapped();
    }

    pub fn add_candidate_list(&self, list: crate::structs::candidate_lists::CandidateList) {
        let mut data = self.data.write();
        data.imported_data
            .candidate_lists
            .insert(list.id, list.clone());
        data.paper_corrected_data
            .candidate_lists
            .insert(list.id, list);
        data.refresh_scrapped();
    }

    /// Test setter writing only the corrected projection, mirroring a list
    /// added during paper corrections.
    pub fn set_paper_corrected_candidate_list(
        &self,
        list: crate::structs::candidate_lists::CandidateList,
    ) {
        let mut data = self.data.write();
        data.paper_corrected_data
            .candidate_lists
            .insert(list.id, list);
        data.refresh_scrapped();
    }

    pub fn add_person(&self, person: crate::structs::persons::Person) {
        let mut data = self.data.write();
        data.imported_data.persons.insert(person.id, person.clone());
        data.paper_corrected_data.persons.insert(person.id, person);
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, str::FromStr};

    use super::*;
    use crate::{
        CsbUser, PgEvent, StreamId,
        structs::{
            common::{Initials, LastName, PlaceOfResidence},
            csb::PersonCorrection,
            persons::Person,
        },
        test_utils::{sample_person, sample_political_group},
    };

    fn import_event() -> CsbEvent {
        CsbAction::Import {
            hash: [42; 32],
            source_stream_id: StreamId::new(),
            snapshot: Box::new(PgStoreData {
                political_group: sample_political_group(),
                ..PgStoreData::default()
            }),
        }
        .by(CsbUser::new_test())
    }

    fn import_event_with_person(person: Person) -> CsbEvent {
        let mut snapshot = PgStoreData::default();
        snapshot.persons.insert(person.id, person);
        CsbAction::Import {
            hash: [42; 32],
            source_stream_id: StreamId::new(),
            snapshot: Box::new(snapshot),
        }
        .by(CsbUser::new_test())
    }

    fn declarations_of_support(districts: Vec<ElectoralDistrict>) -> Omission {
        Omission::new(
            OmissionCategory::DeclarationsOfSupport(districts),
            "Declarations of support missing".parse().unwrap(),
            "Too few declarations of support were handed in."
                .parse()
                .unwrap(),
            None,
        )
    }

    fn replay(events: &[CsbAction]) -> CsbStoreData {
        let mut data = CsbStoreData::default();
        for (index, action) in events.iter().enumerate() {
            data.apply(StoreEvent::new(
                index + 1,
                action.clone().by(CsbUser::new_test()),
            ));
        }
        data
    }

    #[test]
    fn a_part_decision_splits_the_part_off_under_an_id_derived_from_the_event() {
        let omission = declarations_of_support(vec![
            ElectoralDistrict::Groningen,
            ElectoralDistrict::Fryslan,
        ]);
        let events = [
            CsbAction::CreateOmission(omission.clone()),
            CsbAction::SetOmissionPartStatus {
                omission_id: omission.id,
                part: OmissionPart::ElectoralDistrict(ElectoralDistrict::Groningen),
                status: OmissionStatus::Recovered,
            },
        ];

        let data = replay(&events);
        assert_eq!(data.omissions.len(), 2);
        let remaining = &data.omissions[&omission.id];
        assert_eq!(
            remaining.category,
            OmissionCategory::DeclarationsOfSupport(vec![ElectoralDistrict::Fryslan])
        );
        assert_eq!(remaining.status, OmissionStatus::Pending);
        let split = &data.omissions[&split_off_id(omission.id, 2)];
        assert_eq!(
            split.category,
            OmissionCategory::DeclarationsOfSupport(vec![ElectoralDistrict::Groningen])
        );
        assert_eq!(split.status, OmissionStatus::Recovered);
        assert_eq!(split.title, omission.title);

        // Replaying the stream yields the same ids.
        let ids = |data: &CsbStoreData| {
            let mut ids: Vec<OmissionId> = data.omissions.keys().copied().collect();
            ids.sort();
            ids
        };
        assert_eq!(ids(&replay(&events)), ids(&data));
    }

    #[test]
    fn decisions_alike_read_as_one_omission() {
        let groningen = declarations_of_support(vec![ElectoralDistrict::Groningen]);
        let fryslan = declarations_of_support(vec![ElectoralDistrict::Fryslan]);
        let data = replay(&[
            CsbAction::CreateOmission(fryslan.clone()),
            CsbAction::CreateOmission(groningen.clone()),
            CsbAction::SetOmissionStatus {
                omission_id: fryslan.id,
                status: OmissionStatus::Recovered,
            },
            // The whole of Groningen is decided alike, so it joins Fryslân.
            CsbAction::SetOmissionPartStatus {
                omission_id: groningen.id,
                part: OmissionPart::ElectoralDistrict(ElectoralDistrict::Groningen),
                status: OmissionStatus::Recovered,
            },
        ]);

        assert_eq!(data.omissions.len(), 1);
        let merged = &data.omissions[&fryslan.id];
        assert_eq!(
            merged.category,
            OmissionCategory::DeclarationsOfSupport(vec![
                ElectoralDistrict::Groningen,
                ElectoralDistrict::Fryslan
            ])
        );
        assert_eq!(merged.status, OmissionStatus::Recovered);
    }

    #[test]
    fn a_part_decision_for_an_uncovered_district_changes_nothing() {
        let omission = declarations_of_support(vec![ElectoralDistrict::Groningen]);
        let data = replay(&[
            CsbAction::CreateOmission(omission.clone()),
            CsbAction::SetOmissionPartStatus {
                omission_id: omission.id,
                part: OmissionPart::ElectoralDistrict(ElectoralDistrict::Utrecht),
                status: OmissionStatus::Recovered,
            },
        ]);

        assert_eq!(data.omissions.len(), 1);
        assert_eq!(data.omissions[&omission.id].status, OmissionStatus::Pending);
    }

    #[test]
    fn import_seeds_paper_corrected_data_from_the_snapshot() {
        let mut data = CsbStoreData::default();

        data.apply(StoreEvent::new(1, import_event()));

        assert_eq!(
            data.paper_corrected_data.political_group.appellation,
            sample_political_group().appellation
        );
    }

    #[test]
    fn paper_corrected_update_applies_only_to_the_corrected_projection() {
        let mut data = CsbStoreData::default();
        data.apply(StoreEvent::new(1, import_event()));

        let mut corrected_group = sample_political_group();
        corrected_group.appellation = Some("Gecorrigeerde Naam".parse().unwrap());
        data.apply(StoreEvent::new(
            2,
            CsbAction::PaperCorrectedUpdate(Box::new(PgEvent::UpdatePoliticalGroup(
                corrected_group.clone(),
            )))
            .by(CsbUser::new_test()),
        ));

        assert_eq!(
            data.paper_corrected_data.political_group.appellation,
            corrected_group.appellation
        );
        // The imported snapshot stays untouched.
        assert_eq!(
            data.imported_data.political_group.appellation,
            sample_political_group().appellation
        );
    }

    #[test]
    fn undoing_csb_correction_on_person_removes_csb_correction() {
        let mut data = CsbStoreData::default();
        let person = sample_person(PersonId::new());

        data.paper_corrected_data
            .persons
            .insert(person.id, person.clone());
        data.apply(StoreEvent::new(
            1,
            CsbAction::UpdateCorrection(Correction::Person(
                person.id,
                PersonCorrection::Initials(Initials::from_str("A.B.").unwrap()),
            ))
            .by(CsbUser::new_test()),
        ));
        data.apply(StoreEvent::new(
            2,
            CsbAction::UpdateCorrection(Correction::Person(
                person.id,
                PersonCorrection::LastName(LastName::from_str("Smit").unwrap()),
            ))
            .by(CsbUser::new_test()),
        ));

        assert_eq!(data.csb_corrected_persons.len(), 1);
        assert_eq!(
            data.csb_corrected_persons
                .get(&person.id)
                .unwrap()
                .get_corrections()
                .len(),
            2
        );

        data.apply(StoreEvent::new(
            3,
            CsbAction::UpdateCorrection(Correction::Person(
                person.id,
                PersonCorrection::LastName(person.name.last_name),
            ))
            .by(CsbUser::new_test()),
        ));

        assert_eq!(data.csb_corrected_persons.len(), 1);
        assert_eq!(
            data.csb_corrected_persons
                .get(&person.id)
                .unwrap()
                .get_corrections()
                .len(),
            1
        );

        data.apply(StoreEvent::new(
            4,
            CsbAction::UpdateCorrection(Correction::Person(
                person.id,
                PersonCorrection::Initials(person.name.initials),
            ))
            .by(CsbUser::new_test()),
        ));
        assert_eq!(data.csb_corrected_persons.len(), 0);
    }

    #[test]
    fn set_omission_status_updates_the_status_and_timestamp() {
        use crate::structs::csb::{OmissionCategory, OmissionStatus, sample_omission};

        let mut data = CsbStoreData::default();
        let omission = sample_omission(OmissionCategory::PoliticalGroup);
        data.apply(StoreEvent::new(
            1,
            CsbAction::CreateOmission(omission.clone()).by(CsbUser::new_test()),
        ));

        data.apply(StoreEvent::new(
            2,
            CsbAction::SetOmissionStatus {
                omission_id: omission.id,
                status: OmissionStatus::Recovered,
            }
            .by(CsbUser::new_test()),
        ));

        let stored = data.omissions.get(&omission.id).unwrap();
        assert_eq!(stored.status, OmissionStatus::Recovered);
        assert_eq!(stored.updated_at, data.events[1].created_at.into());
        // The rest of the omission is untouched.
        assert_eq!(stored.description, omission.description);
    }

    #[test]
    fn set_omission_status_for_an_unknown_omission_is_a_no_op() {
        use crate::structs::csb::{OmissionId, OmissionStatus};

        let mut data = CsbStoreData::default();
        data.apply(StoreEvent::new(
            1,
            CsbAction::SetOmissionStatus {
                omission_id: OmissionId::new(),
                status: OmissionStatus::NotRecovered,
            }
            .by(CsbUser::new_test()),
        ));

        assert!(data.omissions.is_empty());
    }

    #[test]
    fn undoing_csb_correction_on_appellation_removes_csb_correction() {
        let mut data = CsbStoreData::default();
        data.paper_corrected_data.political_group.appellation =
            Some(Appellation::from_str("Partij").unwrap());
        data.csb_corrected_appellation =
            Some(Appellation::from_str("Gecorrigeerde Partij").unwrap());

        data.apply(StoreEvent::new(
            1,
            CsbAction::UpdateCorrection(Correction::Appellation(
                Appellation::from_str("Partij").unwrap(),
            ))
            .by(CsbUser::new_test()),
        ));

        assert_eq!(data.csb_corrected_appellation, None);
    }

    #[test]
    fn correction_appellation_sets_corrected_appellation() {
        let mut data = CsbStoreData::default();
        data.apply(StoreEvent::new(1, import_event()));

        let appellation: Appellation = "Gecorrigeerde Naam".parse().unwrap();
        data.apply(StoreEvent::new(
            2,
            CsbAction::UpdateCorrection(Correction::Appellation(appellation.clone()))
                .by(CsbUser::new_test()),
        ));

        assert_eq!(data.csb_corrected_appellation, Some(appellation));
    }

    #[test]
    fn correction_person_accumulates_multiple_corrections() {
        let person_id = PersonId::new();
        let person = sample_person(person_id);

        let mut data = CsbStoreData::default();
        data.apply(StoreEvent::new(1, import_event_with_person(person)));

        let corrections = [
            PersonCorrection::Initials("X.Y.Z.".parse().unwrap()),
            PersonCorrection::LastName("Bakker".parse().unwrap()),
            PersonCorrection::DateOfBirth("15-06-1985".parse().unwrap()),
            PersonCorrection::PlaceOfResidence(PlaceOfResidence::Known("Amsterdam".to_string())),
        ];

        // After each event the accumulated set contains all corrections so far.
        let mut expected = HashSet::new();
        for (i, correction) in corrections.into_iter().enumerate() {
            data.apply(StoreEvent::new(
                i + 2,
                CsbAction::UpdateCorrection(Correction::Person(person_id, correction.clone()))
                    .by(CsbUser::new_test()),
            ));
            expected.insert(correction);

            let corrected = data.csb_corrected_persons.get(&person_id).unwrap();
            assert_eq!(corrected.get_corrections(), expected);
        }
    }
}

#[cfg(test)]
mod brp_reset_tests {
    use super::*;
    use crate::{
        structs::{
            brp::BrpFinding,
            csb::{Correction, PersonCorrection},
        },
        test_utils::sample_person,
    };

    /// A store holding `person`, already checked against the BRP.
    async fn checked_store(person: crate::structs::persons::Person) -> crate::CsbStore {
        let store = crate::CsbStore::new_for_test();
        store.add_person(person.clone());
        store
            .update(CsbAction::BrpPersonChecked {
                person: person.id,
                findings: vec![BrpFinding::NotDutch],
            })
            .await
            .unwrap();
        store
            .update(CsbAction::SetBrpStatus(BrpStatus::Finished))
            .await
            .unwrap();
        assert!(store.is_brp_checked(person.id));
        store
    }

    #[tokio::test]
    async fn an_ambtshalve_correction_drops_what_the_brp_said() {
        let person = sample_person(PersonId::new());
        let person_id = person.id;
        let store = checked_store(person).await;

        store
            .update(CsbAction::UpdateCorrection(Correction::Person(
                person_id,
                PersonCorrection::LastName("Gecorrigeerd".parse().unwrap()),
            )))
            .await
            .unwrap();

        assert!(
            !store.is_brp_checked(person_id),
            "the findings are about values that are no longer on screen"
        );
        // The sweep still ran to completion; it is the candidate that changed.
        assert_eq!(store.get_brp_status(), BrpStatus::Finished);
    }

    #[tokio::test]
    async fn a_paper_correction_to_the_candidate_drops_what_the_brp_said() {
        let mut person = sample_person(PersonId::new());
        let person_id = person.id;
        let store = checked_store(person.clone()).await;

        person.name.last_name = "Gecorrigeerd".parse().unwrap();
        store
            .update(CsbAction::PaperCorrectedUpdate(Box::new(
                crate::PgEvent::UpdatePerson(person),
            )))
            .await
            .unwrap();

        assert!(!store.is_brp_checked(person_id));
    }

    #[tokio::test]
    async fn a_paper_correction_csv_import_drops_what_the_brp_said() {
        let mut person = sample_person(PersonId::new());
        let person_id = person.id;
        let store = checked_store(person.clone()).await;

        person.name.last_name = "Gecorrigeerd".parse().unwrap();
        store
            .update(CsbAction::PaperCorrectedUpdate(Box::new(
                crate::PgEvent::ImportCandidates {
                    list_id: crate::structs::candidate_lists::CandidateListId::new(),
                    file_name: "kandidaten.csv".to_string(),
                    file_size: 1,
                    created_persons: Vec::new(),
                    updated_persons: vec![person],
                    candidates: vec![person_id],
                },
            )))
            .await
            .unwrap();

        assert!(!store.is_brp_checked(person_id));
    }

    #[tokio::test]
    async fn a_change_the_brp_check_never_looked_at_leaves_the_findings_alone() {
        let person = sample_person(PersonId::new());
        let person_id = person.id;
        let store = checked_store(person).await;

        // The correspondence address is not compared against the BRP.
        store
            .update(CsbAction::PaperCorrectedUpdate(Box::new(
                crate::PgEvent::UpdatePersonAddress {
                    person_id,
                    address: Default::default(),
                },
            )))
            .await
            .unwrap();
        // Nor is anything about the political group.
        store
            .update(CsbAction::UpdateCorrection(Correction::Appellation(
                "Andere Naam".parse().unwrap(),
            )))
            .await
            .unwrap();

        assert!(store.is_brp_checked(person_id));
    }

    #[tokio::test]
    async fn a_candidate_nobody_checked_is_unaffected() {
        let store = crate::CsbStore::new_for_test();
        let person = sample_person(PersonId::new());
        let person_id = person.id;
        store.add_person(person);

        store
            .update(CsbAction::UpdateCorrection(Correction::Person(
                person_id,
                PersonCorrection::LastName("Gecorrigeerd".parse().unwrap()),
            )))
            .await
            .unwrap();

        assert!(!store.is_brp_checked(person_id));
        assert_eq!(store.get_brp_status(), BrpStatus::NotStarted);
    }
}
