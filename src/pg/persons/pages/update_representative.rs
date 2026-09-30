use crate::structs::persons::Person;
use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, AppResponse, Context, Form, HtmlTemplate, Overlay, PgStore, QueryParamState, filters,
    form::FormData,
    persons::{RepresentativeForm, pages::UpdateRepresentativePath},
    structs::common::{HasSeverity, Problematic},
};

#[derive(Template)]
#[template(path = "pg/persons/pages/update_representative.html")]
struct RepresentativeUpdateTemplate {
    should_warn: bool,
    address_unknown: bool,
    person: Person,
    form: FormData<RepresentativeForm>,
    overlay: Overlay,
}

pub async fn update_representative(
    _: UpdateRepresentativePath,
    context: Context,
    person: Person,
    Query(query): Query<QueryParamState>,
) -> AppResponse<impl IntoResponse> {
    Ok(HtmlTemplate(
        RepresentativeUpdateTemplate {
            should_warn: query.should_warn(),
            address_unknown: person.representative_address_unknown(),
            form: FormData::new_with_data(RepresentativeForm::from(
                person.clone().representative.unwrap_or_default(),
            )),
            overlay: Overlay::new_edit(&query),
            person,
        },
        context,
    ))
}

pub async fn update_representative_submit(
    _: UpdateRepresentativePath,
    context: Context,
    person: Person,
    store: PgStore,
    Query(query): Query<QueryParamState>,
    Form(form): Form<RepresentativeForm>,
) -> Result<Response, AppError> {
    let representative = person.clone().representative.unwrap_or_default();
    match form.validate_update(&representative) {
        Err(form_data) => Ok(HtmlTemplate(
            RepresentativeUpdateTemplate {
                should_warn: query.should_warn(),
                address_unknown: person.representative_address_unknown(),
                person,
                form: form_data,
                overlay: Overlay::new_edit(&query),
            },
            context,
        )
        .into_response()),
        Ok(representative) => {
            person.save_representative(&store, representative).await?;

            Ok(query.redirect_or(person.highlight_success_path()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppError, Context, Form, PgStore, QueryParamState,
        structs::persons::PersonId,
        test_utils::{
            extract_csrf_token, response_body_string, sample_person, sample_representative_form,
        },
    };
    use axum::{
        extract::Query,
        http::{StatusCode, header},
        response::IntoResponse,
    };

    #[tokio::test]
    async fn update_representative_renders_existing_person() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let person_id = PersonId::new();
        let person = sample_person(person_id);

        person.create(&store).await?;

        let response = update_representative(
            UpdateRepresentativePath { person_id },
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
    async fn update_representative_renders_valid_csrf_token() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let person_id = PersonId::new();
        let person = sample_person(person_id);

        person.create(&store).await?;

        let context = Context::new_test_without_db();
        let expected_csrf = context.session.csrf_token().clone();

        let response = update_representative(
            UpdateRepresentativePath { person_id },
            context,
            person,
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        let csrf_token = extract_csrf_token(&body).expect("csrf token");
        assert_eq!(csrf_token, expected_csrf);

        Ok(())
    }

    #[tokio::test]
    async fn update_representative_persists_and_redirects() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let person_id = PersonId::new();
        let person = sample_person(person_id);

        person.create(&store).await?;

        let context = Context::new_test_without_db();
        let mut form = sample_representative_form();
        form.name.last_name = "Smit".to_string();
        let expected_path = person.highlight_success_path().to_string();

        let response = update_representative_submit(
            UpdateRepresentativePath { person_id },
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

        let updated = store.get_person(person_id)?;
        assert_eq!(
            updated.representative.unwrap().name.last_name.to_string(),
            "Smit"
        );

        Ok(())
    }

    #[tokio::test]
    async fn update_representative_invalid_form_renders_template() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let person_id = PersonId::new();
        let person = sample_person(person_id);

        person.create(&store).await?;

        let context = Context::new_test_without_db();
        let mut form = sample_representative_form();
        form.address.postal_code = "a".to_string();

        let response = update_representative_submit(
            UpdateRepresentativePath { person_id },
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
}
