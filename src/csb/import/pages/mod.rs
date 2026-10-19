use axum::Router;
use axum_extra::routing::RouterExt;

use crate::AppRequestState;

pub(crate) use super::paths::{CsbCreateEmptyPath, CsbImportPath};

mod import;

pub use import::{
    BrpSweepStatus, brp_sweep_running, brp_sweep_status, do_brp_verification, is_brp_status_poll,
};
pub(in crate::csb) use import::{ImportForm, ImportResult, import_package};

#[cfg(test)]
pub use import::claim_sweep_for_test;

pub fn router<S: AppRequestState>() -> Router<S> {
    Router::new()
        .typed_get(import::import)
        .typed_post(import::import_submit::<S>)
        .typed_post(import::create_empty::<S>)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csb_import_path_matches_expected_route() {
        assert_eq!(CsbImportPath {}.to_string(), "/csb/import");
    }

    #[test]
    fn csb_import_router_builds() {
        let _router = router::<crate::AppState>();
    }
}
