//! Diffing typed audit fields.
//!
//! [`diff`] compares two states of one entity by listing both sides' fields
//! and zipping them by path. Sets and ordered collections get their own
//! treatment ([`set_diff`], [`ordered_diff`]) so a district that was added or a
//! candidate that moved shows up as exactly that, instead of as a wall of
//! changed positions.

use super::{AuditFields, AuditValue, Change, ChangeKind, Field, FieldPath, Move};

/// The fields of `entity`, or none when it does not exist.
pub fn fields_of<T: AuditFields>(entity: Option<&T>) -> Vec<Field> {
    fields_at(&FieldPath::root(), entity)
}

fn fields_at<T: AuditFields>(path: &FieldPath, entity: Option<&T>) -> Vec<Field> {
    let mut out = Vec::new();
    if let Some(entity) = entity {
        entity.audit_fields(path, &mut out);
    }
    out
}

/// The changes between two states of one entity. `None` on either side means
/// the entity did not exist there, so every field of the other side is an
/// addition or a removal.
pub fn diff<T: AuditFields>(old: Option<&T>, new: Option<&T>) -> Vec<Change> {
    diff_fields(fields_of(old), fields_of(new))
}

/// Like [`diff`], for an entity that sits under `path` in its parent.
pub fn diff_at<T: AuditFields>(path: FieldPath, old: Option<&T>, new: Option<&T>) -> Vec<Change> {
    diff_fields(fields_at(&path, old), fields_at(&path, new))
}

/// Zip two field lists by path. Fields present on the new side come first, in
/// the new side's order; fields only present on the old side follow.
pub fn diff_fields(old: Vec<Field>, new: Vec<Field>) -> Vec<Change> {
    let mut old = old;
    let mut changes = Vec::new();

    for (path, new_value) in new {
        let old_value = old
            .iter()
            .position(|(old_path, _)| *old_path == path)
            .map(|index| old.remove(index).1)
            .unwrap_or(AuditValue::Missing);
        changes.extend(between(path, old_value, new_value));
    }
    for (path, old_value) in old {
        changes.extend(between(path, old_value, AuditValue::Missing));
    }

    changes
}

/// The change from `old` to `new` at `path`, or `None` when nothing changed.
pub fn between(path: FieldPath, old: AuditValue, new: AuditValue) -> Option<Change> {
    let kind = between_values(old, new)?;
    Some(Change { path, kind })
}

fn between_values(old: AuditValue, new: AuditValue) -> Option<ChangeKind> {
    match (old, new) {
        (AuditValue::Missing, AuditValue::Missing) => None,
        (old, new) if old == new => None,
        // An entity that appears or disappears with an empty collection has
        // nothing to show for it.
        (AuditValue::Missing, new) if new.is_empty_collection() => None,
        (old, AuditValue::Missing) if old.is_empty_collection() => None,
        (AuditValue::Missing, new) => Some(ChangeKind::Added(new)),
        (old, AuditValue::Missing) => Some(ChangeKind::Removed(old)),
        (AuditValue::Set(old), AuditValue::Set(new)) => set_diff(&old, &new),
        (AuditValue::Ordered(old), AuditValue::Ordered(new)) => ordered_diff(&old, &new),
        (old, new) => Some(ChangeKind::Changed { old, new }),
    }
}

/// The items that entered or left a set; `None` when the sets hold the same
/// items.
pub fn set_diff(old: &[AuditValue], new: &[AuditValue]) -> Option<ChangeKind> {
    let (added, removed) = added_and_removed(old, new);
    if added.is_empty() && removed.is_empty() {
        return None;
    }
    Some(ChangeKind::Collection {
        added,
        removed,
        moved: Vec::new(),
    })
}

