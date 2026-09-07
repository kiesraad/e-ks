//! Typed paths for the registered political groups routes, plus the path
//! helpers templates use on a [`RegisteredPoliticalGroup`].

use axum_extra::routing::TypedPath;
use serde::Deserialize;

use crate::{
    AppError,
    structs::csb::{RegisteredPoliticalGroup, RegisteredPoliticalGroupId},
};

#[derive(TypedPath)]
#[typed_path("/csb/registered-political-groups", rejection(AppError))]
pub struct CsbRegisteredPoliticalGroupsPath;

#[derive(TypedPath)]
#[typed_path("/csb/registered-political-groups/add", rejection(AppError))]
pub struct CsbAddRegisteredPoliticalGroupPath;

#[derive(TypedPath, Deserialize)]
#[typed_path("/csb/registered-political-groups/{id}", rejection(AppError))]
pub struct CsbEditRegisteredPoliticalGroupPath {
    pub id: RegisteredPoliticalGroupId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path("/csb/registered-political-groups/{id}/delete", rejection(AppError))]
pub struct CsbDeleteRegisteredPoliticalGroupPath {
    pub id: RegisteredPoliticalGroupId,
}

impl RegisteredPoliticalGroup {
    pub fn edit_path(&self) -> CsbEditRegisteredPoliticalGroupPath {
        CsbEditRegisteredPoliticalGroupPath { id: self.id }
    }

    pub fn delete_path(&self) -> CsbDeleteRegisteredPoliticalGroupPath {
        CsbDeleteRegisteredPoliticalGroupPath { id: self.id }
    }
}
