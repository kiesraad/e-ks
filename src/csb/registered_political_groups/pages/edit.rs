use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, Context, CsbContext, CsbMainAction, CsbMainStore, Form, HtmlTemplate, Overlay,
    QueryParamState,
    csb::registered_political_groups::{
        RegisteredPoliticalGroupForm,
        paths::{
            CsbAddRegisteredPoliticalGroupPath, CsbEditRegisteredPoliticalGroupPath,
            CsbRegisteredPoliticalGroupsPath,
        },
    },
    filters,
    form::{FormData, ValidationError},
    redirect_success,
    structs::csb::RegisteredPoliticalGroup,
};

#[derive(Template)]
#[template(path = "csb/registered_political_groups/pages/edit.html")]
struct EditRegisteredPoliticalGroupTemplate {
    form: FormData<RegisteredPoliticalGroupForm>,
    overlay: Overlay,
    close_action: String,
    /// The group being edited; `None` when adding a new one.
    group: Option<RegisteredPoliticalGroup>,
}

fn render(
    context: CsbContext,
    query: &QueryParamState,
    form: FormData<RegisteredPoliticalGroupForm>,
    group: Option<RegisteredPoliticalGroup>,
) -> Response {
    HtmlTemplate(
        EditRegisteredPoliticalGroupTemplate {
            form,
            overlay: Overlay::new(query),
            close_action: CsbRegisteredPoliticalGroupsPath.to_string(),
            group,
        },
        context,
    )
    .into_response()
}

/// Validates the submitted form into a group, additionally refusing an
/// appellation already registered to another group.
fn validate(
    main_store: &CsbMainStore,
    form: RegisteredPoliticalGroupForm,
    current: Option<&RegisteredPoliticalGroup>,
) -> Result<RegisteredPoliticalGroup, FormData<RegisteredPoliticalGroupForm>> {
    let group = match current {
        Some(current) => form.validate_update(current)?,
        None => form.validate_create()?,
    };
    if main_store.has_registered_appellation(&group.appellation, current.map(|c| c.id)) {
        return Err(FormData::new_with_errors(
            group.into(),
            vec![(
                "appellation".to_string(),
                ValidationError::AppellationAlreadyExists,
            )],
        ));
    }
    Ok(group)
}

/// Render the "add registered political group" dialog.
pub async fn add(
    _: CsbAddRegisteredPoliticalGroupPath,
    context: CsbContext,
    Query(query): Query<QueryParamState>,
) -> Result<Response, AppError> {
    Ok(render(context, &query, FormData::new(), None))
}

pub async fn add_submit(
    _: CsbAddRegisteredPoliticalGroupPath,
    context: CsbContext,
    main_store: CsbMainStore,
    Query(query): Query<QueryParamState>,
    Form(form): Form<RegisteredPoliticalGroupForm>,
) -> Result<Response, AppError> {
    let group = match validate(&main_store, form, None) {
        Ok(group) => group,
        Err(form) => return Ok(render(context, &query, form, None)),
    };
    main_store
        .update(CsbMainAction::CreateRegisteredPoliticalGroup(group).by(context.user()?))
        .await?;
    Ok(redirect_success(CsbRegisteredPoliticalGroupsPath))
}

/// Render the edit dialog for an existing registered political group.
pub async fn edit(
    CsbEditRegisteredPoliticalGroupPath { id }: CsbEditRegisteredPoliticalGroupPath,
    context: CsbContext,
    main_store: CsbMainStore,
    Query(query): Query<QueryParamState>,
) -> Result<Response, AppError> {
    let group = main_store.get_registered_political_group(id)?;
    Ok(render(
        context,
        &query,
        FormData::new_with_data(group.clone().into()),
        Some(group),
    ))
}

pub async fn edit_submit(
    CsbEditRegisteredPoliticalGroupPath { id }: CsbEditRegisteredPoliticalGroupPath,
    context: CsbContext,
    main_store: CsbMainStore,
    Query(query): Query<QueryParamState>,
    Form(form): Form<RegisteredPoliticalGroupForm>,
) -> Result<Response, AppError> {
    let current = main_store.get_registered_political_group(id)?;
    let group = match validate(&main_store, form, Some(&current)) {
        Ok(group) => group,
        Err(form) => return Ok(render(context, &query, form, Some(current))),
    };
    main_store
        .update(CsbMainAction::UpdateRegisteredPoliticalGroup(group).by(context.user()?))
        .await?;
    Ok(redirect_success(CsbRegisteredPoliticalGroupsPath))
}

