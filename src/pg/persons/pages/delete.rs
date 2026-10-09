use crate::structs::persons::Person;
use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, AppResponse, Context, HtmlTemplate, Overlay, PgStore, QueryParamState, filters,
    persons::pages::DeletePersonPath,
    structs::common::{HasSeverity, Problematic},
};

#[derive(Template)]
#[template(path = "pg/persons/pages/delete.html")]
struct DeletePersonTemplate {
    person: Person,
    overlay: Overlay,
}

pub async fn delete_person_confirm(
    _: DeletePersonPath,
    context: Context,
    person: Person,
    Query(query): Query<QueryParamState>,
) -> AppResponse<impl IntoResponse> {
    Ok(HtmlTemplate(
        DeletePersonTemplate {
            person,
            overlay: Overlay::new_edit(&query),
        },
        context,
    ))
}

pub async fn delete_person(
    _: DeletePersonPath,
    _context: Context,
    person: Person,
    store: PgStore,
    Query(query): Query<QueryParamState>,
) -> Result<Response, AppError> {
    person.delete(&store).await?;

    Ok(query.redirect_or(Person::list_path()))
}

#[cfg(test)]
mod tests {
    use crate::OrNotFound;
    use axum::extract::Query;
    use axum_extra::routing::TypedPath;

    use super::*;
    use crate::{
        AppError, Context, PgStore, QueryParamState,
        structs::persons::PersonId,
        test_utils::{response_body_string, sample_person},
    };

    #[tokio::test]
    async fn delete_person_confirm_contains_delete_button() -> Result<(), AppError> {
        let person_id = PersonId::new();
        let person = sample_person(person_id);

        let response = delete_person_confirm(
            DeletePersonPath { person_id },
            Context::new_test_without_db(),
            person.clone(),
            Query(QueryParamState::default()),
        )
        .await?
        .into_response();

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains(&person.delete_path().to_string()));

        Ok(())
    }

    #[tokio::test]
    async fn delete_person_removes_and_redirects() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let person_id = PersonId::new();
        let person = sample_person(person_id);

        person.create(&store).await?;

        let context = Context::new_test_without_db();

        let response = delete_person(
            DeletePersonPath { person_id },
            context,
            person,
            store.clone(),
            Query(QueryParamState::default()),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get(axum::http::header::LOCATION)
            .expect("location header")
            .to_str()
            .expect("location header value");
        assert_eq!(
            location,
            Person::list_path()
                .with_query_params(QueryParamState::success())
                .to_string()
        );

        let found = store.snapshot().person(person_id).cloned().or_not_found();
        assert!(found.is_err());

        Ok(())
    }
}
