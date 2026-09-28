use axum::{Router, extract::DefaultBodyLimit};
use axum_extra::routing::RouterExt;

use crate::AppRequestState;

use super::paths::{
    CandidateListCreatePath, CandidateListExportPath, CandidateListImportPath,
    CandidateListImportTemplatePath, CandidateListReorderPath, CandidateListUpdatePath,
    CandidateListsDeletePath, CandidateListsPath, ViewCandidateListPath,
};

mod create;
mod delete;
mod export;
mod import;
mod list;
mod reorder;
mod update;
mod view;

pub fn router<S: AppRequestState>() -> Router<S> {
    Router::new()
        // manage lists
        .typed_get(list::list_candidate_lists)
        // create a new list
        .typed_get(create::create_candidate_list)
        .typed_post(create::create_candidate_list_submit)
        // manage single list
        .typed_get(view::view_candidate_list)
        .typed_get(update::update_candidate_list)
        .typed_post(update::update_candidate_list_submit)
        .typed_get(delete::delete_candidate_list_confirm)
        .typed_post(delete::delete_candidate_list)
        .typed_post(reorder::reorder_candidate_list)
        .typed_get(import::import_export)
        .typed_get(export::export_candidate_list)
        .merge(
            Router::new()
                .typed_post(import::import_candidate_list)
                .layer(DefaultBodyLimit::max(import::MAX_IMPORT_SIZE_BYTES)),
        )
        .typed_get(import::download_import_template)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        structs::candidate_lists::{CandidateList, CandidateListId},
        test_utils::sample_candidate_list,
    };

    #[test]
    fn candidate_list_paths_match_expected_routes() {
        let list = sample_candidate_list(CandidateListId::new());

        assert_eq!(CandidateList::list_path().to_string(), "/candidate-lists");
        assert_eq!(
            CandidateList::create_path().to_string(),
            "/candidate-lists/create"
        );
        assert_eq!(
            list.update_path().to_string(),
            format!("/candidate-lists/{}/update", list.id)
        );
        assert_eq!(
            list.delete_path().to_string(),
            format!("/candidate-lists/{}/delete", list.id)
        );
        assert_eq!(
            list.view_path().to_string(),
            format!("/candidate-lists/{}", list.id)
        );
        assert_eq!(
            list.reorder_path().to_string(),
            format!("/candidate-lists/{}/reorder", list.id)
        );
        assert_eq!(
            list.add_candidate_path().to_string(),
            format!("/candidate-lists/{}/add", list.id)
        );
        assert_eq!(
            list.create_candidate_path().to_string(),
            format!("/candidate-lists/{}/create", list.id)
        );
        let event_hash: crate::EventHashPrefix = "abababababababab".parse().expect("prefix");
        assert_eq!(
            list.export_path(event_hash).to_string(),
            format!("/candidate-lists/{}/export/abababababababab", list.id)
        );
        assert_eq!(
            list.import_path().to_string(),
            format!("/candidate-lists/{}/import", list.id)
        );
    }

    #[test]
    fn candidate_list_after_create_path_includes_initial_query() {
        let list = sample_candidate_list(CandidateListId::new());
        let expected = format!("/candidate-lists/{}?&initial=true&success=true", list.id);

        assert_eq!(list.after_create_path().to_string(), expected);
    }

    #[test]
    fn candidate_list_router_builds() {
        let _router = router::<crate::AppState>();
    }
}
