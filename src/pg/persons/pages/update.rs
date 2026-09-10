use crate::structs::persons::Person;
use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, AppResponse, Context, Form, HtmlTemplate, Overlay, PgStore, QueryParamState, filters,
    form::FormData,
    persons::{PersonalDataForm, pages::UpdatePersonPath},
    structs::common::{HasSeverity, Problematic},
};

#[derive(Template)]
#[template(path = "pg/persons/pages/update.html")]
struct PersonUpdateTemplate {
    person: Person,
    form: FormData<PersonalDataForm>,
    overlay: Overlay,
    locality_unknown: bool,
}

pub async fn update_person(
    _: UpdatePersonPath,
    context: Context,
    person: Person,
    Query(query): Query<QueryParamState>,
) -> AppResponse<impl IntoResponse> {
    Ok(HtmlTemplate(
        PersonUpdateTemplate {
            form: FormData::new_with_data(PersonalDataForm::from(person.clone())),
            overlay: Overlay::new(&query),
            locality_unknown: person
                .personal_data
                .show_unknown_place_of_residence_warning(),
            person,
        },
        context,
    ))
}

pub async fn update_person_submit(
    _: UpdatePersonPath,
    context: Context,
    store: PgStore,
    person: Person,
    Query(query): Query<QueryParamState>,
    Form(form): Form<PersonalDataForm>,
) -> Result<Response, AppError> {
    match form.validate_update_with_checks(&person, &store) {
        Err(form_data) => Ok(HtmlTemplate(
            PersonUpdateTemplate {
                locality_unknown: person
                    .personal_data
                    .show_unknown_place_of_residence_warning(),
                person,
                form: *form_data,
                overlay: Overlay::new(&query),
            },
            context,
        )
        .into_response()),
        Ok(updated) => {
            let updated = person
                .update_personal_data(&store, updated.name, updated.personal_data)
                .await?;

            Ok(query.redirect_or(updated.after_update_path()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppError, Context, Form, PgStore, QueryParamState,
        structs::{common::DateOfBirth, persons::PersonId},
        test_utils::{response_body_string, sample_person, sample_person_form},
    };
    use axum::{
        extract::Query,
        http::{StatusCode, header},
        response::IntoResponse,
    };
    use chrono::Duration;

    #[tokio::test]
    async fn update_person_renders_existing_person() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let person_id = PersonId::new();
        let person = sample_person(person_id);

        person.create(&store).await?;

        let response = update_person(
            UpdatePersonPath { person_id },
            Context::new_test_without_db(),
            person,
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Jansen"));

        Ok(())
    }

    #[tokio::test]
    async fn update_person_persists_and_redirects() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let person_id = PersonId::new();
        let person = sample_person(person_id);

        person.create(&store).await?;

        let context = Context::new_test_without_db();
        let mut form = sample_person_form();
        form.name.last_name = "Updated".to_string();
        let expected_path = format!("{}?&success=true", person.after_update_path());

        let response = update_person_submit(
            UpdatePersonPath { person_id },
            context,
            store.clone(),
            person,
            Query(QueryParamState::default()),
            Form(form),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get(header::LOCATION)
            .expect("location header")
            .to_str()
            .expect("location header value");
        assert_eq!(location, expected_path);

        let updated = store.get_person(person_id)?;
        assert_eq!(updated.name.last_name.to_string(), "Updated");

        Ok(())
    }

    #[tokio::test]
    async fn update_person_invalid_form_renders_template() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let person_id = PersonId::new();
        let person = sample_person(person_id);

        person.create(&store).await?;

        let context = Context::new_test_without_db();
        let mut form = sample_person_form();
        form.name.last_name = " ".to_string();

        let response = update_person_submit(
            UpdatePersonPath { person_id },
            context,
            store,
            person,
            Query(QueryParamState::default()),
            Form(form),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("This field must not be empty."));

        Ok(())
    }

    #[tokio::test]
    async fn update_person_too_young_renders_error() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let person_id = PersonId::new();
        let person = sample_person(person_id);

        person.create(&store).await?;

        let context = Context::new_test_without_db();
        let mut form = sample_person_form();
        form.personal_data.date_of_birth =
            DateOfBirth::from(context.election.eligible_date_of_birth() + Duration::days(1))
                .format(crate::core::constants::DEFAULT_DATE_FORMAT)
                .to_string();

        let response = update_person_submit(
            UpdatePersonPath { person_id },
            context,
            store,
            person,
            Query(QueryParamState::default()),
            Form(form),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Candidate too young to participate in election"));

        Ok(())
    }

    #[tokio::test]
    async fn unknown_bag_locality_renders_warning() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let person_id = PersonId::new();
        let person = sample_person(person_id);

        person.create(&store).await?;

        let context = Context::new_test_without_db();
        let mut form = sample_person_form();
        form.personal_data.place_of_residence = "Fantasy City".to_string();

        let response = update_person_submit(
            UpdatePersonPath { person_id },
            context,
            store,
            person,
            Query(QueryParamState::default()),
            Form(form),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Place of residence not found in the BAG"));

        Ok(())
    }
}
