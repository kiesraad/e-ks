use std::collections::HashMap;

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
    let mut counts = HashMap::new();

    given
        .iter()
        .map(ToString::to_string)
        .for_each(|s_id| *counts.entry(s_id).or_insert(0) += 1);
    expected
        .iter()
        .map(ToString::to_string)
        .for_each(|s_id| *counts.entry(s_id).or_insert(0) -= 1);

    counts.values().all(|&v| v == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::csb::examination::extractors::CsbPoliticalGroup;

    #[tokio::test]
    async fn update_order_records_the_posted_order() -> Result<(), AppError> {
        let main_store = CsbMainStore::new_for_test();
        let first = CsbPoliticalGroup::sample("Eerste");
        let second = CsbPoliticalGroup::sample("Tweede");
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
        let known = CsbPoliticalGroup::sample("Bekend");

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
                CsbPoliticalGroups(vec![CsbPoliticalGroup::sample("Bekend")]),
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
