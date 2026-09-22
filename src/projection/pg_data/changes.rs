//! What each [`PgEvent`] changes, for the audit log.
//!
//! Every event is read against `before`, the projection as it stood when the
//! event was applied. The payload is the new value of whatever the event
//! touches, so no after-state is needed: an update diffs the stored entity
//! against the payload, a create diffs against nothing, a delete diffs the
//! stored entity against nothing. Candidate lists get their candidates
//! resolved to names, and a reorder is reported as the candidates that moved.

use crate::{
    PgEvent, PgStoreData,
    structs::{
        audit_log::{
            AuditValue, Change, EntityId, EntityRef, FieldKey, FieldPath, between, diff, diff_at,
        },
        candidate_lists::{CandidateList, CandidateListId},
        list_submitters::{ListSubmitter, ListSubmitterId},
        persons::{Person, PersonId},
    },
};

impl PgEvent {
    /// Field-level changes this event makes to `before`.
    pub(crate) fn audit_changes(&self, before: &PgStoreData) -> Vec<Change> {
        match self {
            PgEvent::UpdatePoliticalGroup(pg) => diff(Some(&before.political_group), Some(pg)),

            PgEvent::CreatePerson(_)
            | PgEvent::CreatePersonPersonalData { .. }
            | PgEvent::UpdatePerson(_)
            | PgEvent::UpdatePersonPersonalData { .. }
            | PgEvent::UpdatePersonAddress { .. }
            | PgEvent::UpdatePersonRepresentative { .. }
            | PgEvent::DeletePerson { .. } => self.person_changes(before),

            PgEvent::CreateCandidateList(_)
            | PgEvent::UpdateCandidateListDistricts { .. }
            | PgEvent::UpdateCandidateListOrder { .. }
            | PgEvent::AddCandidateToCandidateList { .. }
            | PgEvent::RemoveCandidateFromCandidateList { .. }
            | PgEvent::DeleteCandidateList(_)
            | PgEvent::ImportCandidates { .. } => self.candidate_list_changes(before),

            PgEvent::CreateNameAuthorisation(na) | PgEvent::UpdateNameAuthorisation(na) => {
                diff(before.name_authorisations.get(&na.id), Some(na))
            }
            PgEvent::DeleteNameAuthorisation(id) => diff(before.name_authorisations.get(id), None),

            PgEvent::UpdateListSubmitter(ls) => diff(Some(&before.list_submitter), Some(ls)),
            PgEvent::CreateSubstituteSubmitter(ss) | PgEvent::UpdateSubstituteSubmitter(ss) => {
                diff(substitute(before, ss.id), Some(ss))
            }
            PgEvent::DeleteSubstituteSubmitter {
                substitute_submitter_id,
            } => diff(substitute(before, *substitute_submitter_id), None),

            PgEvent::DeveloperLogin { stream_id } => {
                vec![Change::added(
                    FieldKey::StreamId,
                    AuditValue::text(stream_id),
                )]
            }
            PgEvent::DownloadFile {
                file_name,
                download_path,
            } => vec![
                Change::added(FieldKey::FileName, AuditValue::text(file_name)),
                Change::added(FieldKey::DownloadPath, AuditValue::text(download_path)),
            ],
            PgEvent::ExportCsv {
                file_name,
                file_size,
                list_id,
            } => vec![
                Change::added(FieldKey::FileName, AuditValue::text(file_name)),
                Change::added(FieldKey::FileSize, AuditValue::text(file_size)),
                Change::added(FieldKey::ListId, list_ref(before, *list_id)),
            ],
            // `before` is the imported snapshot the paper corrections start from.
            PgEvent::Import { .. } => before.import_summary(),
            PgEvent::Login | PgEvent::Logout | PgEvent::HideDownloadWarning => Vec::new(),
        }
    }

