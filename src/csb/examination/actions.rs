//! Store-backed operations for omissions.

use crate::{
    AppError, CsbAction, CsbStore,
    structs::csb::{Omission, OmissionPart, OmissionStatus},
};

impl Omission {
    pub async fn create(&self, store: &CsbStore) -> Result<(), AppError> {
        store.update(CsbAction::CreateOmission(self.clone())).await
    }

    pub async fn update(&self, store: &CsbStore) -> Result<(), AppError> {
        store.update(CsbAction::UpdateOmission(self.clone())).await
    }

    pub async fn delete(&self, store: &CsbStore) -> Result<(), AppError> {
        store
            .update(CsbAction::DeleteOmission {
                omission_id: self.id,
            })
            .await
    }

    /// Record whether this omission was recovered during the "Herstelde
    /// lijsten" phase. Irreparable omissions cannot be assessed.
    pub async fn set_status(
        &self,
        store: &CsbStore,
        status: OmissionStatus,
    ) -> Result<(), AppError> {
        if !self.is_actionable() {
            return Err(AppError::UserError(
                "an irreparable omission cannot be assessed".to_string(),
            ));
        }

        store
            .update(CsbAction::SetOmissionStatus {
                omission_id: self.id,
                status,
            })
            .await
    }

    /// Record the recovery decision for one part of this omission: an
    /// electoral district, or a candidate list. The projection splits the part
    /// off while the omission covers other parts, so those keep waiting for
    /// their own decision, and reads parts decided the same way as one
    /// omission again (see [`CsbAction::SetOmissionPartStatus`]).
    pub async fn set_part_status(
        &self,
        store: &CsbStore,
        part: OmissionPart,
        status: OmissionStatus,
    ) -> Result<(), AppError> {
        if !self.is_actionable() {
            return Err(AppError::UserError(
                "an irreparable omission cannot be assessed".to_string(),
            ));
        }
        if !self.covers(&store.election, part) {
            return Err(AppError::UserError(format!(
                "the omission was not reported for {part:?}"
            )));
        }

        // A legacy omission covers all districts as none; spell them out, so
        // the projection knows which ones remain after the split.
        if let Some(explicit) = self.with_explicit_districts(&store.election) {
            explicit.update(store).await?;
        }

        store
            .update(CsbAction::SetOmissionPartStatus {
                omission_id: self.id,
                part,
                status,
            })
            .await
    }
}
