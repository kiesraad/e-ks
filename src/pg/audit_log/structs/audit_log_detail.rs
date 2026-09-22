use chrono::{DateTime, Utc};

use crate::{
    Event, Locale, PgEvent, PgStoreData,
    audit_log::{AuditLogEntry, AuditLogPath},
    store::StoreEvent,
    structs::audit_log::{ChangeGroup, EntityId, RenderContext, render_groups},
};

/// Detailed view of an audit log event, including field-level changes.
pub struct AuditLogDetail {
    pub event_id: usize,
    pub description: String,
    pub details: String,
    pub subject_id_full: String,
    pub subject_path: String,
    pub created_at: DateTime<Utc>,
    /// The changes, grouped by the part of the entity they belong to.
    pub changes: Vec<ChangeGroup>,
}

impl AuditLogDetail {
    /// The detail view of `event`, whose changes are read against `before`:
    /// the projection as it stood when the event was applied.
    pub fn from_event(before: &PgStoreData, event: &StoreEvent<PgEvent>, locale: Locale) -> Self {
        let changes = event.payload.changes(before);
        let link = |entity: &EntityId| format!("{AuditLogPath}?search={entity}");
        let ctx = RenderContext {
            locale,
            link: Some(&link),
        };
        let entry = AuditLogEntry::new(event.clone(), locale);

        AuditLogDetail {
            event_id: entry.event_id,
            description: entry.description,
            details: entry.details,
            subject_id_full: entry.subject_id_full,
            subject_path: entry.subject_path,
            created_at: entry.created_at,
            changes: render_groups(&changes, &ctx),
        }
    }

