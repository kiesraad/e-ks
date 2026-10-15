//! Store-backed operations for [`ListSubmitter`].

use crate::{AppError, PgEvent, PgStore, structs::list_submitters::ListSubmitter};

impl ListSubmitter {
    pub async fn update(&self, store: &PgStore) -> Result<(), AppError> {
        store
            .update(PgEvent::UpdateListSubmitter(Box::new(self.clone())))
            .await
    }

    pub async fn create_substitute(&self, store: &PgStore) -> Result<(), AppError> {
        store
            .update(PgEvent::CreateSubstituteSubmitter(Box::new(self.clone())))
            .await
    }

    pub async fn update_substitute(&self, store: &PgStore) -> Result<(), AppError> {
        store
            .update(PgEvent::UpdateSubstituteSubmitter(Box::new(self.clone())))
            .await
    }

    pub async fn delete_substitute(&self, store: &PgStore) -> Result<(), AppError> {
        store
            .update(PgEvent::DeleteSubstituteSubmitter {
                substitute_submitter_id: self.id,
            })
            .await
    }
}
