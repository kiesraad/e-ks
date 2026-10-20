//! The locale-free description of what an event changed.

use super::{AuditValue, FieldPath};

/// One field-level change: where it happened and what happened.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub path: FieldPath,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChangeKind {
    /// The field had no value before the event.
    Added(AuditValue),
    /// The field has no value after the event.
    Removed(AuditValue),
    /// The field had a value on both sides.
    Changed { old: AuditValue, new: AuditValue },
    /// A set or ordered collection: only the items that came in, went out, or
    /// changed place. The rest of the collection is unchanged.
    Collection {
        added: Vec<AuditValue>,
        removed: Vec<AuditValue>,
        moved: Vec<Move>,
    },
}

/// An item that changed place in an ordered collection; positions are 1-based.
#[derive(Debug, Clone, PartialEq)]
pub struct Move {
    pub item: AuditValue,
    pub from: usize,
    pub to: usize,
}

impl Change {
    pub fn added(path: impl Into<FieldPath>, value: AuditValue) -> Self {
        Change {
            path: path.into(),
            kind: ChangeKind::Added(value),
        }
    }

    pub fn removed(path: impl Into<FieldPath>, value: AuditValue) -> Self {
        Change {
            path: path.into(),
            kind: ChangeKind::Removed(value),
        }
    }
}
