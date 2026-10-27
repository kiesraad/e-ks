use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Redirect, Response},
};

use crate::{
    AppError, Context, CsbAction, CsbContext, CsbMainStore, CsbStore, HtmlTemplate, Overlay,
    QueryParamState,
    csb::examination::{
        CsbExaminationOverviewPath, extractors::CsbPoliticalGroup,
        paths::CsbPoliticalGroupDeletePath,
    },
    filters,
};

#[derive(Template)]
#[template(path = "csb/examination/pages/delete.html")]
struct CsbPoliticalGroupDeleteTemplate {
    political_group: CsbPoliticalGroup,
    overlay: Overlay,
    close_action: String,
}

pub async fn delete(
    _: CsbPoliticalGroupDeletePath,
    context: CsbContext,
    Query(query): Query<QueryParamState>,
    store: CsbStore,
) -> Result<Response, AppError> {
    let political_group = CsbPoliticalGroup::new_from_csb_store(&store);
    let close_action = political_group.group_path().to_string();
    Ok(HtmlTemplate(
        CsbPoliticalGroupDeleteTemplate {
            close_action,
            political_group,
            overlay: Overlay::new_edit(&query),
        },
        context,
    )
    .into_response())
}

pub async fn delete_submit(
    _: CsbPoliticalGroupDeletePath,
    store: CsbStore,
    main_store: CsbMainStore,
) -> Result<Response, AppError> {
    store.update(CsbAction::Delete).await?;
    main_store
        .remove_from_list_order(store.user().clone(), store.stream_id)
        .await?;
    Ok(Redirect::to(&CsbExaminationOverviewPath.to_string()).into_response())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use reqwest::StatusCode;

    use crate::{
        CsbMainAction, CsbUser, StreamId, projection::WithCorrections,
        structs::common::Appellation, test_utils::response_body_string,
    };

    use super::*;

    #[tokio::test]
    async fn delete_page_includes_appellation_and_delete_button() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();
        let context = CsbContext::new_test();

        let data = store.snapshot();
        let mut pg = data.pg(WithCorrections::All).get_political_group().clone();
        pg.appellation = Appellation::from_str("Test Partij").ok();
        store.set_political_group(pg);

        let response = delete(
            CsbPoliticalGroupDeletePath {
                stream_id: store.stream_id,
            },
            context,
            Query::default(),
            store,
        )
        .await?;

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Delete Test Partij"));
        assert!(body.contains("<button type=\"submit\" class=\"button tertiary-destructive icon-trash\">Delete</button>"));

        Ok(())
    }

    /// Deleting a group drops it from the recorded list order, keeping the
    /// other groups in their order; a group not in the order leaves the
    /// order untouched.
    #[tokio::test]
    async fn delete_submit_removes_the_group_from_the_list_order() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();
        let main_store = CsbMainStore::new_for_test();
        let other = StreamId::new();
        main_store
            .update(
                CsbMainAction::UpdateListOrder(vec![other, store.stream_id])
                    .by(CsbUser::new_test()),
            )
            .await?;

        let response = delete_submit(
            CsbPoliticalGroupDeletePath {
                stream_id: store.stream_id,
            },
            store.clone(),
            main_store.clone(),
        )
        .await?;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert!(store.snapshot().is_deleted());
        assert_eq!(main_store.list_order(), vec![other]);
        assert_eq!(main_store.list_order_events(), 2);

        let untouched = CsbStore::new_for_test();
        delete_submit(
            CsbPoliticalGroupDeletePath {
                stream_id: untouched.stream_id,
            },
            untouched,
            main_store.clone(),
        )
        .await?;

        assert_eq!(main_store.list_order(), vec![other]);
        assert_eq!(main_store.list_order_events(), 2);

        Ok(())
    }
}
