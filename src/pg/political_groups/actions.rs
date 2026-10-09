//! Store-backed operations for [`PoliticalGroup`].

use crate::{
    AppError, PgEvent, PgStore, PgStoreData, QueryParamState,
    structs::{list_designation::ListDesignation, political_groups::PoliticalGroup},
};
use axum_extra::routing::TypedPath;

impl PoliticalGroup {
    /// Check if the full general information section is empty
    pub fn is_general_information_empty(&self, data: &PgStoreData) -> bool {
        self.is_list_designation_type_empty()
            && self.is_group_information_empty()
            && data.name_authorisations().is_empty()
            && data.list_submitter().is_empty()
            && data.substitute_submitters().is_empty()
    }

    /// URL for the "General information" step.
    /// Includes `initial=true` when all fields are still empty, so the
    /// first-visit flow suppresses warnings for steps not yet reached.
    pub fn general_information_path(&self, data: &PgStoreData) -> String {
        if self.is_general_information_empty(data) {
            ListDesignation::update_path()
                .with_query_params(QueryParamState::initial())
                .to_string()
        } else {
            ListDesignation::update_path().to_string()
        }
    }

    pub async fn create(&self, store: &PgStore) -> Result<(), AppError> {
        store
            .update(PgEvent::UpdatePoliticalGroup(self.clone()))
            .await
    }

    pub async fn update(&self, store: &PgStore) -> Result<(), AppError> {
        store
            .update(PgEvent::UpdatePoliticalGroup(self.clone()))
            .await
    }
}
