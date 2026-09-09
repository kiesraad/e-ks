//! Typed paths for the pre-submission routes.

use axum_extra::routing::TypedPath;
use serde::Deserialize;

use crate::{AppError, StreamId};

#[derive(TypedPath)]
#[typed_path("/csb/pre-submission", rejection(AppError))]
pub struct CsbPreSubmissionOverviewPath;

#[derive(TypedPath)]
#[typed_path("/csb/pre-submission/import", rejection(AppError))]
pub struct CsbPreSubmissionImportPath;

#[derive(TypedPath, Deserialize)]
#[typed_path("/csb/pre-submission/{stream_id}", rejection(AppError))]
pub struct CsbPreSubmissionGroupPath {
    pub stream_id: StreamId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path("/csb/pre-submission/{stream_id}/brp-check", rejection(AppError))]
pub struct CsbPreSubmissionBrpCheckPath {
    pub stream_id: StreamId,
}
