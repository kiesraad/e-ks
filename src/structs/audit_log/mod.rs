mod event_type_category;
mod field_change;

pub use event_type_category::EventTypeCategory;
#[cfg(test)]
pub(crate) use event_type_category::assert_covers;
pub use field_change::{EntityRef, FieldChange};
