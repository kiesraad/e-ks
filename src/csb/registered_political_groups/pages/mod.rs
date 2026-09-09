use axum::Router;
use axum_extra::routing::RouterExt;

use crate::AppRequestState;

mod delete;
mod edit;
mod list;

pub fn router<S: AppRequestState>() -> Router<S> {
    Router::new()
        .typed_get(list::list)
        .typed_get(edit::add)
        .typed_post(edit::add_submit)
        .typed_get(edit::edit)
        .typed_post(edit::edit_submit)
        .typed_get(delete::delete)
        .typed_post(delete::delete_submit)
}
