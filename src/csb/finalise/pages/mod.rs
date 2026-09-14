use axum::Router;
use axum_extra::routing::RouterExt;

use crate::AppRequestState;

mod order;
mod overview;

pub fn router<S: AppRequestState>() -> Router<S> {
    Router::new()
        .typed_get(overview::overview)
        .typed_post(order::update_order)
}
