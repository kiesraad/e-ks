mod event;
mod getters;

pub use event::PgEvent;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{
    Scope,
    store::{StoreData, StoreEvent},
    structs::{
        candidate_lists::{CandidateList, CandidateListId},
        common::UtcDateTime,
        list_submitters::ListSubmitter,
        name_authorisations::{NameAuthorisation, NameAuthorisationId},
        persons::{Person, PersonId},
        political_groups::PoliticalGroup,
    },
};

/// Event-sourced domain projection for a single (stream, election) pair.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PgStoreData {
    pub(crate) political_group: PoliticalGroup,
    pub(crate) persons: HashMap<PersonId, Person>,
    pub(crate) candidate_lists: HashMap<CandidateListId, CandidateList>,
    pub(crate) name_authorisations: HashMap<NameAuthorisationId, NameAuthorisation>,
    pub(crate) list_submitter: ListSubmitter,
    pub(crate) substitute_submitters: Vec<ListSubmitter>,
    pub(crate) events: Vec<StoreEvent<PgEvent>>,
}

impl StoreData for PgStoreData {
    type Event = PgEvent;

    fn apply(&mut self, event: StoreEvent<PgEvent>) {
        self.events.push(event.clone());

        let event_time = UtcDateTime::from(event.created_at);

        match event.payload {
            PgEvent::UpdatePoliticalGroup(pg) => self.political_group = pg,

            event @ (PgEvent::CreatePerson(_)
            | PgEvent::CreatePersonPersonalData { .. }
            | PgEvent::UpdatePerson(_)
            | PgEvent::UpdatePersonPersonalData { .. }
            | PgEvent::UpdatePersonAddress { .. }
            | PgEvent::UpdatePersonRepresentative { .. }
            | PgEvent::DeletePerson { .. }) => self.apply_person_event(event, event_time),

            event @ (PgEvent::CreateCandidateList(_)
            | PgEvent::UpdateCandidateListDistricts { .. }
            | PgEvent::UpdateCandidateListOrder { .. }
            | PgEvent::AddCandidateToCandidateList { .. }
            | PgEvent::RemoveCandidateFromCandidateList { .. }
            | PgEvent::DeleteCandidateList(_)) => {
                self.apply_candidate_list_event(event, event_time)
            }

            event @ (PgEvent::CreateNameAuthorisation(_)
            | PgEvent::UpdateNameAuthorisation(_)
            | PgEvent::DeleteNameAuthorisation(_)) => self.apply_name_authorisation_event(event),

            event @ (PgEvent::UpdateListSubmitter(_)
            | PgEvent::CreateSubstituteSubmitter(_)
            | PgEvent::UpdateSubstituteSubmitter(_)
            | PgEvent::DeleteSubstituteSubmitter { .. }) => self.apply_submitter_event(event),

            PgEvent::ImportCandidates {
                list_id,
                created_persons,
                updated_persons,
                candidates,
                ..
            } => self.apply_import_candidates_event(
                list_id,
                created_persons,
                updated_persons,
                candidates,
                event_time,
            ),

            // Only the serialized event is relevant for logging.
            PgEvent::DeveloperLogin { .. }
            | PgEvent::Login
            | PgEvent::Logout
            | PgEvent::DownloadFile { .. }
            | PgEvent::HideDownloadWarning
            | PgEvent::ExportCsv { .. }
            | PgEvent::Import { .. } => {}
        }
    }

    fn events(&self) -> &[StoreEvent<Self::Event>] {
        &self.events
    }

    fn scope() -> Scope {
        Scope::PoliticalGroup
    }
}

impl PgStoreData {
    /// Build a point-in-time snapshot of the projection by replaying `events`
    /// up to and including the event with `target_event_id`.
    ///
    /// The returned snapshot has its own `events` log
    /// cleared: it captures only the derived state, not the event history that
    /// produced it. `events` is expected to be the ordered event log of a single
    /// stream (as returned by `StoreData::events`); later events are ignored.
    pub fn snapshot_until(events: &[StoreEvent<PgEvent>], target_event_id: usize) -> Self {
        let mut snapshot = PgStoreData::default();
        for event in events
            .iter()
            .filter(|event| event.event_id <= target_event_id)
        {
            snapshot.apply(event.clone());
        }
        snapshot.events.clear();
        snapshot
    }

