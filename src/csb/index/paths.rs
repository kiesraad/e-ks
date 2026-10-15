//! Typed paths for the CSB index route.

use axum_extra::routing::TypedPath;

use crate::AppError;

#[derive(TypedPath)]
#[typed_path("/csb", rejection(AppError))]
pub struct CsbIndexPath;