/// The items that entered, left, or changed place in an ordered collection;
/// `None` when the order is unchanged.
///
/// Only the items that actually moved are reported: the items that kept their
/// relative order (the longest increasing subsequence of old positions) are
/// left out, so moving one candidate up a list yields one move, not one per
/// candidate that shifted down to make room.
pub fn ordered_diff(old: &[AuditValue], new: &[AuditValue]) -> Option<ChangeKind> {
    let (added, removed) = added_and_removed(old, new);

    // Items on both sides, in new order, with their (new, old) positions.
    let common: Vec<(usize, usize)> = new
        .iter()
        .enumerate()
        .filter_map(|(new_index, item)| {
            old.iter()
                .position(|candidate| candidate.same_item(item))
                .map(|old_index| (new_index, old_index))
        })
        .collect();
    let old_positions: Vec<usize> = common.iter().map(|(_, old_index)| *old_index).collect();
    let kept = longest_increasing_subsequence(&old_positions);

    let moved: Vec<Move> = common
        .iter()
        .enumerate()
        .filter(|(index, _)| !kept.contains(index))
        .map(|(_, (new_index, old_index))| Move {
            item: new[*new_index].clone(),
            from: old_index + 1,
            to: new_index + 1,
        })
        .collect();

    if added.is_empty() && removed.is_empty() && moved.is_empty() {
        return None;
    }
    Some(ChangeKind::Collection {
        added,
        removed,
        moved,
    })
}

fn added_and_removed(old: &[AuditValue], new: &[AuditValue]) -> (Vec<AuditValue>, Vec<AuditValue>) {
    let added = new
        .iter()
        .filter(|item| !old.iter().any(|candidate| candidate.same_item(item)))
        .cloned()
        .collect();
    let removed = old
        .iter()
        .filter(|item| !new.iter().any(|candidate| candidate.same_item(item)))
        .cloned()
        .collect();
    (added, removed)
}