    /// Apply a person-related event. Routed here exclusively by [`Self::apply`].
    fn apply_person_event(&mut self, event: PgEvent, event_time: UtcDateTime) {
        match event {
            PgEvent::CreatePerson(_) | PgEvent::CreatePersonPersonalData { .. } => {
                self.apply_person_create_event(event, event_time)
            }
            PgEvent::UpdatePerson(_)
            | PgEvent::UpdatePersonPersonalData { .. }
            | PgEvent::UpdatePersonAddress { .. }
            | PgEvent::UpdatePersonRepresentative { .. } => {
                self.apply_person_update_event(event, event_time)
            }
            PgEvent::DeletePerson { person_id } => {
                self.candidate_lists
                    .values_mut()
                    .for_each(|list| list.candidates.retain(|id| *id != person_id));

                self.persons.remove(&person_id);
            }
            _ => unreachable!("apply_person_event received a non-person event"),
        }
    }

    /// Apply a person-creation event. Routed here exclusively by [`Self::apply_person_event`].
    fn apply_person_create_event(&mut self, event: PgEvent, event_time: UtcDateTime) {
        match event {
            PgEvent::CreatePerson(mut person) => {
                person.updated_at = event_time;
                self.persons.insert(person.id, person);
            }
            PgEvent::CreatePersonPersonalData {
                person_id,
                name,
                personal_data,
            } => {
                let person = Person {
                    id: person_id,
                    name,
                    personal_data,
                    updated_at: event_time,
                    ..Default::default()
                };
                self.persons.insert(person_id, person);
            }
            _ => unreachable!("apply_person_create_event received a non-create event"),
        }
    }

    /// Apply a person-update event. Routed here exclusively by [`Self::apply_person_event`].
    fn apply_person_update_event(&mut self, event: PgEvent, event_time: UtcDateTime) {
        match event {
            PgEvent::UpdatePerson(mut person) => {
                person.updated_at = event_time;
                let person_id = person.id;
                self.persons.entry(person_id).and_modify(|existing| {
                    *existing = person;
                });
            }
            PgEvent::UpdatePersonPersonalData {
                person_id,
                name,
                personal_data,
            } => {
                self.persons.entry(person_id).and_modify(|existing| {
                    existing.name = name;
                    existing.personal_data = personal_data;
                    existing.updated_at = event_time;
                });
            }
            PgEvent::UpdatePersonAddress { person_id, address } => {
                self.persons.entry(person_id).and_modify(|existing| {
                    existing.address = address;
                    existing.updated_at = event_time;
                });
            }
            PgEvent::UpdatePersonRepresentative {
                person_id,
                representative,
            } => {
                self.persons.entry(person_id).and_modify(|existing| {
                    existing.representative = representative;
                    existing.updated_at = event_time;
                });
            }
            _ => unreachable!("apply_person_update_event received a non-update event"),
        }
    }

    /// Apply a candidate-list event. Routed here exclusively by [`Self::apply`].
    fn apply_candidate_list_event(&mut self, event: PgEvent, event_time: UtcDateTime) {
        match event {
            PgEvent::CreateCandidateList(mut cl) => {
                cl.created_at = event_time;
                self.candidate_lists.insert(cl.id, cl);
            }
            PgEvent::UpdateCandidateListDistricts {
                list_id,
                electoral_districts,
            } => {
                self.candidate_lists.entry(list_id).and_modify(|existing| {
                    existing.electoral_districts = electoral_districts;
                });
            }
            PgEvent::UpdateCandidateListOrder {
                list_id,
                candidates,
            } => {
                self.candidate_lists.entry(list_id).and_modify(|existing| {
                    existing.candidates = candidates;
                });
            }
            PgEvent::AddCandidateToCandidateList { list_id, person_id } => {
                self.candidate_lists.entry(list_id).and_modify(|existing| {
                    if !existing.candidates.contains(&person_id) {
                        existing.candidates.push(person_id);
                    }
                });
            }
            PgEvent::RemoveCandidateFromCandidateList { list_id, person_id } => {
                self.candidate_lists.entry(list_id).and_modify(|existing| {
                    existing.candidates.retain(|id| *id != person_id);
                });
            }
            PgEvent::DeleteCandidateList(cl_id) => {
                self.candidate_lists.remove(&cl_id);
            }
            _ => unreachable!("apply_candidate_list_event received a non-candidate-list event"),
        }
    }

