use serde_json::json;

use crate::{
    PgEvent, PgStoreData,
    structs::{
        list_submitters::{ListSubmitter, ListSubmitterId},
        persons::Person,
    },
};

/// The substitute submitter with the given ID, if present.
fn substitute_by_id<'a>(data: &'a PgStoreData, id: &ListSubmitterId) -> Option<&'a ListSubmitter> {
    data.substitute_submitters.iter().find(|s| &s.id == id)
}

/// The JSON snapshots of an entity before and after an event.
type OldNew = (Option<serde_json::Value>, Option<serde_json::Value>);

/// Serializes the state of an entity before and after an event to JSON.
fn json_diff<T: serde::Serialize>(old: Option<&T>, new: Option<&T>) -> OldNew {
    let to_json = |data: Option<&T>| data.and_then(|data| serde_json::to_value(data).ok());
    (to_json(old), to_json(new))
}

/// A payload that only exists after the event (system events).
fn added(value: serde_json::Value) -> OldNew {
    (None, Some(value))
}

/// Extract the old/new JSON snapshots for a given event, used by
/// `AuditLogDetail::compute` to build a field-level diff.
///
/// - Create events: no old state; new contains the created entity.
/// - Update events: old is the prior entity from `state_before`; new is from
///   `state_after`.
/// - Delete events: old is the entity from `state_before`; no new state.
/// - System events: informational only; rendered as additions.
pub(super) fn extract_old_new(
    event: &PgEvent,
    state_before: &PgStoreData,
    state_after: &PgStoreData,
) -> OldNew {
    entity_old_new(event, state_before, state_after).unwrap_or_else(|| system_event_payload(event))
}

/// Old/new snapshots for events that concern a stored entity; `None` for
/// system events, whose payloads are not diffed from the store.
fn entity_old_new(
    event: &PgEvent,
    state_before: &PgStoreData,
    state_after: &PgStoreData,
) -> Option<OldNew> {
    // Look up one entity by ID in both snapshots.
    let person = |id| json_diff(state_before.persons.get(id), state_after.persons.get(id));
    let list = |id| {
        json_diff(
            state_before.candidate_lists.get(id),
            state_after.candidate_lists.get(id),
        )
    };
    let name_auth = |id| {
        json_diff(
            state_before.name_authorisations.get(id),
            state_after.name_authorisations.get(id),
        )
    };
    let substitute = |id| {
        json_diff(
            substitute_by_id(state_before, id),
            substitute_by_id(state_after, id),
        )
    };

    Some(match event {
        PgEvent::CreatePerson(p) => person(&p.id),
        // Technically a create, but a diff against the absent old state works too
        PgEvent::CreatePersonPersonalData { person_id, .. } => person(person_id),
        PgEvent::CreateCandidateList(cl) => list(&cl.id),
        PgEvent::CreateNameAuthorisation(na) => name_auth(&na.id),
        PgEvent::CreateSubstituteSubmitter(ss) => substitute(&ss.id),

        PgEvent::UpdatePerson(p) => person(&p.id),
        PgEvent::UpdateNameAuthorisation(na) => name_auth(&na.id),
        PgEvent::UpdateListSubmitter(_) => json_diff(
            Some(&state_before.list_submitter),
            Some(&state_after.list_submitter),
        ),
        PgEvent::UpdateSubstituteSubmitter(ss) => substitute(&ss.id),
        PgEvent::UpdatePoliticalGroup(_) => json_diff(
            Some(&state_before.political_group),
            Some(&state_after.political_group),
        ),

        PgEvent::UpdatePersonPersonalData { person_id, .. }
        | PgEvent::UpdatePersonAddress { person_id, .. }
        | PgEvent::UpdatePersonRepresentative { person_id, .. } => person(person_id),
        PgEvent::UpdateCandidateListDistricts { list_id, .. }
        | PgEvent::UpdateCandidateListOrder { list_id, .. } => list(list_id),

        PgEvent::DeletePerson { person_id } => person(person_id),
        PgEvent::DeleteCandidateList(cl_id) => list(cl_id),
        PgEvent::DeleteNameAuthorisation(na_id) => name_auth(na_id),
        PgEvent::DeleteSubstituteSubmitter {
            substitute_submitter_id,
        } => substitute(substitute_submitter_id),

        PgEvent::AddCandidateToCandidateList { .. }
        | PgEvent::RemoveCandidateFromCandidateList { .. }
        | PgEvent::DeveloperLogin { .. }
        | PgEvent::Login
        | PgEvent::Logout
        | PgEvent::HideDownloadWarning
        | PgEvent::Import { .. }
        | PgEvent::DownloadFile { .. }
        | PgEvent::ExportCsv { .. }
        | PgEvent::ImportCandidates { .. } => return None,
    })
}

