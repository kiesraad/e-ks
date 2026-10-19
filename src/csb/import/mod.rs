//! CSB import flow and related routes.
mod pages;
mod paths;

/// Only reachable from the development CSB logins, which need `dev-features`.
#[cfg(all(feature = "fixtures", feature = "dev-features"))]
pub mod fixture;

pub use pages::{
    BrpSweepStatus, brp_sweep_running, brp_sweep_status, do_brp_verification, is_brp_status_poll,
    router,
};
pub(in crate::csb) use pages::{ImportForm, ImportResult, import_package};

#[cfg(test)]
pub use pages::claim_sweep_for_test;
pub use paths::{CsbCreateEmptyPath, CsbImportPath};
