//! What an event means for the audit log, kept apart from the store's
//! [`Event`](crate::Event) so the generic store knows nothing of audit-log
//! types.

use super::{Change, Subject};

pub trait AuditEvent {
    /// The projection this event is applied to. [`AuditEvent::changes`] reads
    /// the state as it stood before the event.
    type State;

    /// Field-level changes this event makes to `before`, the projection as it
    /// stood when the event was applied. Empty for events that change no
    /// structured data (a login, a download).
    fn changes(&self, before: &Self::State) -> Vec<Change>;

    /// The item the event is about; `None` for events about no item (a
    /// login, a download).
    fn subject(&self) -> Option<Subject>;
}