/// Payloads of system events, synthesized from the event itself.
fn system_event_payload(event: &PgEvent) -> OldNew {
    match event {
        PgEvent::AddCandidateToCandidateList { person_id, .. } => {
            added(json!({ "person_id": person_id.to_string() }))
        }
        PgEvent::RemoveCandidateFromCandidateList { person_id, .. } => {
            (Some(json!({ "person_id": person_id.to_string() })), None)
        }
        PgEvent::DeveloperLogin { stream_id } => {
            added(json!({ "stream_id": stream_id.to_string() }))
        }
        PgEvent::DownloadFile {
            file_name,
            download_path,
        } => added(json!({
            "file_name": file_name,
            "download_path": download_path,
        })),
        PgEvent::ExportCsv {
            file_name,
            file_size,
            list_id,
        } => added(json!({
            "file_name": file_name,
            "file_size": file_size,
            "list_id": list_id.to_string(),
        })),
        PgEvent::ImportCandidates {
            file_name,
            file_size,
            list_id,
            created_persons,
            updated_persons,
            ..
        } => added(json!({
            "file_name": file_name,
            "file_size": file_size,
            "list_id": list_id.to_string(),
            "created_persons": person_ids(created_persons),
            "updated_persons": person_ids(updated_persons),
        })),
        // `HideDownloadWarning` and `Import` carry no payload. Entity-backed
        // events never reach here (they are handled by `entity_old_new`).
        _ => added(json!({})),
    }
}

