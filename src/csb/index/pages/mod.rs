use crate::AppRequestState;
use axum::Router;
use axum_extra::routing::RouterExt;

mod election_definition;
mod index;

pub fn router<S: AppRequestState>() -> Router<S> {
    Router::new()
        .typed_get(index::index)
        .typed_get(election_definition::download_election_definition)
}
