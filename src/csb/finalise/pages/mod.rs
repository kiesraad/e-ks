use axum::Router;
use axum_extra::routing::RouterExt;

use crate::AppRequestState;

mod candidate_list_overview;
mod candidate_list_publication;
mod eml_zip;
mod i4;
mod objection;
mod order;
mod overview;

pub fn router<S: AppRequestState>() -> Router<S> {
    Router::new()
        .typed_get(overview::overview)
        .typed_post(order::update_order)
        .typed_get(objection::add_objection)
        .typed_post(objection::add_objection_submit)
        .typed_get(objection::update_objection)
        .typed_post(objection::update_objection_submit)
        .typed_post(objection::delete_objection)
        .typed_get(eml_zip::download_eml_zip::<S>)
        .typed_get(i4::gen_i4_final::<S>)
        .typed_get(i4::gen_i4_final_docx::<S>)
        .typed_get(i4::gen_i4_draft::<S>)
        .typed_get(i4::gen_i4_draft_docx::<S>)
        .typed_get(candidate_list_overview::gen_candidate_list_overview_docx::<S>)
        .typed_get(candidate_list_overview::gen_candidate_list_overview::<S>)
        .typed_get(candidate_list_publication::gen_candidate_list_publication::<S>)
        .typed_get(candidate_list_publication::gen_candidate_list_publication_docx::<S>)
}
