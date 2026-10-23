//! Audit log overview showing all applied events.
mod event_info;
mod links;
mod pages;
mod paths;
mod structs;

pub use links::PgLinks;
pub use pages::router;
pub use paths::{AuditLogDetailPath, AuditLogPath};
pub use structs::{AuditLogDetail, AuditLogEntry};