    /// Replay `events` on top of `base` up to the target event and build its
    /// detail view. `base` is the imported snapshot in paper-corrections mode,
    /// an empty projection otherwise. `None` if the event id is not found.
    #[cfg(test)]
    pub fn compute(
        base: &PgStoreData,
        events: &[StoreEvent<PgEvent>],
        target_event_id: usize,
        locale: Locale,
    ) -> Option<Self> {
        use crate::store::StoreData;

        let target_index = events.iter().position(|e| e.event_id == target_event_id)?;
        let mut before = base.clone();
        for event in &events[..target_index] {
            before.apply(event.clone());
        }
        Some(Self::from_event(&before, &events[target_index], locale))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::{
        ElectoralDistrict, Locale,
        structs::{
            audit_log::{ChangeRow, RowDetail, RowKind},
            candidate_lists::CandidateListId,
            common::{DutchAddress, FullName, PlaceOfResidence, PreviousElectionResults},
            persons::{PersonId, Representative},
        },
        test_utils::{sample_candidate_list, sample_person, sample_political_group},
    };

    const EN: Locale = Locale::En;

    fn empty_state() -> PgStoreData {
        PgStoreData::default()
    }

    /// All rows across groups, with the group heading they sit under.
    fn rows(detail: &AuditLogDetail) -> Vec<(Option<&str>, &ChangeRow)> {
        detail
            .changes
            .iter()
            .flat_map(|group| {
                group
                    .rows
                    .iter()
                    .map(move |row| (group.label.as_deref(), row))
            })
            .collect()
    }

    fn row<'a>(detail: &'a AuditLogDetail, label: &str) -> &'a ChangeRow {
        rows(detail)
            .into_iter()
            .map(|(_, row)| row)
            .find(|row| row.label == label)
            .unwrap_or_else(|| panic!("no row labelled {label}"))
    }

    /// The texts in the old and new column of a scalar row.
    fn texts(row: &ChangeRow) -> (Vec<&str>, Vec<&str>) {
        match &row.detail {
            RowDetail::Scalar { old, new } => (
                old.iter().map(|c| c.text.as_str()).collect(),
                new.iter().map(|c| c.text.as_str()).collect(),
            ),
            RowDetail::Collection { .. } => panic!("expected a scalar row"),
        }
    }

    #[test]
    fn compute_create_event() {
        let person = sample_person(PersonId::new());
        let events = vec![StoreEvent::new(1, PgEvent::CreatePerson(person))];

        let detail = AuditLogDetail::compute(&empty_state(), &events, 1, EN).unwrap();

        assert_eq!(detail.event_id, 1);
        assert_eq!(detail.description, "Created person");
        assert!(!detail.changes.is_empty());
        for (_, row) in rows(&detail) {
            assert_eq!(row.kind, RowKind::Added);
            assert!(texts(row).0.is_empty());
        }
    }

    #[test]
    fn compute_update_event_shows_diff() {
        let person_id = PersonId::new();
        let person = sample_person(person_id);
        let mut updated_person = person.clone();
        updated_person.name = FullName {
            first_name: Some("Updated".parse().unwrap()),
            ..person.name.clone()
        };

        let events = vec![
            StoreEvent::new(1, PgEvent::CreatePerson(person)),
            StoreEvent::new(2, PgEvent::UpdatePerson(updated_person)),
        ];

        let detail = AuditLogDetail::compute(&empty_state(), &events, 2, EN).unwrap();
        let change = row(&detail, "First name");

        assert_eq!(change.kind, RowKind::Changed);
        assert_eq!(texts(change), (vec!["Henk"], vec!["Updated"]));
    }

    /// Paper-corrections case: the updated person exists only in the base
    /// (imported) state, not in the event stream itself.
    #[test]
    fn compute_update_diffs_against_the_base_state() {
        let person = sample_person(PersonId::new());
        let mut base = PgStoreData::default();
        base.persons.insert(person.id, person.clone());

        let mut updated = person.clone();
        updated.name = FullName {
            first_name: Some("Updated".parse().unwrap()),
            ..person.name.clone()
        };
        let events = vec![StoreEvent::new(2, PgEvent::UpdatePerson(updated))];

        let detail = AuditLogDetail::compute(&base, &events, 2, EN).unwrap();

        assert_eq!(
            texts(row(&detail, "First name")),
            (vec!["Henk"], vec!["Updated"])
        );
    }

    /// Regression test for issue #1157: the diff used to show the raw serde
    /// field name `previous_election_results` and its raw value `zero_seats`.
    #[test]
    fn compute_political_group_update_shows_user_facing_field_and_value() {
        let before = sample_political_group();
        let mut after = before.clone();
        after.previous_election_results = Some(PreviousElectionResults::SixteenOrMoreSeats);

        let events = vec![
            StoreEvent::new(1, PgEvent::UpdatePoliticalGroup(before)),
            StoreEvent::new(2, PgEvent::UpdatePoliticalGroup(after)),
        ];

        let detail = AuditLogDetail::compute(&empty_state(), &events, 2, EN).unwrap();
        let change = row(&detail, "Result of the previous election");

        assert_eq!(
            texts(change),
            (
                vec!["0 seats, or did not participate"],
                vec!["16 or more seats"]
            )
        );
    }

    /// Regression test for issue #1157: the diff used to show the field name
    /// `personal_data.place_of_residence.Known`, leaking the enum variant that
    /// records whether the locality is known in the BAG.
    #[test]
    fn compute_place_of_residence_change_hides_the_bag_variant() {
        let person_id = PersonId::new();
        let mut person = sample_person(person_id);
        person.personal_data.place_of_residence =
            Some(PlaceOfResidence::Unknown("Juinen".to_string()));
        let mut updated = person.clone();
        updated.personal_data.place_of_residence =
            Some(PlaceOfResidence::Known("Utrecht".to_string()));

        let events = vec![
            StoreEvent::new(1, PgEvent::CreatePerson(person)),
            StoreEvent::new(2, PgEvent::UpdatePerson(updated)),
        ];

        let detail = AuditLogDetail::compute(&empty_state(), &events, 2, EN).unwrap();

        assert_eq!(rows(&detail).len(), 1);
        assert_eq!(
            texts(row(&detail, "Place of residence")),
            (vec!["Juinen"], vec!["Utrecht"])
        );
    }

    #[test]
    fn compute_create_person_shows_user_facing_values_in_groups() {
        let person = sample_person(PersonId::new());
        let events = vec![StoreEvent::new(1, PgEvent::CreatePerson(person))];

        let detail = AuditLogDetail::compute(&empty_state(), &events, 1, EN).unwrap();
        let value_of = |label: &str| texts(row(&detail, label)).1;

        assert_eq!(value_of("Gender"), vec!["Female"]);
        assert_eq!(
            value_of("Social security number (BSN)"),
            vec!["Confirmed: no social security number"]
        );
        assert_eq!(value_of("Date of birth"), vec!["01-02-1990"]);
        assert_eq!(value_of("Known in the BAG"), vec!["Yes"]);

        // Personal data sits at the root; the address is its own group.
        let group_of = |label: &str| {
            rows(&detail)
                .into_iter()
                .find(|(_, row)| row.label == label)
                .map(|(group, _)| group)
                .unwrap()
        };
        assert_eq!(group_of("Gender"), None);
        assert_eq!(group_of("Known in the BAG"), Some("Correspondence address"));
    }

    #[test]
    fn compute_representative_update_is_grouped() {
        let person = sample_person(PersonId::new());
        let events = vec![
            StoreEvent::new(1, PgEvent::CreatePerson(person.clone())),
            StoreEvent::new(
                2,
                PgEvent::UpdatePersonRepresentative {
                    person_id: person.id,
                    representative: Some(Representative {
                        name: FullName {
                            last_name: "Bos".parse().unwrap(),
                            initials: "E.".parse().unwrap(),
                            ..Default::default()
                        },
                        address: DutchAddress {
                            locality: Some("Rotterdam".parse().unwrap()),
                            ..Default::default()
                        },
                    }),
                },
            ),
        ];

        let detail = AuditLogDetail::compute(&empty_state(), &events, 2, Locale::Nl).unwrap();

        let headings: Vec<Option<&str>> =
            detail.changes.iter().map(|g| g.label.as_deref()).collect();
        assert_eq!(
            headings,
            vec![
                Some("Gemachtigde"),
                Some("Gemachtigde › Correspondentie\u{ad}adres")
            ]
        );
        assert_eq!(texts(row(&detail, "Achternaam")), (vec![], vec!["Bos"]));
        assert_eq!(
            texts(row(&detail, "Woonplaats")),
            (vec![], vec!["Rotterdam"])
        );
    }

    #[test]
    fn compute_delete_event() {
        let person_id = PersonId::new();
        let person = sample_person(person_id);

        let events = vec![
            StoreEvent::new(1, PgEvent::CreatePerson(person)),
            StoreEvent::new(2, PgEvent::DeletePerson { person_id }),
        ];

        let detail = AuditLogDetail::compute(&empty_state(), &events, 2, EN).unwrap();

        assert!(!detail.changes.is_empty());
        for (_, row) in rows(&detail) {
            assert_eq!(row.kind, RowKind::Removed);
            assert!(texts(row).1.is_empty());
        }
    }

    #[test]
    fn compute_returns_none_for_unknown_event() {
        let events = vec![StoreEvent::new(
            1,
            PgEvent::UpdatePoliticalGroup(sample_political_group()),
        )];
        assert!(AuditLogDetail::compute(&empty_state(), &events, 999, EN).is_none());
    }

    #[test]
    fn compute_returns_none_for_empty_events() {
        let events: Vec<StoreEvent<PgEvent>> = vec![];
        assert!(AuditLogDetail::compute(&empty_state(), &events, 1, EN).is_none());
    }

    #[test]
    fn compute_system_event_has_no_old_state() {
        let list_id = CandidateListId::new();
        let events = vec![StoreEvent::new(
            1,
            PgEvent::ExportCsv {
                file_name: "export.csv".to_string(),
                file_size: 100,
                list_id,
            },
        )];

        let detail = AuditLogDetail::compute(&empty_state(), &events, 1, EN).unwrap();

        assert_eq!(rows(&detail).len(), 3);
        for (_, row) in rows(&detail) {
            assert_eq!(row.kind, RowKind::Added);
        }
        assert_eq!(texts(row(&detail, "File name")).1, vec!["export.csv"]);
    }

    #[test]
    fn compute_district_change_lists_what_entered_and_left() {
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.electoral_districts = BTreeSet::from([ElectoralDistrict::Groningen]);

        let events = vec![
            StoreEvent::new(1, PgEvent::CreateCandidateList(list)),
            StoreEvent::new(
                2,
                PgEvent::UpdateCandidateListDistricts {
                    list_id,
                    electoral_districts: BTreeSet::from([
                        ElectoralDistrict::Groningen,
                        ElectoralDistrict::Fryslan,
                    ]),
                },
            ),
        ];

        let detail = AuditLogDetail::compute(&empty_state(), &events, 2, EN).unwrap();
        let districts = row(&detail, "Electoral districts");

        assert_eq!(districts.kind, RowKind::Collection);
        let RowDetail::Collection {
            removed,
            added,
            moved,
        } = &districts.detail
        else {
            panic!("expected a collection row");
        };
        assert!(removed.is_empty());
        // Fryslân's title carries a diacritic the serde tag ("Fryslan") does not.
        assert_eq!(
            added.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(),
            vec!["Fryslân"]
        );
        assert!(moved.is_empty());
    }

    #[test]
    fn compute_reorder_shows_the_moved_candidate_by_name() {
        let list_id = CandidateListId::new();
        let p1 = sample_person(PersonId::new());
        let p2 = sample_person(PersonId::new());
        let p3 = sample_person(PersonId::new());
        let (p1_id, p2_id, p3_id) = (p1.id, p2.id, p3.id);
        let p1_name = p1.name.display();

        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![p1_id, p2_id, p3_id];

        let events = vec![
            StoreEvent::new(1, PgEvent::CreatePerson(p1)),
            StoreEvent::new(2, PgEvent::CreatePerson(p2)),
            StoreEvent::new(3, PgEvent::CreatePerson(p3)),
            StoreEvent::new(4, PgEvent::CreateCandidateList(list)),
            // Swap positions 1 and 2; position 3 unchanged.
            StoreEvent::new(
                5,
                PgEvent::UpdateCandidateListOrder {
                    list_id,
                    candidates: vec![p2_id, p1_id, p3_id],
                },
            ),
        ];

        let detail = AuditLogDetail::compute(&empty_state(), &events, 5, EN).unwrap();
        let candidates = row(&detail, "Candidates");

        let RowDetail::Collection { moved, .. } = &candidates.detail else {
            panic!("expected a collection row");
        };
        assert_eq!(moved.len(), 1, "a swap is one move: {moved:#?}");
        assert_eq!(moved[0].cell.text, p1_name);
        assert_eq!((moved[0].from, moved[0].to), (1, 2));
        let link = moved[0].cell.entity.as_ref().expect("entity link");
        assert_eq!(link.id_full, p1_id.to_string());
        assert_eq!(
            link.href.as_deref(),
            Some(format!("/audit-log?search={p1_id}").as_str())
        );
    }

    #[test]
    fn compute_removed_candidate_is_described_from_the_state_before() {
        let list_id = CandidateListId::new();
        let person = sample_person(PersonId::new());
        let person_id = person.id;
        let person_name = person.name.display();

        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id];

        let events = vec![
            StoreEvent::new(1, PgEvent::CreatePerson(person)),
            StoreEvent::new(2, PgEvent::CreateCandidateList(list)),
            StoreEvent::new(
                3,
                PgEvent::RemoveCandidateFromCandidateList { list_id, person_id },
            ),
        ];

        let detail = AuditLogDetail::compute(&empty_state(), &events, 3, EN).unwrap();
        let candidates = row(&detail, "Candidates");

        let RowDetail::Collection { removed, added, .. } = &candidates.detail else {
            panic!("expected a collection row");
        };
        assert!(added.is_empty());
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].text, person_name);
    }

    #[test]
    fn compute_create_candidate_list_numbers_the_candidates() {
        let list_id = CandidateListId::new();
        let persons: Vec<_> = (0..12).map(|_| sample_person(PersonId::new())).collect();

        let mut list = sample_candidate_list(list_id);
        list.candidates = persons.iter().map(|p| p.id).collect();

        let mut events: Vec<_> = persons
            .iter()
            .enumerate()
            .map(|(i, p)| StoreEvent::new(i + 1, PgEvent::CreatePerson(p.clone())))
            .collect();
        events.push(StoreEvent::new(
            persons.len() + 1,
            PgEvent::CreateCandidateList(list),
        ));

        let detail =
            AuditLogDetail::compute(&empty_state(), &events, persons.len() + 1, EN).unwrap();
        let candidates = row(&detail, "Candidates");

        let RowDetail::Scalar { new, .. } = &candidates.detail else {
            panic!("expected a scalar row");
        };
        let markers: Vec<&str> = new.iter().filter_map(|c| c.marker.as_deref()).collect();
        let expected: Vec<String> = (1..=12).map(|n| format!("#{n}")).collect();
        assert_eq!(
            markers,
            expected.iter().map(String::as_str).collect::<Vec<_>>()
        );
    }

    #[test]
    fn compute_import_candidates_lists_imported_persons() {
        let list_id = CandidateListId::new();

        let existing = sample_person(PersonId::new());
        let existing_name = existing.name.display();
        let created = sample_person(PersonId::new());
        let created_name = created.name.display();

        let events = vec![
            StoreEvent::new(1, PgEvent::CreatePerson(existing.clone())),
            StoreEvent::new(
                2,
                PgEvent::CreateCandidateList(sample_candidate_list(list_id)),
            ),
            StoreEvent::new(
                3,
                PgEvent::ImportCandidates {
                    list_id,
                    file_name: "candidates.csv".to_string(),
                    file_size: 200,
                    created_persons: vec![created.clone()],
                    updated_persons: vec![existing.clone()],
                    candidates: vec![created.id, existing.id],
                },
            ),
        ];

        let detail = AuditLogDetail::compute(&empty_state(), &events, 3, EN).unwrap();

        assert_eq!(
            texts(row(&detail, "Created candidates")).1,
            vec![created_name.as_str()]
        );
        assert_eq!(
            texts(row(&detail, "Updated candidates")).1,
            vec![existing_name.as_str()]
        );
    }
}
