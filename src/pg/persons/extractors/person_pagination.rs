use crate::{
    pagination::Pagination,
    pg::request_extractor,
    structs::persons::{Person, PersonPagination, PersonSort},
};

request_extractor!(PersonPagination, |store, parts, state| {
    let pagination: Pagination<PersonSort> = Pagination::from_request_parts(parts, state).await?;

    let total_items = store.snapshot().person_count();
    let pagination = pagination.set_total(total_items);

    let persons = Person::list(
        &store,
        pagination.limit(),
        pagination.offset(),
        pagination.sort(),
        pagination.direction(),
    )?;

    Ok(PersonPagination {
        persons,
        pagination,
    })
});

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        routing::get,
    };
    use tower::ServiceExt;

    use crate::{
        AppState, PgStore,
        structs::persons::PersonId,
        test_utils::{response_body_string, sample_person_with_last_name},
    };

    #[tokio::test]
    async fn person_pagination_extractor_slices_and_orders() {
        let app_state = AppState::new_for_tests().await;
        let store = PgStore::new_for_test();
        sample_person_with_last_name(PersonId::new(), "Bakker")
            .create(&store)
            .await
            .unwrap();
        sample_person_with_last_name(PersonId::new(), "Jansen")
            .create(&store)
            .await
            .unwrap();

        let app = Router::new()
            .route(
                "/persons",
                get(|pagination: PersonPagination| async move {
                    let last_name = pagination
                        .persons
                        .first()
                        .map(|person| person.data.name.last_name.to_string())
                        .unwrap_or_default();
                    format!("{}:{}", pagination.pagination.page, last_name)
                }),
            )
            .with_state(app_state);

        let mut request = Request::builder()
            .uri("/persons?page=2&per_page=1&sort=last_name&order=asc")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(store.clone());

        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("2:Jansen"));
    }
}
