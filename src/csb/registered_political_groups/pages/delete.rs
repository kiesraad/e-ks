use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, Context, CsbContext, CsbMainAction, CsbMainStore, HtmlTemplate, Overlay,
    QueryParamState,
    csb::registered_political_groups::paths::{
        CsbDeleteRegisteredPoliticalGroupPath, CsbRegisteredPoliticalGroupsPath,
    },
    filters, redirect_success,
    structs::csb::RegisteredPoliticalGroup,
};

#[derive(Template)]
#[template(path = "csb/registered_political_groups/pages/delete.html")]
struct DeleteRegisteredPoliticalGroupTemplate {
    group: RegisteredPoliticalGroup,
    overlay: Overlay,
    close_action: String,
}

/// Render the delete confirmation dialog.
pub async fn delete(
    CsbDeleteRegisteredPoliticalGroupPath { id }: CsbDeleteRegisteredPoliticalGroupPath,
    context: CsbContext,
    main_store: CsbMainStore,
    Query(query): Query<QueryParamState>,
) -> Result<Response, AppError> {
    let group = main_store.get_registered_political_group(id)?;
    Ok(HtmlTemplate(
        DeleteRegisteredPoliticalGroupTemplate {
            group,
            overlay: Overlay::new(&query),
            close_action: CsbRegisteredPoliticalGroupsPath.to_string(),
        },
        context,
    )
    .into_response())
}

pub async fn delete_submit(
    CsbDeleteRegisteredPoliticalGroupPath { id }: CsbDeleteRegisteredPoliticalGroupPath,
    context: CsbContext,
    main_store: CsbMainStore,
) -> Result<Response, AppError> {
    // Deleting a group that is already gone is a stale form, not a change.
    main_store.get_registered_political_group(id)?;
    main_store
        .update(CsbMainAction::DeleteRegisteredPoliticalGroup(id).by(context.user()?))
        .await?;
    Ok(redirect_success(CsbRegisteredPoliticalGroupsPath))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    use crate::{
        csb::registered_political_groups::pages::edit::store_with,
        structs::csb::{RegisteredPoliticalGroupId, sample_registered_political_group},
        test_utils::response_body_string,
    };

    #[tokio::test]
    async fn delete_asks_for_confirmation() {
        let group = sample_registered_political_group("Weg Partij", 1, 1);
        let store = store_with(std::slice::from_ref(&group)).await;

        let response = delete(
            CsbDeleteRegisteredPoliticalGroupPath { id: group.id },
            CsbContext::new_test(),
            store,
            Query(QueryParamState::default()),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Delete Weg Partij"));
        assert!(body.contains("Are you sure you want to delete the political group “Weg Partij“?"));
    }

    #[tokio::test]
    async fn delete_submit_removes_the_group_and_redirects_to_the_list() {
        let group = sample_registered_political_group("Weg Partij", 1, 1);
        let kept = sample_registered_political_group("Blijft", 2, 1);
        let store = store_with(&[group.clone(), kept]).await;

        let response = delete_submit(
            CsbDeleteRegisteredPoliticalGroupPath { id: group.id },
            CsbContext::new_test(),
            store.clone(),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let groups = store.registered_political_groups();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].appellation.to_string(), "Blijft");
    }

    #[tokio::test]
    async fn deleting_an_unknown_group_is_not_found() {
        let store = store_with(&[]).await;

        let result = delete_submit(
            CsbDeleteRegisteredPoliticalGroupPath {
                id: RegisteredPoliticalGroupId::new(),
            },
            CsbContext::new_test(),
            store.clone(),
        )
        .await;

        assert!(matches!(result, Err(AppError::GenericNotFound)));
        // No event was recorded for the stale request.
        assert!(store.data.read().events.is_empty());
    }
}
