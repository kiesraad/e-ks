use axum::Router;
use axum_extra::routing::RouterExt;

use crate::AppRequestState;

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
}
