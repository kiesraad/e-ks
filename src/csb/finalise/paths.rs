//! Typed paths for the CSB finalise routes.

use axum_extra::routing::TypedPath;

use crate::AppError;

#[derive(TypedPath)]
#[typed_path("/csb/finalise", rejection(AppError))]
pub struct CsbFinalisePath;

/// Records the order drawn by lot; posted as JSON by the sortable table.
#[derive(TypedPath)]
#[typed_path("/csb/finalise/order", rejection(AppError))]
pub struct CsbListOrderPath;
