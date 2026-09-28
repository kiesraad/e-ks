//! Detailed (per-field) view of a single audit-log event.
//!
//! `AuditLogDetail::compute` replays the event stream on top of a base state
//! to reconstruct the state before and after the target event, then diffs the
//! flattened JSON representations to produce a list of `FieldChange`s.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use crate::{
    Locale, PgEvent, PgStoreData,
    store::{StoreData, StoreEvent},
};

use crate::structs::audit_log::FieldChange;

use super::{
    audit_log_entry::AuditLogEntry,
    entity_refs::build_ref_diffs_for_key,
    event_payload::extract_old_new,
    field_display::{field_label, field_value},
    json_flatten::flatten,
};

/// Flattened-JSON keys to skip from the diff (metadata, not meaningful changes).
const EXCLUDED_FIELDS: &[&str] = &["id", "updated_at", "created_at"];

/// Whether a flattened key, or its leaf segment, is one of `EXCLUDED_FIELDS`.
pub(super) fn is_excluded_field(key: &str) -> bool {
    EXCLUDED_FIELDS
        .iter()
        .any(|ex| key == *ex || key.ends_with(&format!(".{ex}")))
}

/// Detailed view of an audit log event, including field-level changes.
pub struct AuditLogDetail {
    pub event_id: usize,
    pub description: String,
    pub details: String,
    pub subject_id_full: String,
    pub subject_path: String,
    pub created_at: DateTime<Utc>,
    pub changes: Vec<FieldChange>,
}

impl AuditLogDetail {
    /// Compute the detail view for a specific event by replaying the event log
    /// on top of `base`: the imported snapshot in paper-corrections mode, an
    /// empty projection otherwise. Returns `None` if the event id is not found.
    pub fn compute(
        base: &PgStoreData,
        events: &[StoreEvent<PgEvent>],
        target_event_id: usize,
        locale: Locale,
    ) -> Option<Self> {
        let target_index = events.iter().position(|e| e.event_id == target_event_id)?;
        let target_event = &events[target_index];

        let state_before = replay(base, &events[..target_index]);
        let state_after = replay(base, &events[..=target_index]);

        let (old_json, new_json) =
            extract_old_new(&target_event.payload, &state_before, &state_after);
        let old_flat = old_json
            .as_ref()
            .map(|v| flatten(v, ""))
            .unwrap_or_default();
        let new_flat = new_json
            .as_ref()
            .map(|v| flatten(v, ""))
            .unwrap_or_default();

        let changes = diff(&old_flat, &new_flat, &state_before, &state_after, locale);
        let entry = AuditLogEntry::new(target_event.clone(), locale);

        Some(AuditLogDetail {
            event_id: entry.event_id,
            description: entry.description,
            details: entry.details,
            subject_id_full: entry.subject_id_full,
            subject_path: entry.subject_path,
            created_at: entry.created_at,
            changes,
        })
    }
}

fn replay(base: &PgStoreData, events: &[StoreEvent<PgEvent>]) -> PgStoreData {
    let mut state = base.clone();
    for event in events {
        state.apply(event.clone());
    }
    state
}

/// Compare two flattened maps and return only the fields that differ,
/// enriched with entity references where keys refer to other entities.
fn diff(
    old: &BTreeMap<String, String>,
    new: &BTreeMap<String, String>,
    state_before: &PgStoreData,
    state_after: &PgStoreData,
    locale: Locale,
) -> Vec<FieldChange> {
    let mut changes = Vec::new();
    let mut all_keys: Vec<&str> = old.keys().chain(new.keys()).map(String::as_str).collect();
    all_keys.sort_unstable_by(|a, b| natural_cmp(a, b));
    all_keys.dedup();

    for key in &all_keys {
        if is_excluded_field(key) {
            continue;
        }

        let old_val = old.get(*key).cloned().unwrap_or_default();
        let new_val = new.get(*key).cloned().unwrap_or_default();
        if old_val == new_val {
            continue;
        }

        if let Some((old_refs, new_refs)) =
            build_ref_diffs_for_key(key, &old_val, state_before, &new_val, state_after)
        {
            changes.push(FieldChange::Entities {
                field: field_label(key, locale),
                old_refs,
                new_refs,
            });
        } else {
            changes.push(FieldChange::Regular {
                field: field_label(key, locale),
                old_value: field_value(key, &old_val, locale),
                new_value: field_value(key, &new_val, locale),
            });
        }
    }

    changes
}

