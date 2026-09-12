use std::collections::BTreeSet;

use crate::{
    AppError, PgStore,
    candidate_lists::pages::CandidateListReorderPath,
    structs::{candidate_lists::CandidateList, persons::PersonId},
};
use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct CandidateListReorderPayload {
    pub person_ids: Vec<PersonId>,
}

/// Only permutes the current candidates.
pub async fn reorder_candidate_list(
    _: CandidateListReorderPath,
    mut candidate_list: CandidateList,
    store: PgStore,
    Json(payload): Json<CandidateListReorderPayload>,
) -> Result<impl IntoResponse, AppError> {
    let submitted: BTreeSet<PersonId> = payload.person_ids.iter().copied().collect();
    let current: BTreeSet<PersonId> = candidate_list.candidates.iter().copied().collect();

    if submitted.len() != payload.person_ids.len() {
        return Err(AppError::DuplicateCandidate);
    }
    if submitted != current {
        return Err(AppError::CandidateSetChanged);
    }

    candidate_list
        .update_order(&store, &payload.person_ids)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        PgStore,
        structs::{
            candidate_lists::{CandidateListId, FullCandidateList},
            persons::PersonId,
        },
        test_utils::{sample_candidate_list, sample_person_with_last_name},
    };

    #[tokio::test]
    async fn reorder_candidate_list_updates_positions() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        let person_a = sample_person_with_last_name(PersonId::new(), "Jansen");
        let person_b = sample_person_with_last_name(PersonId::new(), "Bakker");

        list.create(&store).await?;
        person_a.create(&store).await?;
        person_b.create(&store).await?;
        list.clone()
            .update_order(&store, &[person_a.id, person_b.id])
            .await?;

        let response = reorder_candidate_list(
            CandidateListReorderPath { list_id },
            store.get_candidate_list(list_id)?,
            store.clone(),
            Json(CandidateListReorderPayload {
                person_ids: vec![person_b.id, person_a.id],
            }),
        )
        .await?
        .into_response();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let full_list = FullCandidateList::get(&store, list_id).expect("candidate list");
        assert_eq!(full_list.candidates.len(), 2);
        assert_eq!(full_list.candidates[0].data.person.id, person_b.id);
        assert_eq!(full_list.candidates[1].data.person.id, person_a.id);

        Ok(())
    }

    async fn two_candidate_list(
        store: &PgStore,
    ) -> Result<(CandidateList, PersonId, PersonId), AppError> {
        let list = sample_candidate_list(CandidateListId::new());
        let person_a = sample_person_with_last_name(PersonId::new(), "Jansen");
        let person_b = sample_person_with_last_name(PersonId::new(), "Bakker");

        list.create(store).await?;
        person_a.create(store).await?;
        person_b.create(store).await?;
        let mut list = store.get_candidate_list(list.id)?;
        list.update_order(store, &[person_a.id, person_b.id])
            .await?;

        Ok((list, person_a.id, person_b.id))
    }

    #[tokio::test]
    async fn reorder_rejects_a_repeated_candidate() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let (list, person_a, person_b) = two_candidate_list(&store).await?;

        let Err(err) = reorder_candidate_list(
            CandidateListReorderPath { list_id: list.id },
            list.clone(),
            store.clone(),
            Json(CandidateListReorderPayload {
                person_ids: vec![person_a, person_a],
            }),
        )
        .await
        else {
            panic!("a repeated candidate must be refused");
        };

        assert!(matches!(err, AppError::DuplicateCandidate));
        assert_eq!(
            store.get_candidate_list(list.id)?.candidates,
            vec![person_a, person_b]
        );

        Ok(())
    }

    #[tokio::test]
    async fn reorder_rejects_a_changed_candidate_set() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let (list, person_a, person_b) = two_candidate_list(&store).await?;
        let outsider = sample_person_with_last_name(PersonId::new(), "Visser");
        outsider.create(&store).await?;

        // an existing person who is not on the list
        let Err(err) = reorder_candidate_list(
            CandidateListReorderPath { list_id: list.id },
            list.clone(),
            store.clone(),
            Json(CandidateListReorderPayload {
                person_ids: vec![person_b, outsider.id],
            }),
        )
        .await
        else {
            panic!("a person not on the list must be refused");
        };
        assert!(matches!(err, AppError::CandidateSetChanged));

        // dropping a candidate
        let Err(err) = reorder_candidate_list(
            CandidateListReorderPath { list_id: list.id },
            list.clone(),
            store.clone(),
            Json(CandidateListReorderPayload {
                person_ids: vec![person_b],
            }),
        )
        .await
        else {
            panic!("dropping a candidate must be refused");
        };
        assert!(matches!(err, AppError::CandidateSetChanged));

        assert_eq!(
            store.get_candidate_list(list.id)?.candidates,
            vec![person_a, person_b]
        );

        Ok(())
    }
}
