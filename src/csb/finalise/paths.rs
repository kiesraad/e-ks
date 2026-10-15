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

#[derive(TypedPath)]
#[typed_path("/csb/finalise/afronding_kandidaatstellen.zip", rejection(AppError))]
pub struct CsbFinalZipDownloadPath;

#[derive(TypedPath)]
#[typed_path("/csb/finalise/concept_i4.pdf", rejection(AppError))]
pub struct CsbI4DraftDownloadPath;

#[derive(TypedPath)]
#[typed_path("/csb/finalise/concept_i4.docx", rejection(AppError))]
pub struct CsbI4DraftDocxDownloadPath;
