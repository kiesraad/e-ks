//! Store-backed operations for [`NameAuthorisation`].

use crate::{AppError, PgEvent, PgStore, structs::name_authorisations::NameAuthorisation};

impl NameAuthorisation {
    pub async fn create(&self, store: &PgStore) -> Result<(), AppError> {
        store
            .update(PgEvent::CreateNameAuthorisation(Box::new(self.clone())))
            .await
    }

    pub async fn update(&self, store: &PgStore) -> Result<(), AppError> {
        store
            .update(PgEvent::UpdateNameAuthorisation(Box::new(self.clone())))
            .await
    }

    pub async fn delete(&self, store: &PgStore) -> Result<(), AppError> {
        store
            .update(PgEvent::DeleteNameAuthorisation(self.id))
            .await
    }
}