#[cfg(test)]
pub(super) async fn store_with(groups: &[RegisteredPoliticalGroup]) -> CsbMainStore {
    let store = CsbMainStore::new_for_test();
    for group in groups {
        store
            .update(
                CsbMainAction::CreateRegisteredPoliticalGroup(group.clone())
                    .by(crate::CsbUser::new_test()),
            )
            .await
            .unwrap();
    }
    store
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{StatusCode, header::LOCATION};

    use crate::{
        structs::csb::{RegisteredPoliticalGroupId, sample_registered_political_group},
        test_utils::response_body_string,
    };

    fn form(appellation: &str, votes: &str, seats: &str) -> Form<RegisteredPoliticalGroupForm> {
        Form(RegisteredPoliticalGroupForm {
            appellation: appellation.to_string(),
            previous_votes: votes.to_string(),
            previous_seats: seats.to_string(),
        })
    }

    fn location(response: &Response) -> &str {
        response.headers()[LOCATION].to_str().unwrap()
    }

    #[tokio::test]
    async fn add_renders_an_empty_form() {
        let response = add(
            CsbAddRegisteredPoliticalGroupPath,
            CsbContext::new_test(),
            Query(QueryParamState::default()),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Add political group"));
        assert!(body.contains("name=\"appellation\""));
        assert!(body.contains("name=\"previous_votes\""));
        assert!(body.contains("name=\"previous_seats\""));
        // Nothing to delete yet.
        assert!(!body.contains("/delete"));
    }

    #[tokio::test]
    async fn add_submit_records_the_group_and_redirects_to_the_list() {
        let store = store_with(&[]).await;

        let response = add_submit(
            CsbAddRegisteredPoliticalGroupPath,
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            form("Nieuwe Partij", "4321", "2"),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert!(location(&response).starts_with("/csb/registered-political-groups?"));
        let groups = store.registered_political_groups();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].appellation.to_string(), "Nieuwe Partij");
        assert_eq!(groups[0].previous_votes.value(), 4321);
        assert_eq!(groups[0].previous_seats.value(), 2);
    }

    #[tokio::test]
    async fn add_submit_re_renders_with_errors_for_invalid_input() {
        let store = store_with(&[]).await;

        let response = add_submit(
            CsbAddRegisteredPoliticalGroupPath,
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            form("Partij", "veel", ""),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("The provided value is not valid."));
        assert!(body.contains("This field must not be empty."));
        // The entered values are kept for correction.
        assert!(body.contains("value=\"veel\""));
        assert!(store.registered_political_groups().is_empty());
    }

    #[tokio::test]
    async fn add_submit_refuses_a_duplicate_appellation() {
        let store = store_with(&[sample_registered_political_group("De Partij", 1, 1)]).await;

        let response = add_submit(
            CsbAddRegisteredPoliticalGroupPath,
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            form("de partij", "2", "2"),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("already registered"));
        assert_eq!(store.registered_political_groups().len(), 1);
    }

    #[tokio::test]
    async fn edit_prefills_the_form_and_offers_deletion() {
        let group = sample_registered_political_group("Bestaande Partij", 777, 3);
        let store = store_with(std::slice::from_ref(&group)).await;

        let response = edit(
            CsbEditRegisteredPoliticalGroupPath { id: group.id },
            CsbContext::new_test(),
            store,
            Query(QueryParamState::default()),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Edit political group"));
        assert!(body.contains("value=\"Bestaande Partij\""));
        assert!(body.contains("value=\"777\""));
        assert!(body.contains("value=\"3\""));
        assert!(body.contains(&format!(
            "href=\"/csb/registered-political-groups/{}/delete\"",
            group.id
        )));
    }

    #[tokio::test]
    async fn edit_of_an_unknown_group_is_not_found() {
        let result = edit(
            CsbEditRegisteredPoliticalGroupPath {
                id: RegisteredPoliticalGroupId::new(),
            },
            CsbContext::new_test(),
            store_with(&[]).await,
            Query(QueryParamState::default()),
        )
        .await;

        assert!(matches!(result, Err(AppError::GenericNotFound)));
    }

    #[tokio::test]
    async fn edit_submit_updates_the_group_keeping_its_id() {
        let group = sample_registered_political_group("Oude Naam", 10, 1);
        let store = store_with(std::slice::from_ref(&group)).await;

        let response = edit_submit(
            CsbEditRegisteredPoliticalGroupPath { id: group.id },
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            form("Nieuwe Naam", "20", "0"),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let updated = store.get_registered_political_group(group.id).unwrap();
        assert_eq!(updated.appellation.to_string(), "Nieuwe Naam");
        assert_eq!(updated.previous_votes.value(), 20);
        assert_eq!(updated.previous_seats.value(), 0);
        assert_eq!(store.registered_political_groups().len(), 1);
    }

    #[tokio::test]
    async fn edit_submit_allows_keeping_the_own_appellation_but_not_anothers() {
        let first = sample_registered_political_group("Eerste", 10, 1);
        let second = sample_registered_political_group("Tweede", 5, 1);
        let store = store_with(&[first.clone(), second.clone()]).await;

        let response = edit_submit(
            CsbEditRegisteredPoliticalGroupPath { id: first.id },
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            form("Eerste", "11", "1"),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let response = edit_submit(
            CsbEditRegisteredPoliticalGroupPath { id: first.id },
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            form("Tweede", "11", "1"),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            store
                .get_registered_political_group(first.id)
                .unwrap()
                .appellation
                .to_string(),
            "Eerste"
        );
    }
}