/// Compare dot-separated paths with all-digit segments ordered numerically, so
/// positional-array keys like `candidates.10` sort after `candidates.2`.
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut a_iter = a.split('.');
    let mut b_iter = b.split('.');
    loop {
        match (a_iter.next(), b_iter.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(a_seg), Some(b_seg)) => {
                let ord = match (a_seg.parse::<u64>(), b_seg.parse::<u64>()) {
                    (Ok(an), Ok(bn)) => an.cmp(&bn),
                    _ => a_seg.cmp(b_seg),
                };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ElectoralDistrict, Locale,
        structs::{
            audit_log::EntityRef,
            candidate_lists::CandidateListId,
            common::{FullName, PlaceOfResidence, PreviousElectionResults},
            persons::PersonId,
        },
        test_utils::{sample_candidate_list, sample_person, sample_political_group},
    };
    use std::collections::BTreeSet;

    impl FieldChange {
        fn field(&self) -> &str {
            match self {
                FieldChange::Regular { field, .. } | FieldChange::Entities { field, .. } => field,
            }
        }

        fn assert_no_old(&self) {
            match self {
                FieldChange::Regular { old_value, .. } => assert!(old_value.is_empty()),
                FieldChange::Entities { old_refs, .. } => assert!(old_refs.is_empty()),
            }
        }

        fn assert_no_new(&self) {
            match self {
                FieldChange::Regular { new_value, .. } => assert!(new_value.is_empty()),
                FieldChange::Entities { new_refs, .. } => assert!(new_refs.is_empty()),
            }
        }

        fn old_value(&self) -> &str {
            match self {
                FieldChange::Regular { old_value, .. } => old_value,
                FieldChange::Entities { .. } => unreachable!(),
            }
        }

        fn new_value(&self) -> &str {
            match self {
                FieldChange::Regular { new_value, .. } => new_value,
                FieldChange::Entities { .. } => unreachable!(),
            }
        }

        fn old_refs(&self) -> &[EntityRef] {
            match self {
                FieldChange::Regular { .. } => unreachable!(),
                FieldChange::Entities { old_refs, .. } => old_refs,
            }
        }

        fn new_refs(&self) -> &[EntityRef] {
            match self {
                FieldChange::Regular { .. } => unreachable!(),
                FieldChange::Entities { new_refs, .. } => new_refs,
            }
        }
    }

    const EN: Locale = Locale::En;

    fn empty_state() -> PgStoreData {
        PgStoreData::default()
    }

    // --- diff() ---

    #[test]
    fn diff_detects_changes() {
        let mut old = BTreeMap::new();
        old.insert("name".to_string(), "Alice".to_string());
        old.insert("age".to_string(), "30".to_string());

        let mut new = BTreeMap::new();
        new.insert("name".to_string(), "Bob".to_string());
        new.insert("age".to_string(), "30".to_string());

        let state = empty_state();
        let changes = diff(&old, &new, &state, &state, EN);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].field(), "name");
        assert_eq!(changes[0].old_value(), "Alice");
        assert_eq!(changes[0].new_value(), "Bob");
    }

    #[test]
    fn diff_detects_additions() {
        let old = BTreeMap::new();
        let mut new = BTreeMap::new();
        new.insert("name".to_string(), "Alice".to_string());

        let state = empty_state();
        let changes = diff(&old, &new, &state, &state, EN);
        assert_eq!(changes.len(), 1);
        changes[0].assert_no_old();
        assert_eq!(changes[0].new_value(), "Alice");
    }

    #[test]
    fn diff_detects_removals() {
        let mut old = BTreeMap::new();
        old.insert("name".to_string(), "Alice".to_string());
        let new = BTreeMap::new();

        let state = empty_state();
        let changes = diff(&old, &new, &state, &state, EN);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].old_value(), "Alice");
        changes[0].assert_no_new();
    }

    #[test]
    fn diff_excludes_id_fields() {
        let mut old = BTreeMap::new();
        old.insert("id".to_string(), "abc".to_string());
        old.insert("name".to_string(), "Alice".to_string());

        let mut new = BTreeMap::new();
        new.insert("id".to_string(), "abc".to_string());
        new.insert("name".to_string(), "Bob".to_string());

        let state = empty_state();
        let changes = diff(&old, &new, &state, &state, EN);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].field(), "name");
    }

    #[test]
    fn diff_no_changes() {
        let mut old = BTreeMap::new();
        old.insert("name".to_string(), "Alice".to_string());
        let new = old.clone();

        let state = empty_state();
        let changes = diff(&old, &new, &state, &state, EN);
        assert!(changes.is_empty());
    }

    #[test]
    fn diff_excludes_nested_id_fields() {
        let mut old = BTreeMap::new();
        old.insert("person.id".to_string(), "abc".to_string());
        old.insert("person.updated_at".to_string(), "old-ts".to_string());
        old.insert("person.created_at".to_string(), "old-ts".to_string());
        old.insert("person.name".to_string(), "Alice".to_string());

        let mut new = BTreeMap::new();
        new.insert("person.id".to_string(), "abc".to_string());
        new.insert("person.updated_at".to_string(), "new-ts".to_string());
        new.insert("person.created_at".to_string(), "new-ts".to_string());
        new.insert("person.name".to_string(), "Bob".to_string());

        let state = empty_state();
        let changes = diff(&old, &new, &state, &state, EN);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].field(), "person.name");
    }

    #[test]
    fn diff_both_empty() {
        let old = BTreeMap::new();
        let new = BTreeMap::new();
        let state = empty_state();
        let changes = diff(&old, &new, &state, &state, EN);
        assert!(changes.is_empty());
    }

    #[test]
    fn diff_orders_positional_array_keys_numerically() {
        // Populate candidates.0 .. candidates.11 with distinct old/new values
        // so every index becomes a change. Without natural ordering these
        // would come out in lexicographic order (#1, #10, #11, #2, ...).
        let mut old = BTreeMap::new();
        let mut new = BTreeMap::new();
        for i in 0..12 {
            old.insert(format!("candidates.{i}"), format!("old-{i}"));
            new.insert(format!("candidates.{i}"), format!("new-{i}"));
        }

        let state = empty_state();
        let changes = diff(&old, &new, &state, &state, EN);

        let fields: Vec<&str> = changes.iter().map(FieldChange::field).collect();
        let expected: Vec<String> = (1..=12).map(|n| format!("Candidates #{n}")).collect();
        assert_eq!(
            fields,
            expected.iter().map(String::as_str).collect::<Vec<_>>()
        );
    }

    // --- natural_cmp() ---

    #[test]
    fn natural_cmp_equal_strings() {
        use std::cmp::Ordering;
        assert_eq!(natural_cmp("candidates.0", "candidates.0"), Ordering::Equal);
    }

    #[test]
    fn natural_cmp_numeric_leaves_ordered_numerically() {
        use std::cmp::Ordering;
        assert_eq!(natural_cmp("candidates.2", "candidates.10"), Ordering::Less);
        assert_eq!(
            natural_cmp("candidates.10", "candidates.11"),
            Ordering::Less
        );
        assert_eq!(
            natural_cmp("candidates.11", "candidates.2"),
            Ordering::Greater
        );
    }

    #[test]
    fn natural_cmp_nested_numeric_segments_ordered_numerically() {
        use std::cmp::Ordering;
        assert_eq!(
            natural_cmp("candidates.2.name", "candidates.10.name"),
            Ordering::Less
        );
    }

    #[test]
    fn natural_cmp_non_numeric_segments_fall_back_to_string_order() {
        use std::cmp::Ordering;
        assert_eq!(natural_cmp("a.1", "b.0"), Ordering::Less);
        assert_eq!(natural_cmp("candidates.x", "candidates.y"), Ordering::Less);
    }

    #[test]
    fn natural_cmp_shorter_path_sorts_before_its_extension() {
        use std::cmp::Ordering;
        assert_eq!(natural_cmp("candidates", "candidates.0"), Ordering::Less);
        assert_eq!(natural_cmp("candidates.0", "candidates"), Ordering::Greater);
    }

    // --- compute() ---

    #[test]
    fn compute_create_event() {
        let person = sample_person(PersonId::new());
        let events = vec![StoreEvent::new(1, PgEvent::CreatePerson(person))];

        let detail = AuditLogDetail::compute(&empty_state(), &events, 1, Locale::En).unwrap();
        assert_eq!(detail.event_id, 1);
        assert_eq!(detail.description, "Created person");
        assert!(!detail.changes.is_empty());
        for change in &detail.changes {
            change.assert_no_old();
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

        let detail = AuditLogDetail::compute(&empty_state(), &events, 2, Locale::En).unwrap();
        let first_name_change = detail
            .changes
            .iter()
            .find(|c| c.field() == "First name")
            .expect("should have first_name change");
        assert_eq!(first_name_change.new_value(), "Updated");
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
        let change = detail
            .changes
            .iter()
            .find(|c| c.field() == "First name")
            .expect("first name change");
        assert_eq!(change.old_value(), "Henk");
        assert_eq!(change.new_value(), "Updated");
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
        let change = detail
            .changes
            .iter()
            .find(|c| c.field() == "Result of the previous election")
            .expect("previous election results change");
        assert_eq!(change.old_value(), "0 seats, or did not participate");
        assert_eq!(change.new_value(), "16 or more seats");
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
        let changes: Vec<_> = detail
            .changes
            .iter()
            .filter(|c| c.field() == "Place of residence")
            .collect();

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].old_value(), "Juinen");
        assert_eq!(changes[0].new_value(), "Utrecht");
    }

    #[test]
    fn compute_create_person_shows_user_facing_personal_data_values() {
        let person = sample_person(PersonId::new());
        let events = vec![StoreEvent::new(1, PgEvent::CreatePerson(person))];

        let detail = AuditLogDetail::compute(&empty_state(), &events, 1, EN).unwrap();
        let value_of = |field: &str| {
            detail
                .changes
                .iter()
                .find(|c| c.field() == field)
                .map(FieldChange::new_value)
                .unwrap_or_else(|| panic!("no change for {field}"))
        };

        assert_eq!(value_of("Gender"), "Female");
        assert_eq!(
            value_of("Social security number (BSN)"),
            "Confirmed: no social security number"
        );
        assert_eq!(value_of("Date of birth"), "01-02-1990");
        assert_eq!(value_of("Known in the BAG"), "Yes");
    }

    #[test]
    fn compute_delete_event() {
        let person_id = PersonId::new();
        let person = sample_person(person_id);

        let events = vec![
            StoreEvent::new(1, PgEvent::CreatePerson(person)),
            StoreEvent::new(2, PgEvent::DeletePerson { person_id }),
        ];

        let detail = AuditLogDetail::compute(&empty_state(), &events, 2, Locale::En).unwrap();
        assert!(!detail.changes.is_empty());
        for change in &detail.changes {
            change.assert_no_new();
        }
    }

    #[test]
    fn compute_returns_none_for_unknown_event() {
        let events = vec![StoreEvent::new(
            1,
            PgEvent::UpdatePoliticalGroup(sample_political_group()),
        )];
        assert!(AuditLogDetail::compute(&empty_state(), &events, 999, Locale::En).is_none());
    }

    #[test]
    fn compute_returns_none_for_empty_events() {
        let events: Vec<StoreEvent<PgEvent>> = vec![];
        assert!(AuditLogDetail::compute(&empty_state(), &events, 1, Locale::En).is_none());
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

        let detail = AuditLogDetail::compute(&empty_state(), &events, 1, Locale::En).unwrap();
        for change in &detail.changes {
            change.assert_no_old();
        }
    }

    #[test]
    fn compute_array_of_scalars_diff_is_single_csv_row() {
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

        let detail = AuditLogDetail::compute(&empty_state(), &events, 2, Locale::En).unwrap();
        let districts: Vec<_> = detail
            .changes
            .iter()
            .filter(|c| c.field() == "Electoral districts")
            .collect();

        assert_eq!(districts.len(), 1);
        assert_eq!(districts[0].old_value(), "Groningen");
        // Fryslân's title carries a diacritic the serde tag ("Fryslan") does not.
        assert_eq!(districts[0].new_value(), "Groningen, Fryslân");
    }

    #[test]
    fn compute_candidates_array_renders_changed_positions() {
        let list_id = CandidateListId::new();
        let p1 = sample_person(PersonId::new());
        let p2 = sample_person(PersonId::new());
        let p3 = sample_person(PersonId::new());
        let (p1_id, p2_id, p3_id) = (p1.id, p2.id, p3.id);
        let p1_name = p1.name.display();
        let p2_name = p2.name.display();

        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![p1_id, p2_id, p3_id];

        let events = vec![
            StoreEvent::new(1, PgEvent::CreatePerson(p1)),
            StoreEvent::new(2, PgEvent::CreatePerson(p2)),
            StoreEvent::new(3, PgEvent::CreatePerson(p3)),
            StoreEvent::new(4, PgEvent::CreateCandidateList(list)),
            // Swap positions 0 and 1; position 2 unchanged.
            StoreEvent::new(
                5,
                PgEvent::UpdateCandidateListOrder {
                    list_id,
                    candidates: vec![p2_id, p1_id, p3_id],
                },
            ),
        ];

        let detail = AuditLogDetail::compute(&empty_state(), &events, 5, Locale::En).unwrap();
        let candidate_changes: Vec<_> = detail
            .changes
            .iter()
            .filter(|c| c.field().starts_with("Candidates #"))
            .collect();

        assert_eq!(candidate_changes.len(), 2);

        let pos_1 = candidate_changes
            .iter()
            .find(|c| c.field() == "Candidates #1")
            .expect("position #1 change");
        assert_eq!(pos_1.old_refs()[0].description, p1_name);
        assert_eq!(pos_1.new_refs()[0].description, p2_name);

        let pos_2 = candidate_changes
            .iter()
            .find(|c| c.field() == "Candidates #2")
            .expect("position #2 change");
        assert_eq!(pos_2.old_refs()[0].description, p2_name);
        assert_eq!(pos_2.new_refs()[0].description, p1_name);

        assert!(
            !detail.changes.iter().any(|c| c.field() == "Candidates #3"),
            "unchanged position should not appear"
        );
    }

    #[test]
    fn compute_non_id_field_has_no_refs() {
        let person_id = PersonId::new();
        let person = sample_person(person_id);
        let mut updated = person.clone();
        updated.name = FullName {
            first_name: Some("Updated".parse().unwrap()),
            ..person.name.clone()
        };

        let events = vec![
            StoreEvent::new(1, PgEvent::CreatePerson(person)),
            StoreEvent::new(2, PgEvent::UpdatePerson(updated)),
        ];

        let detail = AuditLogDetail::compute(&empty_state(), &events, 2, Locale::En).unwrap();
        let change = detail
            .changes
            .iter()
            .find(|c| c.field() == "First name")
            .expect("first name change");
        match change {
            FieldChange::Regular { .. } => {}
            FieldChange::Entities { .. } => panic!(),
        }
    }

    #[test]
    fn compute_removed_candidate_ref_resolved_from_state_before() {
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

        let detail = AuditLogDetail::compute(&empty_state(), &events, 3, Locale::En).unwrap();
        let change = detail
            .changes
            .iter()
            .find(|c| c.field() == "Person ID")
            .expect("person_id change");
        assert_eq!(change.old_refs().len(), 1);
        assert_eq!(change.old_refs()[0].description, person_name);
        change.assert_no_new();
    }

    #[test]
    fn compute_create_candidate_list_orders_candidates_numerically() {
        // Regression test: for a candidate list with 10+ candidates the detail
        // view used to render rows in lexicographic order of the array index
        // (#1, #10, #11, ..., #19, #2, ...). The rows should instead follow
        // nomination order (#1, #2, ..., #12).
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
            AuditLogDetail::compute(&empty_state(), &events, persons.len() + 1, Locale::En)
                .unwrap();
        let candidate_fields: Vec<&str> = detail
            .changes
            .iter()
            .map(FieldChange::field)
            .filter(|f| f.starts_with("Candidates #"))
            .collect();

        let expected: Vec<String> = (1..=12).map(|n| format!("Candidates #{n}")).collect();
        assert_eq!(
            candidate_fields,
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

        let detail = AuditLogDetail::compute(&empty_state(), &events, 3, Locale::En).unwrap();

        let created_change = detail
            .changes
            .iter()
            .find(|c| c.field() == "Created candidates")
            .expect("created candidates change");
        created_change.assert_no_old();
        assert_eq!(created_change.new_refs().len(), 1);
        assert_eq!(created_change.new_refs()[0].description, created_name);

        let updated_change = detail
            .changes
            .iter()
            .find(|c| c.field() == "Updated candidates")
            .expect("updated candidates change");
        updated_change.assert_no_old();
        assert_eq!(updated_change.new_refs().len(), 1);
        assert_eq!(updated_change.new_refs()[0].description, existing_name);
    }
}
