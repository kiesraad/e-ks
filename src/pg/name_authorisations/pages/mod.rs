use axum::Router;
use axum_extra::routing::{RouterExt, TypedPath};
use serde::Deserialize;

use crate::{
    AppError, AppRequestState, QueryParamState,
    structs::name_authorisations::{NameAuthorisation, NameAuthorisationId},
};

mod create;
mod delete;
mod update;
mod view;

#[derive(TypedPath, Deserialize)]
#[typed_path("/political-group/name-authorisation", rejection(AppError))]
pub struct NameAuthorisationsPath;

#[derive(TypedPath)]
#[typed_path("/political-group/name-authorisation/create", rejection(AppError))]
pub struct NameAuthorisationCreatePath;

#[derive(TypedPath, Deserialize)]
#[typed_path(
    "/political-group/name-authorisation/{authorisation_id}/update",
    rejection(AppError)
)]
pub struct NameAuthorisationUpdatePath {
    pub authorisation_id: NameAuthorisationId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path(
    "/political-group/name-authorisation/{authorisation_id}/delete",
    rejection(AppError)
)]
pub struct NameAuthorisationDeletePath {
    pub authorisation_id: NameAuthorisationId,
}

impl NameAuthorisation {
    pub fn list_path() -> impl TypedPath {
        NameAuthorisationsPath {}
    }

    pub fn create_path() -> impl TypedPath {
        NameAuthorisationCreatePath {}
    }

    pub fn update_path(&self) -> impl TypedPath {
        NameAuthorisationUpdatePath {
            authorisation_id: self.id,
        }
    }

    pub fn update_path_from(&self, from: impl std::fmt::Display) -> impl TypedPath {
        self.update_path()
            .with_query_params(QueryParamState::redirect_to(from.to_string()))
    }

    pub fn delete_path(&self) -> impl TypedPath {
        NameAuthorisationDeletePath {
            authorisation_id: self.id,
        }
    }
}

pub fn router<S: AppRequestState>() -> Router<S> {
    Router::new()
        .typed_get(view::list_name_authorisations)
        .typed_get(create::create_name_authorisation)
        .typed_post(create::create_name_authorisation_submit)
        .typed_get(update::update_name_authorisation)
        .typed_post(update::update_name_authorisation_submit)
        .typed_get(delete::delete_name_authorisation_confirm)
        .typed_post(delete::delete_name_authorisation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        structs::name_authorisations::NameAuthorisationId, test_utils::sample_name_authorisation,
    };

    #[test]
    fn name_authorisation_paths_match_expected_routes() {
        let agent = sample_name_authorisation(NameAuthorisationId::new());

        assert_eq!(
            NameAuthorisation::list_path().to_string(),
            "/political-group/name-authorisation"
        );
        assert_eq!(
            NameAuthorisation::create_path().to_string(),
            "/political-group/name-authorisation/create"
        );
        assert_eq!(
            agent.update_path().to_string(),
            format!("/political-group/name-authorisation/{}/update", agent.id)
        );
        assert_eq!(
            agent.delete_path().to_string(),
            format!("/political-group/name-authorisation/{}/delete", agent.id)
        );
    }

    #[test]
    fn name_authorisation_router_builds() {
        let _router = router::<crate::AppState>();
    }
}
