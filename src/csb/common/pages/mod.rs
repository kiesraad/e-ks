use axum::Router;
use axum_extra::routing::RouterExt;

use crate::AppRequestState;

mod not_found;

pub fn router<S: AppRequestState>() -> Router<S> {
    Router::new().typed_get(not_found::not_found)
}
