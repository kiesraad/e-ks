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
#[typed_path("/csb/pre-submission/{stream_id}/delete", rejection(AppError))]
pub struct CsbPreSubmissionDeleteGroupPath {
    pub stream_id: StreamId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path("/csb/pre-submission/{stream_id}/brp-check", rejection(AppError))]
pub struct CsbPreSubmissionBrpCheckPath {
    pub stream_id: StreamId,
}

/// Whether the package's BRP sweep is still running, as JSON for the script
/// that tells the user when it finished.
#[derive(TypedPath, Deserialize)]
#[typed_path(
    "/csb/pre-submission/{stream_id}/brp-check/status",
    rejection(AppError)
)]
pub struct CsbPreSubmissionBrpCheckStatusPath {
    pub stream_id: StreamId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path(
    "/csb/pre-submission/{stream_id}/brp-overzicht.pdf",
    rejection(AppError)
)]
pub struct CsbPreSubmissionBrpOverviewPdfPath {
    pub stream_id: StreamId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path(
    "/csb/pre-submission/{stream_id}/brp-overzicht.docx",
    rejection(AppError)
)]
pub struct CsbPreSubmissionBrpOverviewDocxPath {
    pub stream_id: StreamId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path(
    "/csb/pre-submission/{stream_id}/brp-overzicht.md",
    rejection(AppError)
)]
pub struct CsbPreSubmissionBrpOverviewMarkdownPath {
    pub stream_id: StreamId,
}
