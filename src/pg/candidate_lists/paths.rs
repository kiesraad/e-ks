//! Typed paths for candidate-list routes and path helpers on [`CandidateList`].

use axum_extra::routing::TypedPath;
use serde::Deserialize;

use crate::{
    AppError, EventHashPrefix, QueryParamState,
    structs::{
        candidate_lists::{CandidateList, CandidateListId},
        persons::PersonId,
    },
};

#[derive(TypedPath, Deserialize)]
#[typed_path("/candidate-lists", rejection(AppError))]
pub struct CandidateListsPath;

#[derive(TypedPath)]
#[typed_path("/candidate-lists/create", rejection(AppError))]
pub struct CandidateListCreatePath;

#[derive(TypedPath, Deserialize)]
#[typed_path("/candidate-lists/{list_id}", rejection(AppError))]
pub struct ViewCandidateListPath {
    pub list_id: CandidateListId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path("/candidate-lists/{list_id}/update", rejection(AppError))]
pub struct CandidateListUpdatePath {
    pub list_id: CandidateListId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path("/candidate-lists/{list_id}/delete", rejection(AppError))]
pub struct CandidateListsDeletePath {
    pub list_id: CandidateListId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path("/candidate-lists/{list_id}/reorder", rejection(AppError))]
pub struct CandidateListReorderPath {
    pub list_id: CandidateListId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path("/candidate-lists/{list_id}/export/{event_hash}", rejection(AppError))]
pub struct CandidateListExportPath {
    pub list_id: CandidateListId,
    /// Binds the link to the stream; see [`EventHashPrefix`].
    pub event_hash: EventHashPrefix,
}

#[derive(TypedPath, Deserialize)]
#[typed_path("/candidate-lists/export-template", rejection(AppError))]
pub struct CandidateListImportTemplatePath;

#[derive(TypedPath, Deserialize)]
#[typed_path("/candidate-lists/{list_id}/import", rejection(AppError))]
pub struct CandidateListImportPath {
    pub list_id: CandidateListId,
}

impl CandidateList {
    pub fn list_path() -> impl TypedPath {
        CandidateListsPath {}
    }

    pub fn highlight_path(&self, person_id: PersonId) -> impl TypedPath {
        ViewCandidateListPath { list_id: self.id }
            .with_query_params(QueryParamState::highlight(person_id.into()))
    }

    pub fn highlight_success_path(&self, person_id: PersonId) -> impl TypedPath {
        ViewCandidateListPath { list_id: self.id }
            .with_query_params(QueryParamState::highlight_success(person_id.into()))
    }

    pub fn highlight_last_success_path(&self, last: usize) -> impl TypedPath {
        ViewCandidateListPath { list_id: self.id }
            .with_query_params(QueryParamState::highlight_last_success(last))
    }

    pub fn create_path() -> impl TypedPath {
        CandidateListCreatePath {}
    }

    pub fn update_path(&self) -> impl TypedPath {
        CandidateListUpdatePath { list_id: self.id }
    }

    pub fn update_path_from(&self, from: impl std::fmt::Display) -> impl TypedPath {
        CandidateListUpdatePath { list_id: self.id }
            .with_query_params(QueryParamState::redirect_to(from.to_string()))
    }

    pub fn delete_path(&self) -> impl TypedPath {
        CandidateListsDeletePath { list_id: self.id }
    }

    pub fn view_path(&self) -> impl TypedPath {
        ViewCandidateListPath { list_id: self.id }
    }

    pub fn max_candidates_reached_path(&self) -> impl TypedPath {
        ViewCandidateListPath { list_id: self.id }
            .with_query_params(QueryParamState::max_candidates_reached())
    }

    pub fn import_capped_path(&self) -> impl TypedPath {
        ViewCandidateListPath { list_id: self.id }
            .with_query_params(QueryParamState::import_capped())
    }

    pub fn reorder_path(&self) -> impl TypedPath {
        CandidateListReorderPath { list_id: self.id }
    }

    pub fn add_candidate_path(&self) -> impl TypedPath {
        crate::candidates::AddCandidatePath { list_id: self.id }
    }

    pub fn create_candidate_path(&self) -> impl TypedPath {
        crate::candidates::CreateCandidatePath { list_id: self.id }
    }

    pub fn after_create_path(&self) -> impl TypedPath {
        ViewCandidateListPath { list_id: self.id }.with_query_params(QueryParamState::created())
    }

    pub fn export_path(&self, event_hash: EventHashPrefix) -> impl TypedPath {
        CandidateListExportPath {
            list_id: self.id,
            event_hash,
        }
    }

    pub fn import_path(&self) -> impl TypedPath {
        CandidateListImportPath { list_id: self.id }
    }

    pub fn import_template_path() -> impl TypedPath {
        CandidateListImportTemplatePath
    }
}
