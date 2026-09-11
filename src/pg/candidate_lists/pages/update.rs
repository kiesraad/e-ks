use crate::structs::candidate_lists::CandidateList;
use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, Context, ElectoralDistrict, Form, HtmlTemplate, Overlay, PgStore, QueryParamState,
    candidate_lists::{CandidateListForm, pages::CandidateListUpdatePath},
    filters,
    form::FormData,
};

#[derive(Template)]
#[template(path = "pg/candidate_lists/pages/update.html")]
struct CandidateListUpdateTemplate {
    should_warn: bool,
    form: FormData<CandidateListForm>,
    candidate_list: CandidateList,
    available_districts: Vec<ElectoralDistrict>,
    duplicate_districts: Vec<ElectoralDistrict>,
    overlay: Overlay,
}

pub async fn update_candidate_list(
    _: CandidateListUpdatePath,
    context: Context,
    candidate_list: CandidateList,
    store: PgStore,
    Query(query): Query<QueryParamState>,
) -> Result<Response, AppError> {
    let available_districts = CandidateList::available_districts(&store, &context.election);
    let duplicate_districts = candidate_list.duplicate_districts(&store);
    Ok(HtmlTemplate(
        CandidateListUpdateTemplate {
            form: FormData::new_with_data(CandidateListForm::from(candidate_list.clone())),
            should_warn: query.should_warn(),
            overlay: Overlay::new(&query),
            candidate_list,
            available_districts,
            duplicate_districts,
        },
        context,
    )
    .into_response())
}

