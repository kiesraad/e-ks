//! Typed paths for the finalise routes.

use axum_extra::routing::TypedPath;
use serde::Deserialize;

use crate::{AppError, EventHashPrefix, core::ModelLocale};

#[derive(TypedPath, Deserialize)]
#[typed_path("/finalise", rejection(AppError))]
pub struct FinalisePath;

/// `event_hash` binds the link to the stream; see [`EventHashPrefix`].
#[derive(TypedPath, Deserialize)]
#[typed_path("/generate/{event_hash}/{locale}/documents.zip", rejection(AppError))]
pub struct DownloadDocumentsPath {
    pub event_hash: EventHashPrefix,
    pub locale: ModelLocale,
}
