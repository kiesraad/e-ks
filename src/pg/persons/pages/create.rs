use crate::structs::persons::Person;
use askama::Template;
use axum::response::{IntoResponse, Redirect, Response};

use crate::{
    AppError, Context, Form, HtmlTemplate, Overlay, PgStore, filters,
    form::FormData,
    persons::{PersonalDataForm, pages::PersonsCreatePath},
};

#[derive(Template)]
#[template(path = "pg/persons/pages/create.html")]
struct PersonCreateTemplate {
    form: FormData<PersonalDataForm>,
    overlay: Overlay,
}

pub async fn create_person(
    _: PersonsCreatePath,
    context: Context,
) -> Result<impl IntoResponse, AppError> {
    Ok(HtmlTemplate(
        PersonCreateTemplate {
            form: FormData::new(),
            overlay: Overlay::default(),
        },
        context,
    ))
}

pub async fn create_person_submit(
    _: PersonsCreatePath,
    context: Context,
    store: PgStore,
    Form(form): Form<PersonalDataForm>,
) -> Result<Response, AppError> {
    match form.validate_create_with_checks(&store) {
        Err(form_data) => Ok(HtmlTemplate(
            PersonCreateTemplate {
                form: *form_data,
                overlay: Overlay::default(),
            },
            context,
        )
        .into_response()),
        Ok(person) => {
            let person =
                Person::create_from_personal_data(&store, person.name, person.personal_data)
                    .await?;

            Ok(Redirect::to(&person.after_create_path()).into_response())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{
        AppError, Context, Form, PgStore,
        test_utils::{response_body_string, sample_person_form},
    };
    use axum::{
        http::{StatusCode, header},
        response::IntoResponse,
    };

    #[tokio::test]
    async fn create_person_renders_csrf_field() {
        let context = Context::new_test_without_db();

        let response = create_person(PersonsCreatePath {}, context)
            .await
            .unwrap()
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response_body_string(response).await;
        assert_eq!(
            // One for: logout, language selection and create person
            body.matches(r#"input type="hidden" name="csrf_token""#)
                .count(),
            3,
        );
    }

    #[tokio::test]
    async fn create_person_persists_and_redirects() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let context = Context::new_test_without_db();
        let form = sample_person_form();

        let response =
            create_person_submit(PersonsCreatePath {}, context, store.clone(), Form(form))
                .await
                .unwrap();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get(header::LOCATION)
            .expect("location header")
            .to_str()
            .expect("location header value");
        let persons = store.get_persons();
        assert_eq!(persons.len(), 1);
        let created = persons.first().expect("person");
        assert_eq!(location, created.after_create_path());

        let count = store.get_person_count();
        assert_eq!(count, 1);

        Ok(())
    }

    #[tokio::test]
    async fn create_person_invalid_form_renders_template() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let context = Context::new_test_without_db();
        let mut form = sample_person_form();
        form.name.last_name = " ".to_string();

        let response = create_person_submit(PersonsCreatePath {}, context, store, Form(form))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("This field must not be empty."));

        Ok(())
    }

    #[tokio::test]
    async fn create_person_duplicate_name_renders_error() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let existing = crate::test_utils::sample_person(crate::structs::persons::PersonId::new());
        existing.create(&store).await?;

        let context = Context::new_test_without_db();
        let form = sample_person_form();

        let response = create_person_submit(PersonsCreatePath {}, context, store, Form(form))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("A person with this name already exists."));

        Ok(())
    }
}