pub async fn update_candidate_list_submit(
    _: CandidateListUpdatePath,
    context: Context,
    candidate_list: CandidateList,
    store: PgStore,
    Query(query): Query<QueryParamState>,
    Form(mut form): Form<CandidateListForm>,
) -> Result<Response, AppError> {
    if context.election.has_only_one_district() {
        return Err(AppError::UserError(
            "Not available for single district elections".to_string(),
        ));
    }
    let available_districts = CandidateList::available_districts(&store, &context.election);
    let duplicate_districts = candidate_list.duplicate_districts(&store);
    form.electoral_districts = context.election.known_districts(&form.electoral_districts);
    match form.validate_update(&candidate_list) {
        Err(form_data) => Ok(HtmlTemplate(
            CandidateListUpdateTemplate {
                should_warn: query.should_warn(),
                form: form_data,
                overlay: Overlay::new(&query),
                candidate_list,
                available_districts,
                duplicate_districts,
            },
            context,
        )
        .into_response()),
        Ok(candidate_list) => {
            candidate_list.update_districts(&store).await?;

            Ok(query.redirect_or(candidate_list.view_path()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Context, ElectionConfig, ElectoralDistrict, Form, PgStore, QueryParamState,
        structs::candidate_lists::{CandidateListId, CandidateListSummary},
        test_utils::{response_body_string, sample_candidate_list},
    };
    use axum::{
        extract::Query,
        http::{StatusCode, header},
    };
    use axum_extra::routing::TypedPath;

    #[tokio::test]
    async fn update_candidate_list_renders_existing_list() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let candidate_list = sample_candidate_list(CandidateListId::new());

        candidate_list.create(&store).await?;

        let response = update_candidate_list(
            CandidateListUpdatePath {
                list_id: candidate_list.id,
            },
            Context::new_test_without_db(),
            candidate_list.clone(),
            store,
            Query(QueryParamState::default()),
        )
        .await?;

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Edit candidate list"));
        assert!(body.contains(&candidate_list.update_path().to_string()));
        assert!(body.contains("electoral_district_prov7"));
        assert!(body.contains("checked"));

        Ok(())
    }

    #[tokio::test]
    async fn update_candidate_list_persists_and_redirects() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let context = Context::new_test_without_db();
        let candidate_list = CandidateList {
            electoral_districts: vec![ElectoralDistrict::Utrecht],
            ..Default::default()
        };
        candidate_list.create(&store).await?;

        let form = CandidateListForm {
            electoral_districts: vec![ElectoralDistrict::Drenthe],
        };
        let response = update_candidate_list_submit(
            CandidateListUpdatePath {
                list_id: candidate_list.id,
            },
            context,
            candidate_list.clone(),
            store.clone(),
            Query(QueryParamState::default()),
            Form(form),
        )
        .await?;

        // verify redirect
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get(header::LOCATION)
            .expect("location header")
            .to_str()
            .expect("location header value");

        // verify updated candidate list object in database
        let lists = CandidateListSummary::list(&store);
        assert_eq!(lists.len(), 1);

        let updated_list = &lists[0].list;

        assert_eq!(
            updated_list
                .view_path()
                .with_query_params(QueryParamState::success())
                .to_string(),
            location
        );

        assert_eq!(candidate_list.id, updated_list.id);
        assert_eq!(
            vec![ElectoralDistrict::Drenthe],
            updated_list.electoral_districts
        );

        Ok(())
    }

    #[tokio::test]
    async fn update_candidate_list_invalid_form_renders_template() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let candidate_list = CandidateList {
            electoral_districts: vec![ElectoralDistrict::Utrecht],
            ..Default::default()
        };
        candidate_list.create(&store).await?;

        let form = CandidateListForm {
            electoral_districts: vec![],
        };
        let response = update_candidate_list_submit(
            CandidateListUpdatePath {
                list_id: candidate_list.id,
            },
            Context::new_test_without_db(),
            candidate_list.clone(),
            store.clone(),
            Query(QueryParamState::default()),
            Form(form),
        )
        .await?;

        assert_eq!(StatusCode::OK, response.status());
        let body = response_body_string(response).await;
        assert!(body.contains("Edit candidate list"));

        let lists = CandidateListSummary::list(&store);
        assert_eq!(lists.len(), 1);

        let updated_list = &lists[0].list;

        // verify candidate list didn't update in database
        assert_eq!(
            candidate_list.electoral_districts,
            updated_list.electoral_districts
        );

        Ok(())
    }

    #[tokio::test]
    async fn update_candidate_list_stores_a_repeated_district_once() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let context = Context::new_test_without_db();
        let candidate_list = CandidateList {
            electoral_districts: vec![ElectoralDistrict::Utrecht],
            ..Default::default()
        };
        candidate_list.create(&store).await?;

        let form = CandidateListForm {
            electoral_districts: vec![
                ElectoralDistrict::Utrecht,
                ElectoralDistrict::Drenthe,
                ElectoralDistrict::Utrecht,
            ],
        };
        update_candidate_list_submit(
            CandidateListUpdatePath {
                list_id: candidate_list.id,
            },
            context,
            candidate_list.clone(),
            store.clone(),
            Query(QueryParamState::default()),
            Form(form),
        )
        .await?;

        // deduplicated, in the election's district order
        assert_eq!(
            store
                .get_candidate_list(candidate_list.id)?
                .electoral_districts,
            vec![ElectoralDistrict::Drenthe, ElectoralDistrict::Utrecht]
        );

        Ok(())
    }

    #[tokio::test]
    async fn district_outside_election_is_ignored() -> Result<(), AppError> {
        // setup
        let store = PgStore::new_for_test_with_election(ElectionConfig::EK27);
        let context = Context::new_test_without_db();
        let candidate_list = CandidateList {
            electoral_districts: vec![ElectoralDistrict::Utrecht],
            ..Default::default()
        };
        candidate_list.create(&store).await?;

        let form = CandidateListForm {
            electoral_districts: vec![ElectoralDistrict::Drenthe, ElectoralDistrict::WsFryslan],
        };

        // test
        let response = update_candidate_list_submit(
            CandidateListUpdatePath {
                list_id: candidate_list.id,
            },
            context,
            candidate_list.clone(),
            store.clone(),
            Query(QueryParamState::default()),
            Form(form),
        )
        .await?;

        // verify
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let lists = store.get_candidate_lists();
        assert_eq!(lists.len(), 1);
        let list = &lists[0];
        // WsFryslan got dropped because it's not part of EK27
        assert_eq!(list.electoral_districts, vec![ElectoralDistrict::Drenthe]);

        Ok(())
    }
}