    fn person_changes(&self, before: &PgStoreData) -> Vec<Change> {
        let person = |id: &PersonId| before.persons.get(id);
        match self {
            PgEvent::CreatePerson(p) | PgEvent::UpdatePerson(p) => diff(person(&p.id), Some(p)),
            PgEvent::CreatePersonPersonalData {
                person_id,
                name,
                personal_data,
            }
            | PgEvent::UpdatePersonPersonalData {
                person_id,
                name,
                personal_data,
            } => {
                let existing = person(person_id);
                let after = Person {
                    id: *person_id,
                    name: name.clone(),
                    personal_data: personal_data.clone(),
                    ..existing.cloned().unwrap_or_default()
                };
                diff(existing, Some(&after))
            }
            PgEvent::UpdatePersonAddress { person_id, address } => diff_at(
                FieldPath::from(FieldKey::Address),
                person(person_id).map(|p| &p.address),
                Some(address),
            ),
            PgEvent::UpdatePersonRepresentative {
                person_id,
                representative,
            } => diff_at(
                FieldPath::from(FieldKey::Representative),
                person(person_id).and_then(|p| p.representative.as_ref()),
                representative.as_ref(),
            ),
            PgEvent::DeletePerson { person_id } => diff(person(person_id), None),
            _ => Vec::new(),
        }
    }

    fn candidate_list_changes(&self, before: &PgStoreData) -> Vec<Change> {
        let resolve = |id: PersonId| person_ref(before, id);
        let list = |id: &CandidateListId| before.candidate_lists.get(id);
        // The list after the event: the stored list with one field replaced.
        let updated = |id: &CandidateListId, update: &dyn Fn(&mut CandidateList)| {
            list(id).cloned().map(|mut list| {
                update(&mut list);
                list
            })
        };
        match self {
            PgEvent::CreateCandidateList(cl) => list_diff(list(&cl.id), Some(cl), &resolve),
            PgEvent::UpdateCandidateListDistricts {
                list_id,
                electoral_districts,
            } => {
                let new = updated(list_id, &|l| {
                    l.electoral_districts = electoral_districts.clone();
                });
                list_diff(list(list_id), new.as_ref(), &resolve)
            }
            PgEvent::UpdateCandidateListOrder {
                list_id,
                candidates,
            } => {
                let new = updated(list_id, &|l| l.candidates = candidates.clone());
                list_diff(list(list_id), new.as_ref(), &resolve)
            }
            PgEvent::AddCandidateToCandidateList { list_id, person_id } => {
                let new = updated(list_id, &|l| {
                    if !l.candidates.contains(person_id) {
                        l.candidates.push(*person_id);
                    }
                });
                list_diff(list(list_id), new.as_ref(), &resolve)
            }
            PgEvent::RemoveCandidateFromCandidateList { list_id, person_id } => {
                let new = updated(list_id, &|l| l.candidates.retain(|id| id != person_id));
                list_diff(list(list_id), new.as_ref(), &resolve)
            }
            PgEvent::DeleteCandidateList(id) => list_diff(list(id), None, &resolve),
            PgEvent::ImportCandidates { .. } => self.import_candidates_changes(before),
            _ => Vec::new(),
        }
    }

    fn import_candidates_changes(&self, before: &PgStoreData) -> Vec<Change> {
        let PgEvent::ImportCandidates {
            list_id,
            file_name,
            file_size,
            created_persons,
            updated_persons,
            candidates,
        } = self
        else {
            return Vec::new();
        };

        // Imported persons are not in `before` yet: resolve them from the payload.
        let imported = |id: PersonId| {
            created_persons
                .iter()
                .chain(updated_persons)
                .find(|p| p.id == id)
        };
        let resolve =
            |id: PersonId| imported(id).map_or_else(|| person_ref(before, id), person_ref_of);
        let refs =
            |persons: &[Person]| AuditValue::Set(persons.iter().map(person_ref_of).collect());

        let mut changes = vec![
            Change::added(FieldKey::FileName, AuditValue::text(file_name)),
            Change::added(FieldKey::FileSize, AuditValue::text(file_size)),
            Change::added(FieldKey::ListId, list_ref(before, *list_id)),
        ];
        changes.extend(between(
            FieldPath::from(FieldKey::CreatedPersons),
            AuditValue::Missing,
            refs(created_persons),
        ));
        changes.extend(between(
            FieldPath::from(FieldKey::UpdatedPersons),
            AuditValue::Missing,
            refs(updated_persons),
        ));

        let old = before.candidate_lists.get(list_id);
        let new = old.cloned().map(|mut list| {
            list.candidates = candidates.clone();
            list
        });
        changes.extend(list_diff(old, new.as_ref(), &resolve));
        changes
    }
}

