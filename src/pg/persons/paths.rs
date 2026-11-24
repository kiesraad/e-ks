//! Typed paths for person routes and path helpers on [`Person`].

use axum_extra::routing::TypedPath;
use serde::Deserialize;

use crate::{
    AppError, QueryParamState,
    structs::persons::{Person, PersonId},
};

#[derive(TypedPath, Deserialize)]
#[typed_path("/persons", rejection(AppError))]
pub struct PersonsPath;

#[derive(TypedPath)]
#[typed_path("/persons/create", rejection(AppError))]
pub struct PersonsCreatePath;

#[derive(TypedPath, Deserialize)]
#[typed_path("/persons/{person_id}/update", rejection(AppError))]
pub struct UpdatePersonPath {
    pub person_id: PersonId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path("/persons/{person_id}/delete", rejection(AppError))]
pub struct DeletePersonPath {
    pub person_id: PersonId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path("/persons/{person_id}/address", rejection(AppError))]
pub struct UpdatePersonAddressPath {
    pub person_id: PersonId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path("/persons/{person_id}/representative", rejection(AppError))]
pub struct UpdateRepresentativePath {
    pub person_id: PersonId,
}

impl Person {
    pub fn list_path() -> impl TypedPath {
        PersonsPath {}
    }

    pub fn highlight_path(&self) -> impl TypedPath {
        PersonsPath {}.with_query_params(QueryParamState::highlight(self.id.into()))
    }

    pub fn highlight_success_path(&self) -> impl TypedPath {
        PersonsPath {}.with_query_params(QueryParamState::highlight_success(self.id.into()))
    }

    pub fn create_path() -> impl TypedPath {
        PersonsCreatePath {}
    }

    pub fn update_path(&self) -> impl TypedPath {
        UpdatePersonPath { person_id: self.id }
    }

    pub fn update_path_from(&self, from: impl std::fmt::Display) -> impl TypedPath {
        self.update_path()
            .with_query_params(QueryParamState::redirect_to(from.to_string()))
    }

    pub fn delete_path(&self) -> impl TypedPath {
        DeletePersonPath { person_id: self.id }
    }

    pub fn update_address_path(&self) -> impl TypedPath {
        UpdatePersonAddressPath { person_id: self.id }
    }

    pub fn update_representative_path(&self) -> impl TypedPath {
        UpdateRepresentativePath { person_id: self.id }
    }

    pub fn after_update_path(&self) -> String {
        if self.needs_representative() {
            self.update_representative_path().to_string()
        } else {
            self.update_address_path().to_string()
        }
    }

    pub fn after_create_path(&self) -> String {
        if !self.needs_representative() {
            UpdatePersonAddressPath { person_id: self.id }
                .with_query_params(QueryParamState::created())
                .to_string()
        } else {
            UpdateRepresentativePath { person_id: self.id }
                .with_query_params(QueryParamState::created())
                .to_string()
        }
    }
}
