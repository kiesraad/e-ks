//! Store-backed operations for [`Person`].

use crate::{
    AppError, OrNotFound, PgEvent, PgStore,
    pagination::SortDirection,
    structs::{
        common::{DutchAddress, FullName, Problematic},
        persons::{
            Person, PersonId, PersonSort, PersonWithProblems, PersonalData, Representative,
            compare_persons,
        },
    },
};

impl Person {
    pub async fn create_from_personal_data(
        store: &PgStore,
        name: FullName,
        personal_data: PersonalData,
    ) -> Result<Person, AppError> {
        let person_id = PersonId::new();

        store
            .update(PgEvent::CreatePersonPersonalData {
                person_id,
                name,
                personal_data,
            })
            .await?;

        store.snapshot().person(person_id).cloned().or_not_found()
    }

    pub async fn update_personal_data(
        &self,
        store: &PgStore,
        name: FullName,
        personal_data: PersonalData,
    ) -> Result<Person, AppError> {
        store
            .update(PgEvent::UpdatePersonPersonalData {
                person_id: self.id,
                name: name.clone(),
                personal_data: personal_data.clone(),
            })
            .await?;

        store.snapshot().person(self.id).cloned().or_not_found()
    }

    pub async fn create(&self, store: &PgStore) -> Result<(), AppError> {
        store.update(PgEvent::CreatePerson(self.clone())).await
    }

    pub async fn update(&self, store: &PgStore) -> Result<(), AppError> {
        store.update(PgEvent::UpdatePerson(self.clone())).await
    }

    pub async fn update_representative(
        &self,
        store: &PgStore,
        representative: Option<Representative>,
    ) -> Result<(), AppError> {
        store
            .update(PgEvent::UpdatePersonRepresentative {
                person_id: self.id,
                representative,
            })
            .await
    }

    /// Persist a validated representative, refreshing its BAG flag first.
    pub async fn save_representative(
        &self,
        store: &PgStore,
        mut representative: Representative,
    ) -> Result<(), AppError> {
        representative.address.update_is_known_in_bag();
        self.update_representative(store, Some(representative))
            .await
    }

    pub async fn update_address(
        &self,
        store: &PgStore,
        address: DutchAddress,
    ) -> Result<(), AppError> {
        store
            .update(PgEvent::UpdatePersonAddress {
                person_id: self.id,
                address,
            })
            .await
    }

    /// Persist this person's (validated) address, refreshing its BAG flag first.
    pub async fn save_address(&mut self, store: &PgStore) -> Result<(), AppError> {
        self.address.update_is_known_in_bag();
        self.update_address(store, self.address.clone()).await
    }

    pub async fn delete(&self, store: &PgStore) -> Result<(), AppError> {
        store
            .update(PgEvent::DeletePerson { person_id: self.id })
            .await
    }

    pub fn list(
        store: &PgStore,
        limit: usize,
        offset: usize,
        sort_field: &PersonSort,
        sort_direction: &SortDirection,
    ) -> Result<Vec<PersonWithProblems>, AppError> {
        let mut persons = store.snapshot().persons().cloned().collect::<Vec<_>>();
        persons.sort_by(|a, b| compare_persons(a, b, sort_field));

        if matches!(sort_direction, SortDirection::Desc) {
            persons.reverse();
        }

        Ok(persons
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|person| PersonWithProblems {
                problems: person.get_problems(store.election),
                data: person,
            })
            .collect())
    }
}
