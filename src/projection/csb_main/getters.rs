//! Read accessors over the CSB main projection.

use crate::{
    AppError, CsbMainStore,
    structs::{
        common::Appellation,
        csb::{HearingDetails, RegisteredPoliticalGroup, RegisteredPoliticalGroupId},
    },
};

impl CsbMainStore {
    /// The registered political groups in the order their lists are numbered
    /// on votes (Kieswet Art. I 14): most votes first.
    pub fn registered_political_groups(&self) -> Vec<RegisteredPoliticalGroup> {
        let mut groups = self.data.read().registered_political_groups.clone();
        groups.sort_by(RegisteredPoliticalGroup::numbering_order);
        groups
    }

    pub fn get_registered_political_group(
        &self,
        id: RegisteredPoliticalGroupId,
    ) -> Result<RegisteredPoliticalGroup, AppError> {
        self.data
            .read()
            .registered_political_groups
            .iter()
            .find(|group| group.id == id)
            .cloned()
            .ok_or(AppError::GenericNotFound)
    }

    pub fn get_hearing_details(&self) -> HearingDetails {
        self.data.read().hearing_details.clone()
    }

    /// Whether a registered group other than `except` already carries
    /// `appellation` (ignoring case).
    pub fn has_registered_appellation(
        &self,
        appellation: &Appellation,
        except: Option<RegisteredPoliticalGroupId>,
    ) -> bool {
        self.data
            .read()
            .registered_political_groups
            .iter()
            .filter(|group| Some(group.id) != except)
            .any(|group| group.has_appellation(appellation))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CsbMainAction, CsbUser, structs::csb::sample_registered_political_group};

    async fn store_with(groups: &[RegisteredPoliticalGroup]) -> CsbMainStore {
        let store = CsbMainStore::new_for_test();
        for group in groups {
            store
                .update(
                    CsbMainAction::CreateRegisteredPoliticalGroup(group.clone())
                        .by(CsbUser::new_test()),
                )
                .await
                .unwrap();
        }
        store
    }

    #[tokio::test]
    async fn lists_groups_most_votes_first() {
        let store = store_with(&[
            sample_registered_political_group("Klein", 10, 0),
            sample_registered_political_group("Groot", 1000, 5),
            sample_registered_political_group("Midden", 500, 2),
        ])
        .await;

        let names: Vec<_> = store
            .registered_political_groups()
            .iter()
            .map(|g| g.appellation.to_string())
            .collect();
        assert_eq!(names, ["Groot", "Midden", "Klein"]);
    }

    #[tokio::test]
    async fn update_replaces_and_delete_removes_a_group() -> Result<(), AppError> {
        let mut group = sample_registered_political_group("Partij", 100, 1);
        let store = store_with(std::slice::from_ref(&group)).await;

        group.previous_votes = 200.into();
        store
            .update(
                CsbMainAction::UpdateRegisteredPoliticalGroup(group.clone())
                    .by(CsbUser::new_test()),
            )
            .await?;
        assert_eq!(
            store
                .get_registered_political_group(group.id)?
                .previous_votes
                .value(),
            200
        );

        store
            .update(CsbMainAction::DeleteRegisteredPoliticalGroup(group.id).by(CsbUser::new_test()))
            .await?;
        assert!(matches!(
            store.get_registered_political_group(group.id),
            Err(AppError::GenericNotFound)
        ));
        assert!(store.registered_political_groups().is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn updating_an_unknown_group_is_ignored() -> Result<(), AppError> {
        let store = store_with(&[]).await;
        let group = sample_registered_political_group("Partij", 100, 1);

        store
            .update(CsbMainAction::UpdateRegisteredPoliticalGroup(group).by(CsbUser::new_test()))
            .await?;

        assert!(store.registered_political_groups().is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn detects_duplicate_appellations_except_the_group_itself() {
        let group = sample_registered_political_group("De Partij", 100, 1);
        let store = store_with(std::slice::from_ref(&group)).await;
        let appellation: Appellation = "de partij".parse().unwrap();

        assert!(store.has_registered_appellation(&appellation, None));
        assert!(!store.has_registered_appellation(&appellation, Some(group.id)));
        assert!(!store.has_registered_appellation(&"Andere".parse().unwrap(), None));
    }
}
