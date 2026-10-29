use crate::{
    Locale, PgEvent, PgStoreData,
    audit_log::AuditLogEntry,
    store::StoreEvent,
    structs::audit_log::{AuditEvent, ChangeGroup, RenderContext, SubjectLinks, render_groups},
};

/// Detailed view of an audit log event, including field-level changes.
pub struct AuditLogDetail {
    pub entry: AuditLogEntry,
    /// The changes, grouped by the part of the entity they belong to.
    pub changes: Vec<ChangeGroup>,
}

impl AuditLogDetail {
    /// The detail view of `event`, whose changes are read against `before`:
    /// the projection as it stood when the event was applied.
    pub fn from_event(
        before: &PgStoreData,
        event: &StoreEvent<PgEvent>,
        locale: Locale,
        links: &dyn SubjectLinks,
    ) -> Self {
        let changes = event.payload.changes(before);
        let entry = AuditLogEntry::new(event, locale, links);

        AuditLogDetail {
            entry,
            changes: render_groups(&changes, &RenderContext { locale, links }),
        }
    }

    /// The detail view of event `target_event_id` of `events`, linking
    /// against the state after all of them. `None` if the id is not found.
    #[cfg(test)]
    pub fn compute(
        events: &[StoreEvent<PgEvent>],
        target_event_id: usize,
        locale: Locale,
    ) -> Option<Self> {
        use crate::{StreamId, audit_log::PgLinks};

        let target = events.iter().find(|e| e.event_id == target_event_id)?;
        let before = PgStoreData::snapshot_until(events, target_event_id - 1);
        let now = PgStoreData::snapshot_until(events, usize::MAX);
        let links = PgLinks {
            stream_id: StreamId::new(),
            data: &now,
        };
        Some(Self::from_event(&before, target, locale, &links))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::{
        ElectoralDistrict, Locale,
        structs::{
            audit_log::{Cell, ChangeRow, Highlight, RowDetail, RowKind},
            candidate_lists::CandidateListId,
            common::{DutchAddress, FullName, PlaceOfResidence, PreviousElectionResults},
            persons::{PersonId, Representative},
        },
        test_utils::{sample_candidate_list, sample_person, sample_political_group},
    };

    const EN: Locale = Locale::En;

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
        let events = vec![StoreEvent::new(1, PgEvent::CreatePerson(Box::new(person)))];

        let detail = AuditLogDetail::compute(&events, 1, EN).unwrap();

        assert_eq!(detail.entry.event_id, 1);
        assert_eq!(detail.entry.description, "Created person");
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
            StoreEvent::new(1, PgEvent::CreatePerson(Box::new(person))),
            StoreEvent::new(2, PgEvent::UpdatePerson(Box::new(updated_person))),
        ];

        let detail = AuditLogDetail::compute(&events, 2, EN).unwrap();
        let change = row(&detail, "First name");

        assert_eq!(change.kind, RowKind::Changed);
        assert_eq!(texts(change), (vec!["Henk"], vec!["Updated"]));
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

        let detail = AuditLogDetail::compute(&events, 2, EN).unwrap();
        let change = row(&detail, "Result of the previous election");

