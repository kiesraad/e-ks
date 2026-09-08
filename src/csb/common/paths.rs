//! Typed paths for the pages shared across the CSB section.

use axum_extra::routing::TypedPath;
use serde::Deserialize;

use crate::AppError;

/// Every path under `/csb` that no CSB route claims (static and parameterised
/// routes take priority over the wildcard). Without it such a request would
/// reach the app router's fallback, whose store middleware redirects committee
/// sessions to the CSB index instead of answering with a not-found page.
#[derive(TypedPath, Deserialize)]
#[typed_path("/csb/{*path}", rejection(AppError))]
pub struct CsbNotFoundPath {
    pub path: String,
}
