use askama::Template;
use axum::response::{IntoResponse, Response};

use crate::{
    AppError, Context, CsbContext, CsbMainStore, HtmlTemplate,
    csb::registered_political_groups::paths::CsbRegisteredPoliticalGroupsPath, filters,
    structs::csb::RegisteredPoliticalGroup,
};

#[derive(Template)]
#[template(path = "csb/registered_political_groups/pages/list.html")]
struct RegisteredPoliticalGroupsTemplate {
    /// In the order the lists are numbered on votes: most votes first.
    groups: Vec<RegisteredPoliticalGroup>,
}

/// The registered political groups of the session's election.
pub async fn list(
    _: CsbRegisteredPoliticalGroupsPath,
    context: CsbContext,
    main_store: CsbMainStore,
) -> Result<Response, AppError> {
    Ok(HtmlTemplate(
        RegisteredPoliticalGroupsTemplate {
            groups: main_store.registered_political_groups(),
        },
        context,
    )
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    use crate::{
        CsbMainAction, CsbUser, structs::csb::sample_registered_political_group,
        test_utils::response_body_string,
    };

    #[tokio::test]
    async fn empty_list_shows_a_placeholder_and_an_add_button() {
        let response = list(
            CsbRegisteredPoliticalGroupsPath,
            CsbContext::new_test(),
            CsbMainStore::new_for_test(),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("No political groups have been registered yet."));
        assert!(body.contains("href=\"/csb/registered-political-groups/add\""));
    }

    #[tokio::test]
    async fn lists_groups_most_votes_first_with_edit_links() {
        let store = CsbMainStore::new_for_test();
        let small = sample_registered_political_group("Kleine Partij", 100, 1);
        let large = sample_registered_political_group("Grote Partij", 5000, 7);
        for group in [&small, &large] {
            store
                .update(
                    CsbMainAction::CreateRegisteredPoliticalGroup(group.clone())
                        .by(CsbUser::new_test()),
                )
                .await
                .unwrap();
        }

        let response = list(
            CsbRegisteredPoliticalGroupsPath,
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        let body = response_body_string(response).await;
        let large_at = body.find("Grote Partij").expect("large group listed");
        let small_at = body.find("Kleine Partij").expect("small group listed");
        assert!(large_at < small_at);
        assert!(body.contains(">5000<"));
        assert!(body.contains(">7<"));
        assert!(body.contains(&format!(
            "href=\"/csb/registered-political-groups/{}\"",
            large.id
        )));
    }
}