impl PgStoreData {
    /// What an imported package holds, for the import event that starts a
    /// paper-corrections log. A snapshot is never diffed field by field.
    pub(crate) fn import_summary(&self) -> Vec<Change> {
        vec![
            Change::added(FieldKey::Persons, AuditValue::text(self.persons.len())),
            Change::added(
                FieldKey::CandidateLists,
                AuditValue::text(self.candidate_lists.len()),
            ),
            Change::added(
                FieldKey::NameAuthorisations,
                AuditValue::text(self.name_authorisations.len()),
            ),
            Change::added(
                FieldKey::SubstituteSubmitters,
                AuditValue::text(self.substitute_submitters.len()),
            ),
        ]
    }
}

/// The changes between two states of a candidate list, its candidates
/// resolved to names so a reorder reads as "who moved where".
fn list_diff(
    old: Option<&CandidateList>,
    new: Option<&CandidateList>,
    resolve: &dyn Fn(PersonId) -> AuditValue,
) -> Vec<Change> {
    // A list that does not exist has no candidates field at all; an existing
    // list without candidates has an empty one, so emptying or first filling a
    // list reads as a collection change.
    let candidates = |list: Option<&CandidateList>| {
        list.map_or(AuditValue::Missing, |l| {
            AuditValue::Ordered(l.candidates.iter().map(|id| resolve(*id)).collect())
        })
    };
    let mut changes = diff(old, new);
    changes.extend(between(
        FieldPath::from(FieldKey::Candidates),
        candidates(old),
        candidates(new),
    ));
    changes
}

fn substitute(data: &PgStoreData, id: ListSubmitterId) -> Option<&ListSubmitter> {
    data.substitute_submitters.iter().find(|s| s.id == id)
}

fn entity(id: EntityId, description: Option<String>) -> AuditValue {
    AuditValue::Entity(EntityRef {
        id,
        description: description.unwrap_or_default(),
    })
}

/// A reference to a person, described by the name they had in `data`.
fn person_ref(data: &PgStoreData, id: PersonId) -> AuditValue {
    entity(
        EntityId::Person(id),
        data.persons.get(&id).map(|p| p.name.display()),
    )
}

fn person_ref_of(person: &Person) -> AuditValue {
    entity(EntityId::Person(person.id), Some(person.name.display()))
}

