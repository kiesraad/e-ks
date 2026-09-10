use axum::Router;
use axum_extra::routing::RouterExt;

use crate::AppRequestState;

pub(in crate::csb) use super::paths::{
    CsbPreSubmissionBrpCheckPath, CsbPreSubmissionGroupPath, CsbPreSubmissionImportPath,
    CsbPreSubmissionOverviewPath,
};

mod group;
mod import;
mod overview;

pub fn router<S: AppRequestState>() -> Router<S> {
    Router::new()
        .typed_get(overview::overview)
        .typed_get(import::import)
        .typed_post(import::import_submit::<S>)
        .typed_get(group::group)
        .typed_post(group::start_brp_check::<S>)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StreamId;

    #[test]
    fn paths_match_the_expected_routes() {
        let stream_id = StreamId::new();
        assert_eq!(
            CsbPreSubmissionOverviewPath.to_string(),
            "/csb/pre-submission"
        );
        assert_eq!(
            CsbPreSubmissionImportPath.to_string(),
            "/csb/pre-submission/import"
        );
        assert_eq!(
            CsbPreSubmissionGroupPath { stream_id }.to_string(),
            format!("/csb/pre-submission/{stream_id}")
        );
        assert_eq!(
            CsbPreSubmissionBrpCheckPath { stream_id }.to_string(),
            format!("/csb/pre-submission/{stream_id}/brp-check")
        );
    }

    #[test]
    fn router_builds() {
        let _router = router::<crate::AppState>();
    }
}