    /// Apply a name-authorisation event. Routed here exclusively by [`Self::apply`].
    fn apply_name_authorisation_event(&mut self, event: PgEvent) {
        match event {
            PgEvent::CreateNameAuthorisation(aa) => {
                self.name_authorisations.insert(aa.id, aa);
            }
            PgEvent::UpdateNameAuthorisation(aa) => {
                let aa_id = aa.id;
                self.name_authorisations
                    .entry(aa_id)
                    .and_modify(|existing| {
                        *existing = aa;
                    });
            }
            PgEvent::DeleteNameAuthorisation(aa_id) => {
                self.name_authorisations.remove(&aa_id);
            }
            _ => unreachable!(
                "apply_name_authorisation_event received a non-name-authorisation event"
            ),
        }
    }

    /// Apply a list-submitter event. Routed here exclusively by [`Self::apply`].
    fn apply_submitter_event(&mut self, event: PgEvent) {
        match event {
            PgEvent::UpdateListSubmitter(ls) => {
                self.list_submitter = ls;
            }
            PgEvent::CreateSubstituteSubmitter(ss) => {
                self.substitute_submitters.push(ss);
            }
            PgEvent::UpdateSubstituteSubmitter(ss) => {
                let ss_id = ss.id;
                if let Some(existing) = self
                    .substitute_submitters
                    .iter_mut()
                    .find(|existing| existing.id == ss_id)
                {
                    *existing = ss;
                }
            }
            PgEvent::DeleteSubstituteSubmitter {
                substitute_submitter_id: ss_id,
            } => {
                self.substitute_submitters
                    .retain(|submitter| submitter.id != ss_id);
            }
            _ => unreachable!("apply_submitter_event received a non-submitter event"),
        }
    }

