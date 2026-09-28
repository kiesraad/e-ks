//! Resolve ID-like diff values (person_id, etc.) into
//! `EntityRef`s the template can render as abbreviated clickable links plus a
//! human-readable description.

use crate::{
    ElectoralDistrict, PgStoreData,
    structs::{audit_log::EntityRef, candidate_lists::CandidateListId, persons::PersonId},
};

/// Entity types that can appear as ID references inside diff values.
enum EntityKind {
    Person,
    CandidateList,
}

/// Detect whether a flattened field key references another entity by ID.
///
/// Handles array-element paths like `candidates.0` by falling back to the
/// parent segment.
fn entity_kind_for_key(key: &str) -> Option<EntityKind> {
    let leaf = key.rsplit('.').next().unwrap_or(key);
    let semantic = if leaf.chars().all(|c| c.is_ascii_digit()) {
        key.rsplit('.').nth(1).unwrap_or(key)
    } else {
        leaf
    };
    match semantic {
        "person_id" | "candidates" | "created_persons" | "updated_persons" => {
            Some(EntityKind::Person)
        }
        "list_id" => Some(EntityKind::CandidateList),
        _ => None,
    }
}

/// Build `EntityRef`s for a diff cell value, if the key references known
/// entities. The value may be a single UUID or a comma-separated list of
/// UUIDs (from the scalar-array CSV collapsing performed in `flatten`).
pub(super) fn build_ref_diffs_for_key(
    key: &str,
    old_value: &str,
    state_before: &PgStoreData,
    new_value: &str,
    state_after: &PgStoreData,
) -> Option<(Vec<EntityRef>, Vec<EntityRef>)> {
    let kind = entity_kind_for_key(key)?;

    let old_refs = old_value
        .split(", ")
        .filter(|s| !s.is_empty())
        .map(|id_str| EntityRef {
            id_full: id_str.to_string(),
            description: describe_entity(&kind, id_str, state_before),
        })
        .collect();
    let new_refs = new_value
        .split(", ")
        .filter(|s| !s.is_empty())
        .map(|id_str| EntityRef {
            id_full: id_str.to_string(),
            description: describe_entity(&kind, id_str, state_after),
        })
        .collect();
    Some((old_refs, new_refs))
}

fn describe_entity(kind: &EntityKind, id_str: &str, state: &PgStoreData) -> String {
    match kind {
        EntityKind::Person => id_str
            .parse::<PersonId>()
            .ok()
            .and_then(|id| state.persons.get(&id))
            .map(|p| p.name.display())
            .unwrap_or_default(),
        EntityKind::CandidateList => id_str
            .parse::<CandidateListId>()
            .ok()
            .and_then(|id| state.candidate_lists.get(&id))
            .map(|cl| {
                cl.electoral_districts
                    .iter()
                    .map(ElectoralDistrict::title)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default(),
    }
}