        assert_eq!(
            texts(change),
            (
                vec!["0 seats, or did not participate"],
                vec!["16 or more seats"]
            )
        );
    }

    /// Rebinding a standalone list to another registration is a change of
    /// its own row, while the unchanged list type stays out of the diff.
    #[test]
    fn compute_political_group_update_shows_a_rebinding() {
        use crate::structs::{
            csb::RegisteredPoliticalGroupId, list_designation::ListDesignation,
            political_groups::PoliticalGroup,
        };
        let bound = |id| {
            Some(ListDesignation::Standalone {
                registration: Some(id),
            })
        };
        let before = PoliticalGroup {
            list_designation: bound(RegisteredPoliticalGroupId::new()),
            ..sample_political_group()
        };
        let mut after = before.clone();
        after.list_designation = bound(RegisteredPoliticalGroupId::new());

        let events = vec![
            StoreEvent::new(1, PgEvent::UpdatePoliticalGroup(before)),
            StoreEvent::new(2, PgEvent::UpdatePoliticalGroup(after)),
        ];

        let detail = AuditLogDetail::compute(&events, 2, EN).unwrap();
        assert_eq!(rows(&detail).len(), 1);
        let (removed, added) = texts(row(&detail, "Registration"));
        assert_eq!(removed.len(), 1);
        assert_eq!(added.len(), 1);
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
            StoreEvent::new(1, PgEvent::CreatePerson(Box::new(person))),
            StoreEvent::new(2, PgEvent::UpdatePerson(Box::new(updated))),
        ];

        let detail = AuditLogDetail::compute(&events, 2, EN).unwrap();

        assert_eq!(rows(&detail).len(), 1);
        assert_eq!(
            texts(row(&detail, "Place of residence")),
            (vec!["Juinen"], vec!["Utrecht"])
        );
    }

    #[test]
    fn compute_create_person_shows_user_facing_values_in_groups() {
        let person = sample_person(PersonId::new());
        let events = vec![StoreEvent::new(1, PgEvent::CreatePerson(Box::new(person)))];

        let detail = AuditLogDetail::compute(&events, 1, EN).unwrap();
        let value_of = |label: &str| texts(row(&detail, label)).1;

        assert_eq!(value_of("Gender"), vec!["Female"]);
        assert_eq!(
            value_of("Citizen service number (BSN)"),
            vec!["Confirmed: no citizen service number"]
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
            StoreEvent::new(1, PgEvent::CreatePerson(Box::new(person.clone()))),
            StoreEvent::new(
                2,
                PgEvent::UpdatePersonRepresentative {
                    person_id: person.id,
                    representative: Some(Box::new(Representative {
                        name: FullName {
                            last_name: "Bos".parse().unwrap(),
                            initials: Some("E.".parse().unwrap()),
                            ..Default::default()
                        },
                        address: DutchAddress {
                            locality: Some("Rotterdam".parse().unwrap()),
                            ..Default::default()
                        },
                    })),
                },
            ),
        ];

        let detail = AuditLogDetail::compute(&events, 2, Locale::Nl).unwrap();

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
            StoreEvent::new(1, PgEvent::CreatePerson(Box::new(person))),
            StoreEvent::new(2, PgEvent::DeletePerson { person_id }),
        ];

        let detail = AuditLogDetail::compute(&events, 2, EN).unwrap();

        assert!(!detail.changes.is_empty());
        for (_, row) in rows(&detail) {
            assert_eq!(row.kind, RowKind::Removed);
            assert!(texts(row).1.is_empty());
        }
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

        let detail = AuditLogDetail::compute(&events, 1, EN).unwrap();

        assert_eq!(rows(&detail).len(), 3);
        for (_, row) in rows(&detail) {
            assert_eq!(row.kind, RowKind::Added);
        }
        assert_eq!(texts(row(&detail, "File name")).1, vec!["export.csv"]);
    }

    #[test]
    fn compute_district_change_shows_both_sides_and_marks_what_entered() {
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.electoral_districts = BTreeSet::from([ElectoralDistrict::Groningen]);

        let events = vec![
            StoreEvent::new(1, PgEvent::CreateCandidateList(Box::new(list))),
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

        let detail = AuditLogDetail::compute(&events, 2, EN).unwrap();
        let districts = row(&detail, "Electoral districts");

        assert_eq!(districts.kind, RowKind::Changed);
        let RowDetail::Scalar { old, new } = &districts.detail else {
            panic!("expected a scalar row");
        };
        fn summary(cells: &[Cell]) -> Vec<(&str, Option<Highlight>)> {
            cells
                .iter()
                .map(|c| (c.text.as_str(), c.highlight))
                .collect()
        }
        assert_eq!(summary(old), vec![("Groningen", None)]);
        // Fryslân's title carries a diacritic the serde tag ("Fryslan") does not.
        assert_eq!(
            summary(new),
            vec![("Groningen", None), ("Fryslân", Some(Highlight::Added))]
        );
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
            StoreEvent::new(1, PgEvent::CreatePerson(Box::new(p1))),
            StoreEvent::new(2, PgEvent::CreatePerson(Box::new(p2))),
            StoreEvent::new(3, PgEvent::CreatePerson(Box::new(p3))),
            StoreEvent::new(4, PgEvent::CreateCandidateList(Box::new(list))),
            // Swap positions 1 and 2; position 3 unchanged.
            StoreEvent::new(
                5,
                PgEvent::UpdateCandidateListOrder {
                    list_id,
                    candidates: vec![p2_id, p1_id, p3_id],
                },
            ),
        ];

        let detail = AuditLogDetail::compute(&events, 5, EN).unwrap();
        let candidates = row(&detail, "Candidates");

        let RowDetail::Collection { moved, .. } = &candidates.detail else {
            panic!("expected a collection row");
        };
        assert_eq!(moved.len(), 1, "a swap is one move: {moved:#?}");
        assert_eq!(moved[0].cell.text, p1_name);
        assert_eq!((moved[0].from, moved[0].to), (1, 2));
        let link = moved[0].cell.entity.as_ref().expect("entity link");
        assert_eq!(link.id_full, Some(p1_id.to_string()));
        assert_eq!(
            link.href.as_deref(),
            Some(format!("/persons/{p1_id}/update").as_str())
        );
        assert_eq!(
            link.events_href.as_deref(),
            Some(format!("/audit-log?search={p1_id}").as_str())
        );
    }

    /// The rows follow the forms, so a reader finds the name where they
    /// expect it: before the address, and the address postal code first.
    #[test]
    fn compute_create_person_lists_fields_in_form_order() {
        let person = sample_person(PersonId::new());
        let events = vec![StoreEvent::new(1, PgEvent::CreatePerson(Box::new(person)))];

        let detail = AuditLogDetail::compute(&events, 1, EN).unwrap();
        let labels: Vec<&str> = rows(&detail)
            .into_iter()
            .map(|(_, row)| row.label.as_str())
            .collect();

        assert_eq!(
            labels,
            vec![
                "Initials",
                "First name",
                "Last name",
                "Country code",
                "Place of residence",
                "Citizen service number (BSN)",
                "Date of birth",
                "Gender",
                "Postal code",
                "House number",
                "House number addition",
                "Street name",
                "Locality",
                "Known in the BAG",
            ]
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
            StoreEvent::new(1, PgEvent::CreatePerson(Box::new(person))),
            StoreEvent::new(2, PgEvent::CreateCandidateList(Box::new(list))),
            StoreEvent::new(
                3,
                PgEvent::RemoveCandidateFromCandidateList { list_id, person_id },
            ),
        ];

        let detail = AuditLogDetail::compute(&events, 3, EN).unwrap();
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
            .map(|(i, p)| StoreEvent::new(i + 1, PgEvent::CreatePerson(Box::new(p.clone()))))
            .collect();
        events.push(StoreEvent::new(
            persons.len() + 1,
            PgEvent::CreateCandidateList(Box::new(list)),
        ));

        let detail = AuditLogDetail::compute(&events, persons.len() + 1, EN).unwrap();
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
            StoreEvent::new(1, PgEvent::CreatePerson(Box::new(existing.clone()))),
            StoreEvent::new(
                2,
                PgEvent::CreateCandidateList(Box::new(sample_candidate_list(list_id))),
            ),
            StoreEvent::new(
                3,
                PgEvent::ImportCandidates(Box::new(crate::projection::CandidateImport {
                    list_id,
                    file_name: "candidates.csv".to_string(),
                    file_size: 200,
                    created_persons: vec![created.clone()],
                    updated_persons: vec![existing.clone()],
                    candidates: vec![created.id, existing.id],
                })),
            ),
        ];

        let detail = AuditLogDetail::compute(&events, 3, EN).unwrap();

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