    /// Apply an import-candidates event. Routed here exclusively by [`Self::apply`].
    fn apply_import_candidates_event(
        &mut self,
        list_id: CandidateListId,
        created_persons: Vec<Person>,
        updated_persons: Vec<Person>,
        candidates: Vec<PersonId>,
        event_time: UtcDateTime,
    ) {
        for mut person in created_persons {
            person.updated_at = event_time;
            self.persons.insert(person.id, person);
        }
        for mut person in updated_persons {
            person.updated_at = event_time;
            let person_id = person.id;
            self.persons.entry(person_id).and_modify(|existing| {
                *existing = person;
            });
        }
        self.candidate_lists.entry(list_id).and_modify(|existing| {
            existing.candidates = candidates;
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        AppError, ElectoralDistrict, PgEvent, PgStore, PgStoreData,
        store::{StoreData, StoreEvent},
        structs::{
            candidate_lists::CandidateListId,
            common::{
                DutchAddress, FullName, HouseNumber, HouseNumberAddition, Initials, LastName,
                Locality, PostalCode, StreetName, UtcDateTime,
            },
            persons::{PersonId, Representative},
        },
        test_utils::{sample_candidate_list, sample_name_authorisation, sample_person},
    };
    use chrono::{Duration, Utc};
    use std::collections::BTreeSet;

    /// A `PgStoreData` containing a single sample person.
    fn data_with_person(person_id: PersonId) -> PgStoreData {
        let mut data = PgStoreData::default();
        data.persons.insert(person_id, sample_person(person_id));
        data
    }

    fn sample_address() -> DutchAddress {
        DutchAddress {
            locality: Some("Utrecht".parse::<Locality>().expect("locality")),
            postal_code: Some("3511 AA".parse::<PostalCode>().expect("postal code")),
            house_number: Some("12".parse::<HouseNumber>().expect("house number")),
            house_number_addition: Some(
                "A".parse::<HouseNumberAddition>()
                    .expect("house number addition"),
            ),
            street_name: Some("Oudegracht".parse::<StreetName>().expect("street name")),
            known_in_bag: Some(true),
        }
    }

    fn sample_representative() -> Representative {
        Representative {
            name: FullName {
                first_name: None,
                last_name: "Bakker".parse::<LastName>().expect("last name"),
                last_name_prefix: None,
                initials: Some("C.D.".parse::<Initials>().expect("initials")),
            },
            address: DutchAddress {
                locality: Some("Rotterdam".parse::<Locality>().expect("locality")),
                postal_code: Some("3011 CC".parse::<PostalCode>().expect("postal code")),
                house_number: Some("5".parse::<HouseNumber>().expect("house number")),
                house_number_addition: None,
                street_name: Some("Coolsingel".parse::<StreetName>().expect("street name")),
                known_in_bag: Some(true),
            },
        }
    }

    #[test]
    fn apply_update_person_address_keeps_representative() {
        let person_id = PersonId::new();
        let mut data = data_with_person(person_id);
        let new_address = sample_address();

        let original_representative = data
            .persons
            .get(&person_id)
            .expect("person exists")
            .representative
            .clone();

        let event_time = Utc::now() - Duration::seconds(20);
        data.apply(StoreEvent::new_at(
            1,
            PgEvent::UpdatePersonAddress {
                person_id,
                address: new_address.clone(),
            },
            event_time,
        ));

        let updated = data.persons.get(&person_id).expect("person exists");
        assert_eq!(updated.address.postal_code, new_address.postal_code);
        assert_eq!(updated.updated_at, UtcDateTime::from(event_time));
        assert_eq!(updated.representative, original_representative);
    }

    #[test]
    fn apply_update_person_representative() {
        let person_id = PersonId::new();
        let mut data = data_with_person(person_id);
        let representative = sample_representative();

        let event_time = Utc::now() - Duration::seconds(10);
        data.apply(StoreEvent::new_at(
            1,
            PgEvent::UpdatePersonRepresentative {
                person_id,
                representative: Some(representative.clone()),
            },
            event_time,
        ));

        let updated = data.persons.get(&person_id).expect("person exists");
        let updated_representative = updated.representative.as_ref().expect("representative");
        assert_eq!(updated_representative.name.last_name.to_string(), "Bakker");
        assert_eq!(
            updated_representative.address.street_name,
            representative.address.street_name
        );
        assert_eq!(updated.updated_at, UtcDateTime::from(event_time));
    }

    #[test]
    fn apply_add_candidate_to_list_deduplicates() {
        let mut data = PgStoreData::default();
        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);

        let created_at = Utc::now() - Duration::seconds(60);
        data.apply(StoreEvent::new_at(
            1,
            PgEvent::CreateCandidateList(list.clone()),
            created_at,
        ));

        let person_id = PersonId::new();
        let added_at = Utc::now() - Duration::seconds(30);
        data.apply(StoreEvent::new_at(
            2,
            PgEvent::AddCandidateToCandidateList { list_id, person_id },
            added_at,
        ));

        let updated = data.candidate_lists.get(&list_id).expect("list exists");
        assert_eq!(updated.candidates, vec![person_id]);

        let ignored_at = Utc::now() - Duration::seconds(5);
        data.apply(StoreEvent::new_at(
            3,
            PgEvent::AddCandidateToCandidateList { list_id, person_id },
            ignored_at,
        ));

        let updated_again = data.candidate_lists.get(&list_id).expect("list exists");
        assert_eq!(updated_again.candidates, vec![person_id]);
    }

    #[test]
    fn apply_delete_person_updates_only_candidate_lists_with_that_candidate() {
        let mut data = PgStoreData::default();
        let person_id = PersonId::new();
        let base_time = Utc::now();

        let list_id_with = CandidateListId::new();
        let mut list_with = sample_candidate_list(list_id_with);
        list_with.candidates = vec![person_id];

        let list_id_without = CandidateListId::new();
        let list_without = sample_candidate_list(list_id_without);

        data.apply(StoreEvent::new_at(
            1,
            PgEvent::CreateCandidateList(list_with),
            base_time - Duration::seconds(50),
        ));
        data.apply(StoreEvent::new_at(
            2,
            PgEvent::CreateCandidateList(list_without),
            base_time - Duration::seconds(40),
        ));

        let removed_at = base_time - Duration::seconds(10);
        data.apply(StoreEvent::new_at(
            3,
            PgEvent::DeletePerson { person_id },
            removed_at,
        ));

        let updated_with = data
            .candidate_lists
            .get(&list_id_with)
            .expect("list with person exists");
        assert!(updated_with.candidates.is_empty());

        let updated_without = data
            .candidate_lists
            .get(&list_id_without)
            .expect("list without person exists");
        assert!(updated_without.candidates.is_empty());
    }

    #[test]
    fn apply_remove_candidate_from_candidate_list_updates_list() {
        let mut data = PgStoreData::default();
        let list_id = CandidateListId::new();
        let person_id = PersonId::new();
        let other_person_id = PersonId::new();
        let base_time = Utc::now();

        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id, other_person_id];

        data.apply(StoreEvent::new_at(
            1,
            PgEvent::CreateCandidateList(list),
            base_time - Duration::seconds(45),
        ));

        let removed_at = base_time - Duration::seconds(5);
        data.apply(StoreEvent::new_at(
            2,
            PgEvent::RemoveCandidateFromCandidateList { list_id, person_id },
            removed_at,
        ));

        let updated = data.candidate_lists.get(&list_id).expect("list exists");
        assert_eq!(updated.candidates, vec![other_person_id]);
    }