/// A reference to a candidate list, described by its districts.
fn list_ref(data: &PgStoreData, id: CandidateListId) -> AuditValue {
    entity(
        EntityId::CandidateList(id),
        data.candidate_lists
            .get(&id)
            .map(CandidateList::districts_name),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::{
        ElectoralDistrict, Locale, StreamId,
        store::StoreData,
        structs::{
            audit_log::{ChangeKind, EnumValue, Move, fields_of},
            common::{
                Address, BsnOrNoneConfirmed, DutchAddress, FullName, Gender, PlaceOfResidence,
                PreviousElectionResults,
            },
            csb::{OmissionCategory, sample_omission, sample_registered_political_group},
            list_submitters::ListSubmitterId,
            name_authorisations::NameAuthorisationId,
            persons::Representative,
        },
        test_utils::{
            sample_candidate_list, sample_list_submitter, sample_name_authorisation, sample_person,
            sample_political_group,
        },
    };

    fn state_with(events: Vec<PgEvent>) -> PgStoreData {
        let mut data = PgStoreData::default();
        for (index, event) in events.into_iter().enumerate() {
            data.apply(crate::store::StoreEvent::new(index + 1, event));
        }
        data
    }

    fn change_at(changes: &[Change], path: FieldPath) -> &Change {
        changes
            .iter()
            .find(|c| c.path == path)
            .unwrap_or_else(|| panic!("no change at {path:?} in {changes:#?}"))
    }

    fn key(key: FieldKey) -> FieldPath {
        FieldPath::from(key)
    }

    fn text(value: &str) -> AuditValue {
        AuditValue::text(value)
    }

    #[test]
    fn create_person_adds_every_filled_field_with_typed_values() {
        let person = sample_person(PersonId::new());
        let changes = PgEvent::CreatePerson(person).audit_changes(&PgStoreData::default());

        assert!(
            changes
                .iter()
                .all(|c| matches!(c.kind, ChangeKind::Added(_)))
        );
        assert_eq!(
            change_at(&changes, key(FieldKey::Gender)).kind,
            ChangeKind::Added(AuditValue::Enum(EnumValue::Gender(Gender::Female)))
        );
        assert_eq!(
            change_at(&changes, key(FieldKey::Bsn)).kind,
            ChangeKind::Added(AuditValue::Enum(EnumValue::BsnNoneConfirmed))
        );
        assert_eq!(
            change_at(&changes, key(FieldKey::DateOfBirth)).kind,
            ChangeKind::Added(AuditValue::Date(
                chrono::NaiveDate::from_ymd_opt(1990, 2, 1).unwrap()
            ))
        );
        assert_eq!(
            change_at(&changes, key(FieldKey::Address).with(FieldKey::KnownInBag)).kind,
            ChangeKind::Added(AuditValue::Bool(true))
        );
        // Ids and timestamps are not fields.
        assert!(
            changes
                .iter()
                .all(|c| c.path.leaf() != Some(FieldKey::StreamId))
        );
    }

    #[test]
    fn update_person_reports_only_the_changed_field() {
        let person = sample_person(PersonId::new());
        let before = state_with(vec![PgEvent::CreatePerson(person.clone())]);
        let mut updated = person.clone();
        updated.name.first_name = Some("Updated".parse().unwrap());

        let changes = PgEvent::UpdatePerson(updated).audit_changes(&before);

        assert_eq!(
            changes,
            vec![Change {
                path: key(FieldKey::FirstName),
                kind: ChangeKind::Changed {
                    old: text("Henk"),
                    new: text("Updated"),
                },
            }]
        );
    }

    #[test]
    fn unchanged_update_reports_nothing() {
        let person = sample_person(PersonId::new());
        let before = state_with(vec![PgEvent::CreatePerson(person.clone())]);

        assert!(
            PgEvent::UpdatePerson(person)
                .audit_changes(&before)
                .is_empty()
        );
    }

    /// Regression for issue #1157: the variant that records the BAG lookup
    /// (`Known` / `Unknown`) is not a field.
    #[test]
    fn place_of_residence_change_hides_the_bag_variant() {
        let mut person = sample_person(PersonId::new());
        person.personal_data.place_of_residence = Some(PlaceOfResidence::Unknown("Juinen".into()));
        let before = state_with(vec![PgEvent::CreatePerson(person.clone())]);
        let mut updated = person.clone();
        updated.personal_data.place_of_residence = Some(PlaceOfResidence::Known("Utrecht".into()));

        let changes = PgEvent::UpdatePerson(updated).audit_changes(&before);

        assert_eq!(
            changes,
            vec![Change {
                path: key(FieldKey::PlaceOfResidence),
                kind: ChangeKind::Changed {
                    old: text("Juinen"),
                    new: text("Utrecht"),
                },
            }]
        );
    }

    #[test]
    fn address_update_is_grouped_under_the_address() {
        let person = sample_person(PersonId::new());
        let before = state_with(vec![PgEvent::CreatePerson(person.clone())]);
        let mut address = person.address.clone();
        address.locality = Some("Nieuwegein".parse().unwrap());
        address.house_number_addition = None;

        let changes = PgEvent::UpdatePersonAddress {
            person_id: person.id,
            address,
        }
        .audit_changes(&before);

        // Rows follow the address's field order.
        assert_eq!(
            changes,
            vec![
                Change::removed(
                    key(FieldKey::Address).with(FieldKey::HouseNumberAddition),
                    text("A")
                ),
                Change {
                    path: key(FieldKey::Address).with(FieldKey::Locality),
                    kind: ChangeKind::Changed {
                        old: text("Juinen"),
                        new: text("Nieuwegein"),
                    },
                },
            ]
        );
    }

    #[test]
    fn representative_fields_are_grouped_under_the_representative() {
        let person = sample_person(PersonId::new());
        let before = state_with(vec![PgEvent::CreatePerson(person.clone())]);
        let representative = Representative {
            name: FullName {
                last_name: "Bos".parse().unwrap(),
                initials: "E.".parse().unwrap(),
                ..Default::default()
            },
            address: DutchAddress::default(),
        };

        let changes = PgEvent::UpdatePersonRepresentative {
            person_id: person.id,
            representative: Some(representative),
        }
        .audit_changes(&before);

        assert_eq!(
            changes,
            vec![
                Change::added(
                    key(FieldKey::Representative).with(FieldKey::LastName),
                    text("Bos")
                ),
                Change::added(
                    key(FieldKey::Representative).with(FieldKey::Initials),
                    text("E.")
                ),
            ]
        );
    }

    #[test]
    fn delete_person_removes_every_field() {
        let person = sample_person(PersonId::new());
        let before = state_with(vec![PgEvent::CreatePerson(person.clone())]);

        let changes = PgEvent::DeletePerson {
            person_id: person.id,
        }
        .audit_changes(&before);

        assert!(!changes.is_empty());
        assert!(
            changes
                .iter()
                .all(|c| matches!(c.kind, ChangeKind::Removed(_)))
        );
    }

    #[test]
    fn personal_data_update_diffs_against_the_stored_person() {
        let person = sample_person(PersonId::new());
        let before = state_with(vec![PgEvent::CreatePerson(person.clone())]);
        let mut personal_data = person.personal_data.clone();
        personal_data.gender = Some(Gender::Male);

        let changes = PgEvent::UpdatePersonPersonalData {
            person_id: person.id,
            name: person.name.clone(),
            personal_data,
        }
        .audit_changes(&before);

        assert_eq!(
            changes,
            vec![Change {
                path: key(FieldKey::Gender),
                kind: ChangeKind::Changed {
                    old: AuditValue::Enum(EnumValue::Gender(Gender::Female)),
                    new: AuditValue::Enum(EnumValue::Gender(Gender::Male)),
                },
            }]
        );
    }

    /// Regression for issue #1157: the value used to be the serde tag
    /// `zero_seats`; it is now the enum, translated when rendered.
    #[test]
    fn political_group_update_keeps_enum_values_typed() {
        let group = sample_political_group();
        let before = state_with(vec![PgEvent::UpdatePoliticalGroup(group.clone())]);
        let mut updated = group;
        updated.previous_election_results = Some(PreviousElectionResults::SixteenOrMoreSeats);

        let changes = PgEvent::UpdatePoliticalGroup(updated).audit_changes(&before);

        assert_eq!(
            changes,
            vec![Change {
                path: key(FieldKey::PreviousElectionResults),
                kind: ChangeKind::Changed {
                    old: AuditValue::Enum(EnumValue::PreviousElectionResults(
                        PreviousElectionResults::ZeroSeats
                    )),
                    new: AuditValue::Enum(EnumValue::PreviousElectionResults(
                        PreviousElectionResults::SixteenOrMoreSeats
                    )),
                },
            }]
        );
    }

    #[test]
    fn district_update_reports_what_entered_and_left() {
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.electoral_districts = BTreeSet::from([ElectoralDistrict::Groningen]);
        let before = state_with(vec![PgEvent::CreateCandidateList(list)]);

        let changes = PgEvent::UpdateCandidateListDistricts {
            list_id,
            electoral_districts: BTreeSet::from([ElectoralDistrict::Fryslan]),
        }
        .audit_changes(&before);

        assert_eq!(
            changes,
            vec![Change {
                path: key(FieldKey::ElectoralDistricts),
                kind: ChangeKind::Collection {
                    added: vec![text("Fryslân")],
                    removed: vec![text("Groningen")],
                    moved: Vec::new(),
                },
            }]
        );
    }

    fn list_with_candidates(persons: &[Person]) -> (CandidateListId, PgStoreData) {
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = persons.iter().map(|p| p.id).collect();
        let mut events: Vec<PgEvent> = persons
            .iter()
            .map(|p| PgEvent::CreatePerson(p.clone()))
            .collect();
        events.push(PgEvent::CreateCandidateList(list));
        (list_id, state_with(events))
    }

    fn named_person(last_name: &str) -> Person {
        let mut person = sample_person(PersonId::new());
        person.name.last_name = last_name.parse().unwrap();
        person
    }

    #[test]
    fn reorder_reports_only_the_candidate_that_moved() {
        let persons = [named_person("Aa"), named_person("Bb"), named_person("Cc")];
        let (list_id, before) = list_with_candidates(&persons);

        let changes = PgEvent::UpdateCandidateListOrder {
            list_id,
            candidates: vec![persons[2].id, persons[0].id, persons[1].id],
        }
        .audit_changes(&before);

        assert_eq!(
            changes,
            vec![Change {
                path: key(FieldKey::Candidates),
                kind: ChangeKind::Collection {
                    added: Vec::new(),
                    removed: Vec::new(),
                    moved: vec![Move {
                        item: person_ref_of(&persons[2]),
                        from: 3,
                        to: 1,
                    }],
                },
            }]
        );
    }

    #[test]
    fn add_and_remove_candidate_report_the_named_candidate() {
        let persons = [named_person("Aa"), named_person("Bb")];
        let (list_id, before) = list_with_candidates(&persons[..1]);
        // The second person exists but is not on the list yet.
        let mut before = before;
        before.apply(crate::store::StoreEvent::new(
            9,
            PgEvent::CreatePerson(persons[1].clone()),
        ));

        let added = PgEvent::AddCandidateToCandidateList {
            list_id,
            person_id: persons[1].id,
        }
        .audit_changes(&before);
        assert_eq!(
            added[0].kind,
            ChangeKind::Collection {
                added: vec![person_ref_of(&persons[1])],
                removed: Vec::new(),
                moved: Vec::new(),
            }
        );

        let removed = PgEvent::RemoveCandidateFromCandidateList {
            list_id,
            person_id: persons[0].id,
        }
        .audit_changes(&before);
        assert_eq!(
            removed[0].kind,
            ChangeKind::Collection {
                added: Vec::new(),
                removed: vec![person_ref_of(&persons[0])],
                moved: Vec::new(),
            }
        );

        // Adding a candidate that is already on the list changes nothing,
        // like `apply` ignores it.
        assert!(
            PgEvent::AddCandidateToCandidateList {
                list_id,
                person_id: persons[0].id,
            }
            .audit_changes(&before)
            .is_empty()
        );
    }

    #[test]
    fn create_list_with_candidates_adds_them_in_order() {
        let persons = [named_person("Aa"), named_person("Bb")];
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = persons.iter().map(|p| p.id).collect();
        let before = state_with(
            persons
                .iter()
                .map(|p| PgEvent::CreatePerson(p.clone()))
                .collect(),
        );

        let changes = PgEvent::CreateCandidateList(list).audit_changes(&before);

        assert_eq!(
            change_at(&changes, key(FieldKey::Candidates)).kind,
            ChangeKind::Added(AuditValue::Ordered(
                persons.iter().map(person_ref_of).collect()
            ))
        );
    }

    #[test]
    fn deleted_list_removes_districts_and_candidates() {
        let persons = [named_person("Aa")];
        let (list_id, before) = list_with_candidates(&persons);

        let changes = PgEvent::DeleteCandidateList(list_id).audit_changes(&before);

        assert!(matches!(
            change_at(&changes, key(FieldKey::ElectoralDistricts)).kind,
            ChangeKind::Removed(AuditValue::Set(_))
        ));
        assert_eq!(
            change_at(&changes, key(FieldKey::Candidates)).kind,
            ChangeKind::Removed(AuditValue::Ordered(vec![person_ref_of(&persons[0])]))
        );
    }

    #[test]
    fn import_candidates_resolves_imported_persons_from_the_payload() {
        let existing = named_person("Existing");
        let (list_id, before) = list_with_candidates(&[]);
        let mut before = before;
        before.apply(crate::store::StoreEvent::new(
            9,
            PgEvent::CreatePerson(existing.clone()),
        ));
        let created = named_person("Created");
        let mut renamed = existing.clone();
        renamed.name.last_name = "Renamed".parse().unwrap();

        let changes = PgEvent::ImportCandidates {
            list_id,
            file_name: "candidates.csv".to_string(),
            file_size: 200,
            created_persons: vec![created.clone()],
            updated_persons: vec![renamed.clone()],
            candidates: vec![created.id, renamed.id],
        }
        .audit_changes(&before);

        assert_eq!(
            change_at(&changes, key(FieldKey::FileName)).kind,
            ChangeKind::Added(text("candidates.csv"))
        );
        assert_eq!(
            change_at(&changes, key(FieldKey::CreatedPersons)).kind,
            ChangeKind::Added(AuditValue::Set(vec![person_ref_of(&created)]))
        );
        assert_eq!(
            change_at(&changes, key(FieldKey::UpdatedPersons)).kind,
            ChangeKind::Added(AuditValue::Set(vec![person_ref_of(&renamed)]))
        );
        // The list existed without candidates: the imported ones join it,
        // described with their imported names.
        assert_eq!(
            change_at(&changes, key(FieldKey::Candidates)).kind,
            ChangeKind::Collection {
                added: vec![person_ref_of(&created), person_ref_of(&renamed)],
                removed: Vec::new(),
                moved: Vec::new(),
            }
        );
    }

    #[test]
    fn name_authorisation_and_submitter_events_diff_the_stored_entity() {
        let na = sample_name_authorisation(NameAuthorisationId::new());
        let ss = sample_list_submitter(ListSubmitterId::new());
        let before = state_with(vec![
            PgEvent::CreateNameAuthorisation(na.clone()),
            PgEvent::CreateSubstituteSubmitter(ss.clone()),
        ]);

        let mut renamed = na.clone();
        renamed.legal_name = "Nieuwe Partij".parse().unwrap();
        assert_eq!(
            PgEvent::UpdateNameAuthorisation(renamed).audit_changes(&before),
            vec![Change {
                path: key(FieldKey::LegalName),
                kind: ChangeKind::Changed {
                    old: text("Kiesraad Demo Partij"),
                    new: text("Nieuwe Partij"),
                },
            }]
        );

        let deleted = PgEvent::DeleteSubstituteSubmitter {
            substitute_submitter_id: ss.id,
        }
        .audit_changes(&before);
        assert_eq!(
            change_at(&deleted, key(FieldKey::Address).with(FieldKey::Locality)).kind,
            ChangeKind::Removed(text("Rotterdam"))
        );

        let submitter = sample_list_submitter(ListSubmitterId::new());
        let changes =
            PgEvent::UpdateListSubmitter(submitter).audit_changes(&PgStoreData::default());
        assert_eq!(
            change_at(&changes, key(FieldKey::LastName)).kind,
            ChangeKind::Added(text("Bos"))
        );
    }

    #[test]
    fn system_events_list_their_payload_or_nothing() {
        let before = PgStoreData::default();
        let list_id = CandidateListId::new();

        let export = PgEvent::ExportCsv {
            file_name: "export.csv".to_string(),
            file_size: 100,
            list_id,
        }
        .audit_changes(&before);
        assert_eq!(export.len(), 3);
        assert!(
            export
                .iter()
                .all(|c| matches!(c.kind, ChangeKind::Added(_)))
        );
        assert_eq!(
            change_at(&export, key(FieldKey::ListId)).kind,
            ChangeKind::Added(AuditValue::Entity(EntityRef {
                id: EntityId::CandidateList(list_id),
                description: String::new(),
            }))
        );

        let stream_id = StreamId::new();
        assert_eq!(
            PgEvent::DeveloperLogin { stream_id }.audit_changes(&before),
            vec![Change::added(
                FieldKey::StreamId,
                AuditValue::text(stream_id)
            )]
        );
        assert!(PgEvent::Login.audit_changes(&before).is_empty());
        assert!(PgEvent::Logout.audit_changes(&before).is_empty());
        assert!(
            PgEvent::HideDownloadWarning
                .audit_changes(&before)
                .is_empty()
        );
    }

    #[test]
    fn import_summarises_the_imported_snapshot() {
        let before = state_with(vec![
            PgEvent::CreatePerson(sample_person(PersonId::new())),
            PgEvent::CreatePerson(sample_person(PersonId::new())),
            PgEvent::CreateCandidateList(sample_candidate_list(CandidateListId::new())),
        ]);

        let changes = PgEvent::Import { hash: [1; 32] }.audit_changes(&before);

        assert_eq!(
            change_at(&changes, key(FieldKey::Persons)).kind,
            ChangeKind::Added(text("2"))
        );
        assert_eq!(
            change_at(&changes, key(FieldKey::CandidateLists)).kind,
            ChangeKind::Added(text("1"))
        );
    }

    /// Every field of every audited entity renders as a real label, in both
    /// locales, and no two fields of one entity share a label within a group.
    /// This is what keeps the raw serde paths of issue #1157 out of the page.
    #[test]
    fn every_field_of_every_entity_has_a_distinct_label() {
        let mut person = sample_person(PersonId::new());
        person.representative = Some(Representative {
            name: FullName::default(),
            address: DutchAddress::default(),
        });
        let mut international = sample_list_submitter(ListSubmitterId::new());
        international.address = Address::International(Default::default());
        let mut bsn_person = sample_person(PersonId::new());
        bsn_person.personal_data.bsn = Some(BsnOrNoneConfirmed::Bsn("999995972".parse().unwrap()));

        let entities: Vec<Vec<(FieldPath, AuditValue)>> = vec![
            fields_of(Some(&person)),
            fields_of(Some(&bsn_person)),
            fields_of(Some(&sample_list_submitter(ListSubmitterId::new()))),
            fields_of(Some(&international)),
            fields_of(Some(&sample_candidate_list(CandidateListId::new()))),
            fields_of(Some(&sample_political_group())),
            fields_of(Some(&sample_name_authorisation(NameAuthorisationId::new()))),
            fields_of(Some(&sample_omission(OmissionCategory::PoliticalGroup))),
            fields_of(Some(&sample_registered_political_group("Partij", 10, 1))),
        ];

        for fields in entities {
            assert!(!fields.is_empty());
            for locale in [Locale::Nl, Locale::En] {
                let mut labels = Vec::new();
                for (path, _) in &fields {
                    let label = format!(
                        "{}{}",
                        path.group_label(locale).unwrap_or_default(),
                        path.leaf_label(locale)
                    );
                    assert!(
                        !label.is_empty() && !label.contains('[') && !label.contains('_'),
                        "raw or missing label for {path:?}: {label}"
                    );
                    assert!(!labels.contains(&label), "duplicate label {label}");
                    labels.push(label);
                }
            }
        }
    }
}
