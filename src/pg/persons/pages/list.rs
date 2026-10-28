use crate::structs::persons::{PersonPagination, PersonSort};
use askama::Template;
use axum::response::IntoResponse;

use crate::{
    AppError, Context, HtmlTemplate, filters, persons::pages::PersonsPath,
    structs::common::HasSeverity,
};

#[derive(Template)]
#[template(path = "pg/persons/pages/list.html")]
struct PersonListTemplate {
    person_pagination: PersonPagination,
}

pub async fn list_persons(
    _: PersonsPath,
    context: Context,
    person_pagination: PersonPagination,
) -> Result<impl IntoResponse, AppError> {
    Ok(HtmlTemplate(
        PersonListTemplate { person_pagination },
        context,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppError, Context, PgStore,
        pagination::Pagination,
        structs::{
            common::Problems,
            persons::{PersonId, PersonWithProblems},
        },
        test_utils::{response_body_string, sample_person},
    };
    use axum::{http::StatusCode, response::IntoResponse};

    #[tokio::test]
    async fn list_persons_shows_created_person() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let id = PersonId::new();
        let person = sample_person(id);

        person.create(&store).await?;

        let response = list_persons(
            PersonsPath {},
            Context::new_test_without_db(),
            PersonPagination {
                persons: vec![PersonWithProblems {
                    data: person,
                    problems: Problems::new_empty(),
                }],
                pagination: Pagination::default().set_total(1),
            },
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Jansen"));

        Ok(())
    }
}
