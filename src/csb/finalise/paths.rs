//! Typed paths for the CSB finalise routes.

use axum_extra::routing::TypedPath;
use serde::Deserialize;

use crate::{
    AppError,
    structs::csb::{Objection, ObjectionId},
};

#[derive(TypedPath)]
#[typed_path("/csb/finalise", rejection(AppError))]
pub struct CsbFinalisePath;

/// Records the order drawn by lot; posted as JSON by the sortable table.
#[derive(TypedPath)]
#[typed_path("/csb/finalise/order", rejection(AppError))]
pub struct CsbListOrderPath;

#[derive(TypedPath)]
#[typed_path("/csb/finalise/objection/add", rejection(AppError))]
pub struct CsbAddObjectionPath;

#[derive(TypedPath)]
#[typed_path(
    "/csb/examination/publicatie-kandidatenlijsten.pdf",
    rejection(AppError)
)]
pub struct CsbCandidateListPublicationDownloadPath;

#[derive(TypedPath)]
#[typed_path(
    "/csb/examination/publicatie-kandidatenlijsten.docx",
    rejection(AppError)
)]
pub struct CsbCandidateListPublicationDocxDownloadPath;

#[derive(TypedPath, Deserialize)]
#[typed_path(
    "/csb/examination/overzicht-kandidatenlijsten.pdf",
    rejection(AppError)
)]
pub struct CsbCandidateListOverviewDownloadPath;

#[derive(TypedPath, Deserialize)]
#[typed_path(
    "/csb/examination/overzicht-kandidatenlijsten.docx",
    rejection(AppError)
)]
pub struct CsbCandidateListOverviewDocxDownloadPath;

#[derive(TypedPath, Deserialize)]
#[typed_path("/csb/finalise/objection/update/{id}", rejection(AppError))]
pub struct CsbUpdateObjectionPath {
    pub id: ObjectionId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path("/csb/finalise/objection/delete/{id}", rejection(AppError))]
pub struct CsbDeleteObjectionPath {
    pub id: ObjectionId,
}

impl Objection {
    pub fn update_path(&self) -> impl TypedPath {
        CsbUpdateObjectionPath { id: self.id }
    }

    pub fn delete_path(&self) -> impl TypedPath {
        CsbDeleteObjectionPath { id: self.id }
    }
}

/// Temporary EML ZIP endpoint, that should eventually be expanded to contain everything
#[derive(TypedPath)]
#[typed_path("/csb/finalise/eml.zip", rejection(AppError))]
pub struct CsbEmlZipDownloadPath;

#[derive(TypedPath)]
#[typed_path("/csb/finalise/i4.pdf", rejection(AppError))]
pub struct CsbI4FinalDownloadPath;

#[derive(TypedPath)]
#[typed_path("/csb/finalise/i4.docx", rejection(AppError))]
pub struct CsbI4FinalDocxDownloadPath;


#[derive(TypedPath)]
#[typed_path("/csb/finalise/candidate-lists.zip", rejection(AppError))]
pub struct CsbCandidateListPerDistrictDownloadPath;
