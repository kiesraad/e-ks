//! Read accessors over the CSB main projection.

use crate::{
    CsbMainStoreData, StreamId,
    structs::{
        common::Appellation,
        csb::{
            HearingDetails, HearingModel, Objection, ObjectionId, RegisteredPoliticalGroup,
            RegisteredPoliticalGroupId,
        },
    },
};

impl CsbMainStoreData {
    /// The registered political groups in the order their lists are numbered
    /// on votes (Kieswet Art. I 14): most votes first.
    pub fn registered_political_groups(&self) -> Vec<&RegisteredPoliticalGroup> {
        let mut groups: Vec<&RegisteredPoliticalGroup> =
            self.registered_political_groups.iter().collect();
        groups.sort_by(|a, b| RegisteredPoliticalGroup::numbering_order(a, b));
        groups
    }

    /// The order the lists numbered by lot were drawn in, as the streams of
    /// their political groups; empty until the order is recorded.
    pub fn list_order(&self) -> &[StreamId] {
        &self.list_order
    }

    pub fn registered_political_group(
        &self,
        id: RegisteredPoliticalGroupId,
    ) -> Option<&RegisteredPoliticalGroup> {
        self.registered_political_groups
            .iter()
            .find(|group| group.id == id)
    }

    pub fn hearing_details(&self, model: HearingModel) -> Option<&HearingDetails> {
        self.hearing_details.get(&model)
    }

    /// Whether a registered group other than `except` already carries
    /// `appellation` (ignoring case).
    pub fn has_registered_appellation(
        &self,
        appellation: &Appellation,
        except: Option<RegisteredPoliticalGroupId>,
    ) -> bool {
        self.registered_political_groups
            .iter()
            .filter(|group| Some(group.id) != except)
            .any(|group| group.has_appellation(appellation))
    }

    pub fn objections(&self) -> &[Objection] {
        &self.objections
    }

    pub fn objection(&self, objection_id: ObjectionId) -> Option<&Objection> {
        self.objections.iter().find(|o| o.id == objection_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppError, CsbMainAction, CsbMainStore, CsbUser, OrNotFound,
        structs::csb::sample_registered_political_group,
    };

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
            .snapshot()
            .registered_political_groups()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
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
                .snapshot()
                .registered_political_group(group.id)
                .cloned()
                .or_not_found()?
                .previous_votes
                .value(),
            200
        );

        store
            .update(CsbMainAction::DeleteRegisteredPoliticalGroup(group.id).by(CsbUser::new_test()))
            .await?;
        assert!(matches!(
            store
                .snapshot()
                .registered_political_group(group.id)
                .cloned()
                .or_not_found(),
            Err(AppError::GenericNotFound)
        ));
        assert!(
            store
                .snapshot()
                .registered_political_groups()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>()
                .is_empty()
        );

        Ok(())
    }

    #[tokio::test]
    async fn list_order_is_empty_until_recorded_and_then_replaced() -> Result<(), AppError> {
        let store = store_with(&[]).await;
        assert!(store.snapshot().list_order().to_vec().is_empty());

        let first = StreamId::new();
        let second = StreamId::new();
        store
            .update(CsbMainAction::UpdateListOrder(vec![first, second]).by(CsbUser::new_test()))
            .await?;
        assert_eq!(store.snapshot().list_order().to_vec(), vec![first, second]);

        store
            .update(CsbMainAction::UpdateListOrder(vec![second, first]).by(CsbUser::new_test()))
            .await?;
        assert_eq!(store.snapshot().list_order().to_vec(), vec![second, first]);

        Ok(())
    }

    #[tokio::test]
    async fn updating_an_unknown_group_is_ignored() -> Result<(), AppError> {
        let store = store_with(&[]).await;
        let group = sample_registered_political_group("Partij", 100, 1);

        store
            .update(CsbMainAction::UpdateRegisteredPoliticalGroup(group).by(CsbUser::new_test()))
            .await?;

        assert!(
            store
                .snapshot()
                .registered_political_groups()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>()
                .is_empty()
        );
        Ok(())
    }

    #[tokio::test]
    async fn detects_duplicate_appellations_except_the_group_itself() {
        let group = sample_registered_political_group("De Partij", 100, 1);
        let store = store_with(std::slice::from_ref(&group)).await;
        let appellation: Appellation = "de partij".parse().unwrap();

        assert!(
            store
                .snapshot()
                .has_registered_appellation(&appellation, None)
        );
        assert!(
            !store
                .snapshot()
                .has_registered_appellation(&appellation, Some(group.id))
        );
        assert!(
            !store
                .snapshot()
                .has_registered_appellation(&"Andere".parse().unwrap(), None)
        );
    }
}
