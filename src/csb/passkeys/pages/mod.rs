use axum::Router;
use axum_extra::routing::RouterExt;

use crate::AppRequestState;

mod delete;
mod delete_account;
mod list;
mod register;

pub fn router<S: AppRequestState>() -> Router<S> {
    Router::new()
        .typed_get(list::list::<S>)
        .typed_post(register::register_start::<S>)
        .typed_post(register::register_finish::<S>)
        .typed_get(delete::delete::<S>)
        .typed_post(delete::delete_submit::<S>)
        .typed_get(delete_account::delete_account::<S>)
        .typed_post(delete_account::delete_account_submit::<S>)
}