    #[test]
    fn apply_update_candidate_list_districts_replaces_districts() {
        let mut data = PgStoreData::default();
        let list_id = CandidateListId::new();
        let base_time = Utc::now();

        let mut list = sample_candidate_list(list_id);
        list.electoral_districts = BTreeSet::from([ElectoralDistrict::Utrecht]);

        data.apply(StoreEvent::new_at(
            1,
            PgEvent::CreateCandidateList(list),
            base_time - Duration::seconds(50),
        ));

        let updated_at = base_time - Duration::seconds(15);
        let districts = BTreeSet::from([
            ElectoralDistrict::NoordHolland,
            ElectoralDistrict::ZuidHolland,
        ]);
        data.apply(StoreEvent::new_at(
            2,
            PgEvent::UpdateCandidateListDistricts {
                list_id,
                electoral_districts: districts.clone(),
            },
            updated_at,
        ));

        let updated = data.candidate_lists.get(&list_id).expect("list exists");
        assert_eq!(updated.electoral_districts, districts);
    }

    #[test]
    fn apply_update_candidate_list_order_replaces_candidates() {
        let mut data = PgStoreData::default();
        let list_id = CandidateListId::new();
        let person_id = PersonId::new();
        let other_person_id = PersonId::new();
        let base_time = Utc::now();

        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id, other_person_id];

        data.apply(StoreEvent::new_at(
            1,
            PgEvent::CreateCandidateList(list),
            base_time - Duration::seconds(40),
        ));

        let updated_at = base_time - Duration::seconds(10);
        let new_order = vec![other_person_id, person_id];
        data.apply(StoreEvent::new_at(
            2,
            PgEvent::UpdateCandidateListOrder {
                list_id,
                candidates: new_order.clone(),
            },
            updated_at,
        ));

