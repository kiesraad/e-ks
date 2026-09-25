use crate::{
    AppError, PgStore,
    structs::{common::Appellation, political_groups::PoliticalGroup},
};

mod candidate_list;
mod persons;
mod political_groups;

/// Load the fixtures into an empty store as the demo political group, named
/// `appellation` when given.
pub async fn load(store: &PgStore, appellation: Option<Appellation>) -> Result<(), AppError> {
    load_for_group(store, political_groups::fixture_group(appellation)).await
}

/// Load the fixtures into an empty store as `political_group`: the persons,
/// the candidate lists, the group itself and its submitters.
pub async fn load_for_group(
    store: &PgStore,
    political_group: PoliticalGroup,
) -> Result<(), AppError> {
    let person_count = store.get_person_count();
    let candidate_list_count = store.get_candidate_list_count();

    if person_count > 0 && candidate_list_count > 0 {
        tracing::warn!("Skip loading fixtures, store not empty");

        return Ok(());
    }

    persons::load(store).await?;
    candidate_list::load(store).await?;
    political_groups::load(store, political_group).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{PgStore, fixtures::load};

    #[tokio::test]
    async fn test_load_all_fixtures() {
        let store = PgStore::new_for_test();
        load(&store, None).await.unwrap();
        let persons = crate::structs::persons::Person::list(
            &store,
            50,
            0,
            &crate::structs::persons::PersonSort::LastName,
            &crate::pagination::SortDirection::Asc,
        )
        .unwrap();

        assert_eq!(persons.len(), 50);
    }
}
