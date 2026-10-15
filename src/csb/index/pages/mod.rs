use crate::AppRequestState;
use axum::Router;
use axum_extra::routing::RouterExt;

mod index;

pub fn router<S: AppRequestState>() -> Router<S> {
    Router::new().typed_get(index::index)
}