/// Indices into `values` of one longest strictly increasing subsequence.
/// Quadratic, which is fine for the collection sizes the audit log sees.
fn longest_increasing_subsequence(values: &[usize]) -> Vec<usize> {
    if values.is_empty() {
        return Vec::new();
    }

    // For every index: the length of the longest subsequence ending there and
    // the index it continues from.
    let mut length = vec![1usize; values.len()];
    let mut previous = vec![None; values.len()];
    for i in 1..values.len() {
        for j in 0..i {
            if values[j] < values[i] && length[j] + 1 > length[i] {
                length[i] = length[j] + 1;
                previous[i] = Some(j);
            }
        }
    }

    let mut end = (0..values.len()).max_by_key(|&i| length[i]).unwrap_or(0);
    let mut subsequence = vec![end];
    while let Some(prev) = previous[end] {
        subsequence.push(prev);
        end = prev;
    }
    subsequence.reverse();
    subsequence
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structs::audit_log::FieldKey;

    fn text(value: &str) -> AuditValue {
        AuditValue::Text(value.to_string())
    }

    fn texts(values: &[&str]) -> Vec<AuditValue> {
        values.iter().map(|value| text(value)).collect()
    }

    fn path(key: FieldKey) -> FieldPath {
        FieldPath::from(key)
    }

    #[test]
    fn equal_values_produce_no_change() {
        assert_eq!(
            between(path(FieldKey::LastName), text("Jansen"), text("Jansen")),
            None
        );
        assert_eq!(
            between(
                path(FieldKey::LastName),
                AuditValue::Missing,
                AuditValue::Missing
            ),
            None
        );
    }

    #[test]
    fn an_empty_collection_appearing_or_disappearing_is_no_change() {
        assert_eq!(
            between(
                path(FieldKey::Candidates),
                AuditValue::Ordered(Vec::new()),
                AuditValue::Missing
            ),
            None
        );
        assert_eq!(
            between(
                path(FieldKey::ElectoralDistricts),
                AuditValue::Missing,
                AuditValue::Set(Vec::new())
            ),
            None
        );
    }

    #[test]
    fn emptying_a_collection_reports_the_removed_items() {
        assert_eq!(
            between(
                path(FieldKey::Candidates),
                AuditValue::Ordered(texts(&["a"])),
                AuditValue::Ordered(Vec::new())
            )
            .map(|c| c.kind),
            Some(ChangeKind::Collection {
                added: Vec::new(),
                removed: texts(&["a"]),
                moved: Vec::new(),
            })
        );
    }

    #[test]
    fn missing_to_value_is_an_addition_and_back_a_removal() {
        assert_eq!(
            between(
                path(FieldKey::LastName),
                AuditValue::Missing,
                text("Jansen")
            ),
            Some(Change::added(FieldKey::LastName, text("Jansen")))
        );
        assert_eq!(
            between(
                path(FieldKey::LastName),
                text("Jansen"),
                AuditValue::Missing
            ),
            Some(Change::removed(FieldKey::LastName, text("Jansen")))
        );
    }

    #[test]
    fn differing_scalars_are_a_change() {
        assert_eq!(
            between(path(FieldKey::LastName), text("Janssen"), text("Jansen")).map(|c| c.kind),
            Some(ChangeKind::Changed {
                old: text("Janssen"),
                new: text("Jansen"),
            })
        );
    }

    #[test]
    fn set_diff_reports_only_what_entered_and_left() {
        let kind = set_diff(
            &texts(&["Groningen", "Utrecht"]),
            &texts(&["Utrecht", "Fryslân"]),
        );

        assert_eq!(
            kind,
            Some(ChangeKind::Collection {
                added: texts(&["Fryslân"]),
                removed: texts(&["Groningen"]),
                moved: Vec::new(),
            })
        );
    }

    #[test]
    fn set_with_the_same_items_in_another_order_is_unchanged() {
        assert_eq!(
            between(
                path(FieldKey::ElectoralDistricts),
                AuditValue::Set(texts(&["a", "b"])),
                AuditValue::Set(texts(&["b", "a"]))
            ),
            None
        );
    }

    #[test]
    fn ordered_diff_reports_a_single_move_to_the_front() {
        let old = texts(&["a", "b", "c", "d"]);
        let new = texts(&["c", "a", "b", "d"]);

        assert_eq!(
            ordered_diff(&old, &new),
            Some(ChangeKind::Collection {
                added: Vec::new(),
                removed: Vec::new(),
                moved: vec![Move {
                    item: text("c"),
                    from: 3,
                    to: 1,
                }],
            })
        );
    }

    #[test]
    fn ordered_diff_reports_a_swap_as_one_move() {
        let old = texts(&["a", "b", "c"]);
        let new = texts(&["b", "a", "c"]);

        let Some(ChangeKind::Collection { moved, .. }) = ordered_diff(&old, &new) else {
            panic!("expected a collection change");
        };
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].item, text("a"));
        assert_eq!((moved[0].from, moved[0].to), (1, 2));
    }

    #[test]
    fn ordered_diff_reports_an_append_as_an_addition_only() {
        let old = texts(&["a", "b"]);
        let new = texts(&["a", "b", "c"]);

        assert_eq!(
            ordered_diff(&old, &new),
            Some(ChangeKind::Collection {
                added: texts(&["c"]),
                removed: Vec::new(),
                moved: Vec::new(),
            })
        );
    }

    #[test]
    fn ordered_diff_reports_a_removal_without_moves_for_the_shifted_tail() {
        let old = texts(&["a", "b", "c", "d"]);
        let new = texts(&["a", "c", "d"]);

        assert_eq!(
            ordered_diff(&old, &new),
            Some(ChangeKind::Collection {
                added: Vec::new(),
                removed: texts(&["b"]),
                moved: Vec::new(),
            })
        );
    }

    #[test]
    fn ordered_diff_of_identical_lists_is_unchanged() {
        assert_eq!(ordered_diff(&texts(&["a", "b"]), &texts(&["a", "b"])), None);
    }

    #[test]
    fn diff_fields_keeps_new_side_order_then_old_only_paths() {
        let old = vec![
            (path(FieldKey::FirstName), text("Henk")),
            (path(FieldKey::LastName), text("Jansen")),
            (path(FieldKey::Initials), text("H.")),
        ];
        let new = vec![
            (path(FieldKey::LastName), text("Janssen")),
            (path(FieldKey::FirstName), text("Henk")),
        ];

        let changes = diff_fields(old, new);

        assert_eq!(
            changes,
            vec![
                Change {
                    path: path(FieldKey::LastName),
                    kind: ChangeKind::Changed {
                        old: text("Jansen"),
                        new: text("Janssen"),
                    },
                },
                Change::removed(FieldKey::Initials, text("H.")),
            ]
        );
    }

    #[test]
    fn longest_increasing_subsequence_picks_a_longest_run() {
        assert_eq!(longest_increasing_subsequence(&[]), Vec::<usize>::new());
        assert_eq!(longest_increasing_subsequence(&[0, 1, 2]), vec![0, 1, 2]);
        assert_eq!(longest_increasing_subsequence(&[2, 0, 1, 3]), vec![1, 2, 3]);
    }
}
