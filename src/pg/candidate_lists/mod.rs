//! Candidate lists management.
//!
//! Scope: forms, extractors, pages, and structs that manage candidate lists,
//! their ordering, submitters, and related list summaries.
mod actions;
mod candidate_record;
#[cfg(all(test, fuzzing))]
mod csv_roundtrip_tests;
mod extractors;
mod forms;
pub(crate) mod importer;
mod pages;
mod paths;

pub use candidate_record::CandidateRecord;
pub(crate) use candidate_record::{CSV_HEADERS, CandidateRecordCsv};
pub use forms::{CandidateListCreateForm, CandidateListForm};
pub use pages::router;
pub use paths::ViewCandidateListPath;
// Only the guard test in this section reads this; see `view::context`.
#[cfg(test)]
pub use paths::CandidateListsPath;
