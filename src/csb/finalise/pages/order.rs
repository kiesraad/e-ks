use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Deserialize;

use crate::{
    AppError, CsbContext, CsbMainAction, CsbMainStore, StreamId,
    csb::{
        examination::{extractors::CsbPoliticalGroups, numbering::ListNumbering},
        finalise::paths::CsbListOrderPath,
    },
};

#[derive(Deserialize)]
pub struct ListOrderPayload {
    /// The streams of the groups numbered by lot, in the order drawn.
    pub stream_ids: Vec<StreamId>,
}

/// Records the list order posted by the sortable table. The order has to name
/// exactly the groups currently numbered; a stale page is refused.
pub async fn update_order(
    _: CsbListOrderPath,
    context: CsbContext,
    main_store: CsbMainStore,
    CsbPoliticalGroups(political_groups): CsbPoliticalGroups,
    Json(payload): Json<ListOrderPayload>,
) -> Result<impl IntoResponse, AppError> {
    let numbering = ListNumbering::new(
        &political_groups,
        &main_store.registered_political_groups(),
        &main_store.list_order(),
    );
    if !is_permutation(&payload.stream_ids, &numbering.stream_ids()) {
        return Err(AppError::UserError(
            "The order does not match the numbered lists".to_string(),
        ));
    }

    main_store
        .update(CsbMainAction::UpdateListOrder(payload.stream_ids).by(context.user()?))
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Whether `given` names every stream in `expected` exactly once.
fn is_permutation(given: &[StreamId], expected: &[StreamId]) -> bool {
    let mut given: Vec<_> = given.iter().map(ToString::to_string).collect();
    let mut expected: Vec<_> = expected.iter().map(ToString::to_string).collect();
    given.sort();
    expected.sort();
    given == expected
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::{
        ElectoralDistrict, csb::examination::extractors::CsbPoliticalGroup,
        structs::candidate_lists::CandidateListId, test_utils::sample_political_group,
    };

    fn group(appellation: &str) -> CsbPoliticalGroup {
        CsbPoliticalGroup {
            political_group: crate::structs::political_groups::PoliticalGroup {
                appellation: Some(appellation.parse().unwrap()),
                ..sample_political_group()
            },
            stream_id: StreamId::new(),
            brp: crate::csb::examination::structs::BrpCheckState::NotChecked,
            mode: crate::structs::csb::CsbPhase::Examination,
            is_examination_finished: true,
            is_deleted: false,
            scrapped: Default::default(),
            restoration_count: 0,
            omission_count: 0,
            recovery: Default::default(),
            first_candidate_name: None,
            candidate_list_districts: HashMap::from([(
                CandidateListId::new(),
                vec![ElectoralDistrict::Groningen],
            )]),
        }
    }

    #[tokio::test]
    async fn update_order_records_the_posted_order() -> Result<(), AppError> {
        let main_store = CsbMainStore::new_for_test();
        let first = group("Eerste");
        let second = group("Tweede");
        let order = vec![second.stream_id, first.stream_id];

        let response = update_order(
            CsbListOrderPath,
            CsbContext::new_test(),
            main_store.clone(),
            CsbPoliticalGroups(vec![first, second]),
            Json(ListOrderPayload {
                stream_ids: order.clone(),
            }),
        )
        .await?
        .into_response();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(main_store.list_order(), order);
        Ok(())
    }

    /// An order naming other groups than the numbered ones is a stale page
    /// and is refused without recording anything.
    #[tokio::test]
    async fn update_order_refuses_an_order_over_other_groups() {
        let main_store = CsbMainStore::new_for_test();
        let known = group("Bekend");

        for stream_ids in [
            vec![],
            vec![StreamId::new()],
            vec![known.stream_id, StreamId::new()],
            vec![known.stream_id, known.stream_id],
        ] {
            let result = update_order(
                CsbListOrderPath,
                CsbContext::new_test(),
                main_store.clone(),
                CsbPoliticalGroups(vec![group("Bekend")]),
                Json(ListOrderPayload { stream_ids }),
            )
            .await;

            assert!(matches!(result, Err(AppError::UserError(_))));
        }
        assert!(main_store.list_order().is_empty());
    }

    #[test]
    fn permutation_ignores_order_but_not_duplicates_or_extras() {
        let a = StreamId::new();
        let b = StreamId::new();

        assert!(is_permutation(&[b, a], &[a, b]));
        assert!(is_permutation(&[], &[]));
        assert!(!is_permutation(&[a, a], &[a, b]));
        assert!(!is_permutation(&[a], &[a, b]));
        assert!(!is_permutation(&[a, b, StreamId::new()], &[a, b]));
    }
}