fn person_ids(persons: &[Person]) -> Vec<String> {
    persons.iter().map(|p| p.id.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ElectoralDistrict, StreamId,
        structs::{
            candidate_lists::CandidateListId, common::PreviousElectionResults,
            list_submitters::ListSubmitterId, name_authorisations::NameAuthorisationId,
            persons::PersonId,
        },
        test_utils::{
            sample_candidate_list, sample_list_submitter, sample_name_authorisation, sample_person,
            sample_political_group,
        },
    };
    use std::collections::BTreeSet;

    fn empty_state() -> PgStoreData {
        PgStoreData::default()
    }

    // --- Create events: entity absent in `state_before`, present in `state_after` ---

    #[test]
    fn create_person_returns_none_and_new_from_state_after() {
        let person = sample_person(PersonId::new());
        let before = empty_state();
        let mut after = empty_state();
        after.persons.insert(person.id, person.clone());

        let (old, new) = extract_old_new(&PgEvent::CreatePerson(person.clone()), &before, &after);
        assert!(old.is_none());
        assert_eq!(new, serde_json::to_value(&person).ok());
    }

    #[test]
    fn create_candidate_list_pulls_new_from_state_after() {
        let list = sample_candidate_list(CandidateListId::new());
        let before = empty_state();
        let mut after = empty_state();
        after.candidate_lists.insert(list.id, list.clone());

        let (old, new) =
            extract_old_new(&PgEvent::CreateCandidateList(list.clone()), &before, &after);
        assert!(old.is_none());
        assert_eq!(new, serde_json::to_value(&list).ok());
    }

    #[test]
    fn create_substitute_submitter_pulls_new_from_state_after() {
        let submitter = sample_list_submitter(ListSubmitterId::new());
        let before = empty_state();
        let mut after = empty_state();
        after.substitute_submitters.push(submitter.clone());

        let (old, new) = extract_old_new(
            &PgEvent::CreateSubstituteSubmitter(submitter.clone()),
            &before,
            &after,
        );
        assert!(old.is_none());
        assert_eq!(new, serde_json::to_value(&submitter).ok());
    }

    #[test]
    fn create_name_authorisation_pulls_new_from_state_after() {
        let name_auth = sample_name_authorisation(NameAuthorisationId::new());
        let before = empty_state();
        let mut after = empty_state();
        after
            .name_authorisations
            .insert(name_auth.id, name_auth.clone());

        let (old, new) = extract_old_new(
            &PgEvent::CreateNameAuthorisation(name_auth.clone()),
            &before,
            &after,
        );
        assert!(old.is_none());
        assert_eq!(new, serde_json::to_value(&name_auth).ok());
    }

    // --- Update events: entity present in both snapshots, with different contents ---

    #[test]
    fn update_person_uses_state_after_for_new() {
        let person_id = PersonId::new();
        let before_person = sample_person(person_id);
        let mut after_person = before_person.clone();
        after_person.name.initials = "X.Y.".parse().unwrap();

        let mut before = empty_state();
        before.persons.insert(person_id, before_person.clone());
        let mut after = empty_state();
        after.persons.insert(person_id, after_person.clone());

        let (old, new) = extract_old_new(
            &PgEvent::UpdatePerson(after_person.clone()),
            &before,
            &after,
        );
        assert_eq!(old, serde_json::to_value(&before_person).ok());
        assert_eq!(new, serde_json::to_value(&after_person).ok());
    }

    #[test]
    fn update_person_personal_data_uses_full_person_snapshots() {
        let person_id = PersonId::new();
        let before_person = sample_person(person_id);
        let mut after_person = before_person.clone();
        after_person.personal_data.country = None;

        let mut before = empty_state();
        before.persons.insert(person_id, before_person.clone());
        let mut after = empty_state();
        after.persons.insert(person_id, after_person.clone());

        let (old, new) = extract_old_new(
            &PgEvent::UpdatePersonPersonalData {
                person_id,
                name: after_person.name.clone(),
                personal_data: after_person.personal_data.clone(),
            },
            &before,
            &after,
        );
        assert_eq!(old, serde_json::to_value(&before_person).ok());
        assert_eq!(new, serde_json::to_value(&after_person).ok());
    }

    #[test]
    fn update_political_group_uses_whole_field_with_no_id() {
        let mut before = empty_state();
        before.political_group = sample_political_group();
        let mut after = empty_state();
        after.political_group = sample_political_group();
        after.political_group.previous_election_results =
            Some(PreviousElectionResults::SixteenOrMoreSeats);

        let (old, new) = extract_old_new(
            &PgEvent::UpdatePoliticalGroup(after.political_group.clone()),
            &before,
            &after,
        );
        assert_eq!(old, serde_json::to_value(&before.political_group).ok());
        assert_eq!(new, serde_json::to_value(&after.political_group).ok());
    }

    #[test]
    fn update_candidate_list_districts_pulls_full_list_from_snapshots() {
        let list_id = CandidateListId::new();
        let mut before_list = sample_candidate_list(list_id);
        before_list.electoral_districts = BTreeSet::from([ElectoralDistrict::Groningen]);
        let mut after_list = before_list.clone();
        after_list.electoral_districts =
            BTreeSet::from([ElectoralDistrict::Groningen, ElectoralDistrict::Fryslan]);

        let mut before = empty_state();
        before.candidate_lists.insert(list_id, before_list.clone());
        let mut after = empty_state();
        after.candidate_lists.insert(list_id, after_list.clone());

        let (old, new) = extract_old_new(
            &PgEvent::UpdateCandidateListDistricts {
                list_id,
                electoral_districts: after_list.electoral_districts.clone(),
            },
            &before,
            &after,
        );
        assert_eq!(old, serde_json::to_value(&before_list).ok());
        assert_eq!(new, serde_json::to_value(&after_list).ok());
    }

    // --- Delete events: entity present in `state_before`, absent in `state_after` ---

    #[test]
    fn delete_person_returns_old_from_state_before_and_none_new() {
        let person = sample_person(PersonId::new());
        let mut before = empty_state();
        before.persons.insert(person.id, person.clone());
        let after = empty_state();

        let (old, new) = extract_old_new(
            &PgEvent::DeletePerson {
                person_id: person.id,
            },
            &before,
            &after,
        );
        assert_eq!(old, serde_json::to_value(&person).ok());
        assert!(new.is_none());
    }

    #[test]
    fn delete_candidate_list_returns_old_from_state_before() {
        let list = sample_candidate_list(CandidateListId::new());
        let mut before = empty_state();
        before.candidate_lists.insert(list.id, list.clone());
        let after = empty_state();

        let (old, new) = extract_old_new(&PgEvent::DeleteCandidateList(list.id), &before, &after);
        assert_eq!(old, serde_json::to_value(&list).ok());
        assert!(new.is_none());
    }

    #[test]
    fn delete_of_missing_entity_yields_none_on_both_sides() {
        // If somehow both snapshots lack the entity, both sides are None —
        // this mirrors the behaviour of `.get().and_then(..)` on an absent key.
        let before = empty_state();
        let after = empty_state();
        let (old, new) = extract_old_new(
            &PgEvent::DeletePerson {
                person_id: PersonId::new(),
            },
            &before,
            &after,
        );
        assert!(old.is_none());
        assert_eq!(new, None);
    }

    // --- Add/Remove candidate: payload built directly, not from state ---

    #[test]
    fn add_candidate_returns_only_person_id_in_new() {
        let person_id = PersonId::new();
        let state = empty_state();
        let (old, new) = extract_old_new(
            &PgEvent::AddCandidateToCandidateList {
                list_id: CandidateListId::new(),
                person_id,
            },
            &state,
            &state,
        );
        assert!(old.is_none());
        assert_eq!(
            new,
            Some(serde_json::json!({ "person_id": person_id.to_string() }))
        );
    }

    #[test]
    fn remove_candidate_returns_only_person_id_in_old() {
        let person_id = PersonId::new();
        let state = empty_state();
        let (old, new) = extract_old_new(
            &PgEvent::RemoveCandidateFromCandidateList {
                list_id: CandidateListId::new(),
                person_id,
            },
            &state,
            &state,
        );
        assert_eq!(
            old,
            Some(serde_json::json!({ "person_id": person_id.to_string() }))
        );
        assert!(new.is_none());
    }

    // --- System events: payload is synthetic, independent of the app state ---

    #[test]
    fn developer_login_builds_event_payload_from_stream_id() {
        let stream_id = StreamId::new();
        let state = empty_state();
        let (old, new) = extract_old_new(&PgEvent::DeveloperLogin { stream_id }, &state, &state);
        assert!(old.is_none());
        assert_eq!(
            new,
            Some(serde_json::json!({ "stream_id": stream_id.to_string() }))
        );
    }

    #[test]
    fn download_file_builds_event_payload_with_all_fields() {
        let state = empty_state();
        let (old, new) = extract_old_new(
            &PgEvent::DownloadFile {
                file_name: "list.csv".to_string(),
                download_path: "/tmp/list.csv".to_string(),
            },
            &state,
            &state,
        );
        assert!(old.is_none());
        assert_eq!(
            new,
            Some(serde_json::json!({
                "file_name": "list.csv",
                "download_path": "/tmp/list.csv",
            }))
        );
    }

    #[test]
    fn import_candidates_payload_includes_imported_persons() {
        let list_id = CandidateListId::new();
        let state = empty_state();
        let person_a = sample_person(PersonId::new());
        let person_b = sample_person(PersonId::new());
        let person_c = sample_person(PersonId::new());

        let event = PgEvent::ImportCandidates {
            list_id,
            file_name: "candidates.csv".to_string(),
            file_size: 123,
            created_persons: vec![person_a.clone(), person_b.clone()],
            updated_persons: vec![person_c.clone()],
            candidates: vec![person_a.id, person_b.id, person_c.id],
        };

        let (old, new) = extract_old_new(&event, &state, &state);

        assert!(old.is_none());
        assert_eq!(
            new,
            Some(serde_json::json!({
                "file_name": "candidates.csv",
                "file_size": 123,
                "list_id": list_id.to_string(),
                "created_persons": [
                    person_a.id.to_string(),
                    person_b.id.to_string(),
                ],
                "updated_persons": [person_c.id.to_string()],
            }))
        );
    }
}
