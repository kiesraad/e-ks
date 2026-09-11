//! CSB import flow and related routes.
mod pages;
mod paths;

/// Only reachable from the development CSB logins, which need `dev-features`.
#[cfg(all(feature = "fixtures", feature = "dev-features"))]
pub mod fixture;

pub(in crate::csb) use pages::{ImportForm, ImportResult, import_package};
pub use pages::{brp_sweep_running, do_brp_verification, router};

#[cfg(test)]
pub use pages::claim_sweep_for_test;
pub use paths::{CsbCreateEmptyPath, CsbImportPath};
