use crate::structs::persons::Person;
use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, AppResponse, Context, Form, HtmlTemplate, Overlay, PgStore, QueryParamState, filters,
    form::FormData,
    persons::{AddressForm, pages::UpdatePersonAddressPath},
    structs::common::{HasSeverity, Problematic},
};

#[derive(Template)]
#[template(path = "pg/persons/pages/update_address.html")]
struct PersonAddressUpdateTemplate {
    should_warn: bool,
    address_unknown: bool,
    person: Person,
    form: FormData<AddressForm>,
    overlay: Overlay,
}

pub async fn update_person_address(
    _: UpdatePersonAddressPath,
    context: Context,
    person: Person,
    Query(query): Query<QueryParamState>,
) -> AppResponse<impl IntoResponse> {
    Ok(HtmlTemplate(
        PersonAddressUpdateTemplate {
            should_warn: query.should_warn(),
            address_unknown: person.address.is_unknown(),
            form: FormData::new_with_data(AddressForm::from(person.clone())),
            overlay: Overlay::new_edit(&query),
            person,
        },
        context,
    ))
}

pub async fn update_person_address_submit(
    _: UpdatePersonAddressPath,
    context: Context,
    person: Person,
    store: PgStore,
    Query(query): Query<QueryParamState>,
    Form(form): Form<AddressForm>,
) -> Result<Response, AppError> {
    match form.validate_update(&person) {
        Err(form_data) => Ok(HtmlTemplate(
            PersonAddressUpdateTemplate {
                should_warn: query.should_warn(),
                address_unknown: person.address.is_unknown(),
                person,
                form: form_data,
                overlay: Overlay::new_edit(&query),
            },
            context,
        )
        .into_response()),
        Ok(mut person) => {
            person.save_address(&store).await?;

            Ok(query.redirect_or(person.highlight_success_path()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OrNotFound;
    use crate::{
        AppError, Context, Form, PgStore, QueryParamState,
        common::DutchAddressForm,
        structs::persons::PersonId,
        test_utils::{response_body_string, sample_address_form, sample_person},
    };
    use axum::{
        extract::Query,
        http::{StatusCode, header},
        response::IntoResponse,
    };

    #[tokio::test]
    async fn update_person_address_renders_existing_person() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let person_id: PersonId = PersonId::new();
        let person = sample_person(person_id);

        person.create(&store).await?;

        let response = update_person_address(
            UpdatePersonAddressPath { person_id },
            Context::new_test_without_db(),
            person,
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Juinen"));

        Ok(())
    }

    #[tokio::test]
    async fn update_person_address_warns_when_not_in_bag() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let person_id = PersonId::new();
        let mut person = sample_person(person_id);
        // The address was checked against the BAG and not found.
        person.address.known_in_bag = Some(false);

        person.create(&store).await?;

        let response = update_person_address(
            UpdatePersonAddressPath { person_id },
            Context::new_test_without_db(),
            person,
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // When the address is not in the BAG the warning is shown (not hidden).
        assert!(body.contains("Address not found in the BAG"));
        assert!(!body.contains("class=\"warning hidden\""));

        Ok(())
    }

    #[tokio::test]
    async fn update_person_address_no_bag_warning_when_known() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let person_id = PersonId::new();
        // `sample_person` has an address that is known in the BAG.
        let person = sample_person(person_id);

        person.create(&store).await?;

        let response = update_person_address(
            UpdatePersonAddressPath { person_id },
            Context::new_test_without_db(),
            person,
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // The warning is always in the DOM; when the address is known it is hidden.
        assert!(body.contains("class=\"warning hidden\""));

        Ok(())
    }

    #[tokio::test]
    async fn update_person_address_persists_and_redirects() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let person_id = PersonId::new();
        let person = sample_person(person_id);

        person.create(&store).await?;

        let context = Context::new_test_without_db();
        let form = sample_address_form();
        let expected_path = person.highlight_success_path().to_string();

        let response = update_person_address_submit(
            UpdatePersonAddressPath { person_id },
            context,
            person,
            store.clone(),
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

        let updated = store.snapshot().person(person_id).cloned().or_not_found()?;
        assert_eq!(
            updated.address.locality.as_deref().map(ToString::to_string),
            Some("Juinen".to_string())
        );

        Ok(())
    }

    #[tokio::test]
    async fn update_person_address_invalid_form_renders_template() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let person_id = PersonId::new();
        let person = sample_person(person_id);

        person.create(&store).await?;

        let context = Context::new_test_without_db();
        let mut form = sample_address_form();
        form.address.postal_code = "a".to_string();

        let response = update_person_address_submit(
            UpdatePersonAddressPath { person_id },
            context,
            person,
            store,
            Query(QueryParamState::default()),
            Form(form),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("The postal code is not valid"));

        Ok(())
    }

    #[tokio::test]
    async fn update_person_address_dutch_xor_non_dutch() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let person_id = PersonId::new();
        let person = sample_person(person_id);

        person.create(&store).await?;

        let context = Context::new_test_without_db();

        // Update with Dutch address (but all form fields filled)
        update_person_address_submit(
            UpdatePersonAddressPath { person_id },
            context.clone(),
            person.clone(),
            store.clone(),
            Query(QueryParamState::default()),
            Form(AddressForm {
                address: DutchAddressForm {
                    locality: "Juinen".to_string(),
                    postal_code: "1234 AB".to_string(),
                    house_number: "10".to_string(),
                    house_number_addition: "A".to_string(),
                    street_name: "Stationsstraat".to_string(),
                },
            }),
        )
        .await
        .unwrap();

        // The international address should be removed because `lives_in_nl` is true
        let updated = store.snapshot().person(person_id).cloned().or_not_found()?;
        assert_eq!(
            updated.address.locality.as_deref().map(ToString::to_string),
            Some("Juinen".to_string())
        );
        assert_eq!(
            updated.address.postal_code.unwrap(),
            "1234AB".parse().unwrap()
        );
        assert_eq!(
            updated
                .address
                .house_number
                .as_deref()
                .map(ToString::to_string),
            Some("10".to_string())
        );
        assert_eq!(
            updated
                .address
                .house_number_addition
                .as_deref()
                .map(ToString::to_string),
            Some("A".to_string())
        );
        assert_eq!(
            updated
                .address
                .street_name
                .as_deref()
                .map(ToString::to_string),
            Some("Stationsstraat".to_string())
        );

        Ok(())
    }
}