        let updated = data.candidate_lists.get(&list_id).expect("list exists");
        assert_eq!(updated.candidates, new_order);
    }

    #[tokio::test]
    async fn store_update_applies_event_in_memory() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let id = crate::structs::name_authorisations::NameAuthorisationId::new();
        let name_authorisation = sample_name_authorisation(id);

        name_authorisation.create(&store).await?;

        let loaded = store.get_name_authorisation(id)?;
        assert_eq!(loaded.id, name_authorisation.id);

        Ok(())
    }

    #[test]
    fn snapshot_until_replays_up_to_the_target_event_and_drops_the_log() {
        let person_a = PersonId::new();
        let person_b = PersonId::new();
        let events = vec![
            StoreEvent::new(1, PgEvent::CreatePerson(sample_person(person_a))),
            StoreEvent::new(2, PgEvent::CreatePerson(sample_person(person_b))),
            StoreEvent::new(
                3,
                PgEvent::DeletePerson {
                    person_id: person_a,
                },
            ),
        ];

        // Up to event 2: both persons present, and the snapshot carries no event log.
        let at_two = PgStoreData::snapshot_until(&events, 2);
        assert!(at_two.persons.contains_key(&person_a));
        assert!(at_two.persons.contains_key(&person_b));
        assert!(at_two.events.is_empty());

        // Up to event 3: the deletion has been applied.
        let at_three = PgStoreData::snapshot_until(&events, 3);
        assert!(!at_three.persons.contains_key(&person_a));
        assert!(at_three.persons.contains_key(&person_b));
        assert!(at_three.events.is_empty());
    }

    #[test]
    fn snapshot_until_ignores_events_past_the_target() {
        let person_a = PersonId::new();
        let person_b = PersonId::new();
        let events = vec![
            StoreEvent::new(1, PgEvent::CreatePerson(sample_person(person_a))),
            StoreEvent::new(2, PgEvent::CreatePerson(sample_person(person_b))),
        ];

        // Stopping at event 1 leaves the later creation out of the snapshot.
        let snapshot = PgStoreData::snapshot_until(&events, 1);
        assert!(snapshot.persons.contains_key(&person_a));
        assert!(!snapshot.persons.contains_key(&person_b));
    }

    /// In paper-corrections mode the handle reads the CSB stream's corrected
    /// projection, and every dispatched app event is wrapped in
    /// [`crate::CsbAction::PaperCorrectedUpdate`] and persisted on that stream.
    #[tokio::test]
    async fn paper_corrections_store_wraps_events_and_refreshes_its_snapshot()
    -> Result<(), AppError> {
        use crate::{CsbAction, CsbStore, test_utils::sample_political_group};

        let csb_store = CsbStore::new_for_test();
        csb_store.set_political_group(sample_political_group());
        let store = csb_store.paper_corrections();

        // Reads serve a snapshot of the corrected projection.
        assert_eq!(
            store.get_political_group().appellation,
            sample_political_group().appellation
        );

        let mut corrected_group = sample_political_group();
        corrected_group.appellation = Some("Gecorrigeerde Naam".parse().unwrap());
        store
            .update(PgEvent::UpdatePoliticalGroup(corrected_group.clone()))
            .await?;

        // The event lands on the CSB stream, wrapped as a paper correction.
        {
            let data = csb_store.data.read();
            assert!(matches!(
                &data.events.last().unwrap().payload.action,
                CsbAction::PaperCorrectedUpdate(inner)
                    if matches!(**inner, PgEvent::UpdatePoliticalGroup(_))
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

        // The request-local snapshot observes the correction right away.
        assert_eq!(
            store.get_political_group().appellation,
            corrected_group.appellation
        );

        Ok(())
    }

    #[cfg(feature = "database")]
    mod database_tests {
        use super::*;
        use crate::{
            ElectionConfig, Province, Scope, StreamId, structs::persons::PersonId,
            test_utils::sample_person,
        };
        use chrono::Utc;
        use sqlx::PgPool;

        fn test_master() -> crate::crypto::MasterKey {
            crate::crypto::MasterKey::new(&secrecy::SecretString::from("test-encryption-key"))
        }

        /// Create a stream row directly, as `into_backend_for_stream` would.
        async fn ensure_test_stream(
            pool: &PgPool,
            stream_id: StreamId,
            election: ElectionConfig,
            scope: Scope,
        ) -> Result<(), AppError> {
            use crate::{crypto::StreamKey, store::persistence::NewStream};

            let encrypted_key =
                test_master().wrap_key(&StreamKey::generate(), stream_id, election)?;
            crate::store::database::ensure_stream(
                pool,
                &NewStream {
                    stream_id,
                    election,
                    scope,
                    encrypted_key,
                },
            )
            .await?;
            Ok(())
        }

        #[cfg_attr(not(feature = "db-tests"), ignore = "requires database")]
        #[sqlx::test(migrations = false)]
        async fn update_persists_and_load_replays(pool: PgPool) -> Result<(), AppError> {
            #[cfg(feature = "migrations")]
            crate::store::database::migrate(&pool).await?;

            let master = test_master();
            let group_id = StreamId::new();
            let store = PgStore::new_with_pool_for_stream(
                pool.clone(),
                group_id,
                ElectionConfig::EK27,
                &master,
            )
            .await
            .unwrap();
            let person_id = PersonId::new();
            let person = sample_person(person_id);

            person.create(&store).await?;

            let loaded = store.get_person(person_id)?;
            assert_eq!(loaded.id, person_id);

            let fresh_store =
                PgStore::new_with_pool_for_stream(pool, group_id, ElectionConfig::EK27, &master)
                    .await
                    .unwrap();
            fresh_store.load().await?;

            let reloaded = fresh_store.get_person(person_id)?;
            assert_eq!(reloaded.id, person_id);

            Ok(())
        }

        #[cfg_attr(not(feature = "db-tests"), ignore = "requires database")]
        #[sqlx::test(migrations = false)]
        async fn load_fails_on_invalid_payloads(pool: PgPool) -> Result<(), AppError> {
            #[cfg(feature = "migrations")]
            crate::store::database::migrate(&pool).await?;

            let master = test_master();
            let group_id = StreamId::new();
            let store = PgStore::new_with_pool_for_stream(
                pool.clone(),
                group_id,
                ElectionConfig::EK27,
                &master,
            )
            .await
            .unwrap();
            let person_id = PersonId::new();
            let person = sample_person(person_id);

            person.create(&store).await?;

            // Insert a bogus event: random payload bytes and a hash that does not
            // match the chain. Either the chain check or the AES-GCM tag will reject it.
            let invalid_payload: Vec<u8> = vec![0u8; 64];
            let invalid_hash: Vec<u8> = vec![0u8; 32];
            let election_id = ElectionConfig::EK27.stable_id();
            sqlx::query(
                r#"INSERT INTO events (stream_id, election, event_id, created_at, hash, payload)
                VALUES ($1, $2, $3, $4, $5, $6)"#,
            )
            .bind(store.stream_id.uuid())
            .bind(&election_id)
            .bind(2_i64)
            .bind(Utc::now())
            .bind(invalid_hash)
            .bind(invalid_payload)
            .execute(&pool)
            .await?;

            sqlx::query(
                r#"UPDATE streams SET last_event_id = $3
                   WHERE stream_id = $1 AND election = $2"#,
            )
            .bind(store.stream_id.uuid())
            .bind(&election_id)
            .bind(2_i64)
            .execute(&pool)
            .await?;

            let fresh_store =
                PgStore::new_with_pool_for_stream(pool, group_id, ElectionConfig::EK27, &master)
                    .await
                    .unwrap();

            let err = fresh_store
                .load()
                .await
                .expect_err("load must fail when an event's payload cannot be decrypted");
            assert!(matches!(err, AppError::EventDecodeError(_)));

            Ok(())
        }

        /// Each stream row carries its scope (set at creation). `streams_by_scope`
        /// lists every data-bearing `(stream_id, election)` of the requested scope,
        /// and a committee stream never leaks into the political-group listing,
        /// even across several elections under one stream_id.
        #[cfg_attr(not(feature = "db-tests"), ignore = "requires database")]
        #[sqlx::test(migrations = false)]
        async fn streams_by_scope_lists_stream_election_pairs_per_scope(
            pool: PgPool,
        ) -> Result<(), AppError> {
            use crate::store::database::streams_by_scope;

            #[cfg(feature = "migrations")]
            crate::store::database::migrate(&pool).await?;

            let committee = StreamId::new();
            let group = StreamId::new();
            let ek27 = ElectionConfig::EK27;
            let ps27 = ElectionConfig::PS27(Province::Gelderland);

            // The committee stream joins two elections; each row is created with the
            // committee scope. The political group joins one.
            ensure_test_stream(&pool, committee, ek27, Scope::CentralElectoralCommittee).await?;
            ensure_test_stream(&pool, committee, ps27, Scope::CentralElectoralCommittee).await?;
            ensure_test_stream(&pool, group, ek27, Scope::PoliticalGroup).await?;

            // Empty placeholder rows (last_event_id = 0) are not yet accessible.
            assert!(
                streams_by_scope(&pool, Scope::CentralElectoralCommittee)
                    .await?
                    .is_empty(),
                "data-less streams are excluded"
            );

            // Give every stream some persisted data so it counts as accessible.
            sqlx::query("UPDATE streams SET last_event_id = 1")
                .execute(&pool)
                .await?;

            // Both committee elections are listed under the committee scope.
            let mut committee_streams =
                streams_by_scope(&pool, Scope::CentralElectoralCommittee).await?;
            committee_streams.sort_by_key(|(_, election)| election.stable_id());
            assert_eq!(
                committee_streams,
                vec![(committee, ek27), (committee, ps27)]
            );

            // The committee stream never leaks into the (default) political-group
            // listing; only the political group's own stream appears there.
            let political = streams_by_scope(&pool, Scope::PoliticalGroup).await?;
            assert_eq!(political, vec![(group, ek27)]);

            Ok(())
        }

        /// A package hash resolves to the political-group event that produced it,
        /// both for a full chain hash and for the 16-byte prefix rendered on
        /// documents; an unrelated prefix resolves to nothing.
        #[cfg_attr(not(feature = "db-tests"), ignore = "requires database")]
        #[sqlx::test(migrations = false)]
        async fn find_event_by_hash_prefix_locates_political_group_events(
            pool: PgPool,
        ) -> Result<(), AppError> {
            use crate::store::database::find_event_by_hash_prefix;

            #[cfg(feature = "migrations")]
            crate::store::database::migrate(&pool).await?;

            let master = test_master();
            let group = StreamId::new();
            let store = PgStore::new_with_pool_for_stream(
                pool.clone(),
                group,
                ElectionConfig::EK27,
                &master,
            )
            .await
            .unwrap();
            sample_person(PersonId::new()).create(&store).await?;

            let target = store
                .get_events()
                .last()
                .cloned()
                .expect("at least one event");
            let expected = Some((group, ElectionConfig::EK27, target.event_id));

            assert_eq!(
                find_event_by_hash_prefix(&pool, &target.hash).await?,
                expected
            );
            assert_eq!(
                find_event_by_hash_prefix(&pool, &target.hash[..16]).await?,
                expected
            );
            assert_eq!(find_event_by_hash_prefix(&pool, &[0xFFu8; 32]).await?, None);

            Ok(())
        }

        /// The lookup is restricted to political-group streams, so a committee
        /// (CSB) event is never returned even when its hash matches exactly.
        #[cfg_attr(not(feature = "db-tests"), ignore = "requires database")]
        #[sqlx::test(migrations = false)]
        async fn find_event_by_hash_prefix_ignores_committee_events(
            pool: PgPool,
        ) -> Result<(), AppError> {
            use crate::store::database::find_event_by_hash_prefix;

            #[cfg(feature = "migrations")]
            crate::store::database::migrate(&pool).await?;

            let committee = StreamId::new();
            let election_id = ElectionConfig::EK27.stable_id();
            ensure_test_stream(
                &pool,
                committee,
                ElectionConfig::EK27,
                Scope::CentralElectoralCommittee,
            )
            .await?;

            let hash = vec![0x42u8; 32];
            sqlx::query(
                r#"INSERT INTO events (stream_id, election, event_id, created_at, hash, payload)
                VALUES ($1, $2, $3, $4, $5, $6)"#,
            )
            .bind(committee.uuid())
            .bind(&election_id)
            .bind(1_i64)
            .bind(Utc::now())
            .bind(&hash)
            .bind(vec![0u8; 8])
            .execute(&pool)
            .await?;

            assert_eq!(find_event_by_hash_prefix(&pool, &hash).await?, None);

            Ok(())
        }

        /// A stream row created before per-stream keys existed carries no
        /// `encrypted_key`. `ensure_stream` backfills the caller's fresh key onto
        /// such a row exactly once (later calls keep the stored key), and events
        /// written under the old scheme fail to decrypt rather than replay.
        #[cfg_attr(not(feature = "db-tests"), ignore = "requires database")]
        #[sqlx::test(migrations = false)]
        async fn ensure_stream_backfills_keys_onto_pre_upgrade_rows(
            pool: PgPool,
        ) -> Result<(), AppError> {
            use crate::{
                crypto::{StreamKey, WrappedKey},
                store::{
                    GENESIS_HASH, chain_hash, database::ensure_stream, event_aad,
                    persistence::NewStream,
                },
            };

            #[cfg(feature = "migrations")]
            crate::store::database::migrate(&pool).await?;

            let master = test_master();
            let group_id = StreamId::new();
            let election = ElectionConfig::EK27;
            let election_id = election.stable_id();

            // A pre-upgrade row: no wrapped key, one event encrypted under the old
            // scheme, whose key is not recoverable from the database.
            sqlx::query(
                r#"INSERT INTO streams (stream_id, election, last_event_id, scope, encrypted_key)
                VALUES ($1, $2, 1, $3, NULL)"#,
            )
            .bind(group_id.uuid())
            .bind(&election_id)
            .bind(Scope::PoliticalGroup.as_str())
            .execute(&pool)
            .await?;

            let old_cipher = StreamKey::generate().cipher();
            let created_at = Utc::now();
            let event = PgEvent::CreatePerson(sample_person(PersonId::new()));
            let payload = old_cipher.encrypt(&event, &event_aad(1, created_at, &GENESIS_HASH))?;
            let hash = chain_hash(&GENESIS_HASH, 1, created_at, &payload);
            sqlx::query(
                r#"INSERT INTO events (stream_id, election, event_id, created_at, hash, payload)
                VALUES ($1, $2, $3, $4, $5, $6)"#,
            )
            .bind(group_id.uuid())
            .bind(&election_id)
            .bind(1_i64)
            .bind(created_at)
            .bind(hash.as_slice())
            .bind(&payload)
            .execute(&pool)
            .await?;

            let new_stream = |encrypted_key: WrappedKey| NewStream {
                stream_id: group_id,
                election,
                scope: Scope::PoliticalGroup,
                encrypted_key,
            };

            // The first call backfills its fresh key onto the NULL column.
            let first_key = master.wrap_key(&StreamKey::generate(), group_id, election)?;
            assert_eq!(
                ensure_stream(&pool, &new_stream(first_key.clone())).await?,
                first_key
            );

            // Later calls keep the stored key; the new candidate is discarded.
            let second_key = master.wrap_key(&StreamKey::generate(), group_id, election)?;
            assert_eq!(
                ensure_stream(&pool, &new_stream(second_key)).await?,
                first_key
            );

            // The old event was not written under the backfilled key, so it must
            // fail to decrypt instead of silently replaying.
            let store =
                PgStore::new_with_pool_for_stream(pool, group_id, election, &master).await?;
            let err = store
                .load()
                .await
                .expect_err("pre-upgrade events must not decrypt under the backfilled key");
            assert!(matches!(err, AppError::EventDecodeError(_)));

            Ok(())
        }
    }
}
