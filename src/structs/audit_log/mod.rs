//! The typed audit-log diff.
//!
//! Entities list their fields ([`AuditFields`], written with [`audit_fields!`]),
//! [`diff()`] compares two states of an entity into [`Change`]s, and
//! [`render_groups`] turns those into translated rows for the templates.
//! Nothing here goes through JSON: every value keeps its type until it is
//! rendered.

mod audit_event;
mod change;
mod diff;
mod event_type_category;
mod field_key;
mod fields;
mod render;
mod subject;
mod value;

pub use audit_event::AuditEvent;
pub use change::{Change, ChangeKind, Move};
#[cfg(test)]
pub use diff::fields_of;
pub use diff::{between, diff, diff_at, diff_updated};
pub use event_type_category::EventTypeCategory;
pub use field_key::{FieldKey, FieldPath};
pub(crate) use fields::audit_fields;
pub use fields::{AuditFields, Field};
#[cfg(test)]
pub use render::ChangeRow;
pub use render::{ChangeGroup, RenderContext, RowDetail, RowKind, bool_label, render_groups};
#[cfg(test)]
pub use subject::NoLinks;
pub use subject::{Subject, SubjectLink, SubjectLinks};
pub use value::{AuditLeaf, AuditValue};
#[cfg(test)]
pub use value::{EntityRef, EnumValue};
