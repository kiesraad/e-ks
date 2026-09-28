//! Fase 1, the pre-submission check (*voorinlevering*): political groups hand
//! in their package ahead of nomination day so the CSB can check the candidates
//! against the BRP and report the differences back, to be fixed before the
//! official submission. The imports live in their own registry (scope
//! [`crate::Scope::PreSubmittedToCsb`]), apart from the examination's, and
//! carry no omissions or corrections.
pub(in crate::csb) mod extractors;
mod pages;
mod paths;

pub use pages::router;
pub use paths::{CsbPreSubmissionImportPath, CsbPreSubmissionOverviewPath};
