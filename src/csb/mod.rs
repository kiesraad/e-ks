//! CSB (Centraal Stembureau) domain.
//!
//! Mirrors the layout of the `pg` domain but is scoped to the central electoral
//! council side of the workflow: the import, pre-submission and examination
//! pages plus their own request context and error pages. The events and store projections these
//! pages read from live in [`crate::projection`].
pub mod audit_log;
pub mod common;
pub mod examination;
pub mod import;
pub mod index;
pub mod login;
pub mod monitoring;
pub mod pre_submission;
pub mod recovery;
pub mod registered_political_groups;

mod context;
mod error_response;

pub use context::CsbContext;
pub use error_response::render_csb_error_pages;
