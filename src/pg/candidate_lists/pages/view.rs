use crate::structs::candidate_lists::{CandidateList, FullCandidateList};
use askama::Template;
use axum::{extract::Query, response::IntoResponse};

use crate::{
    AppError, Context, ElectoralDistrict, HtmlTemplate, PgStore, QueryParamState,
    candidate_lists::pages::ViewCandidateListPath, filters, structs::common::HasSeverity,
};

#[derive(Template)]
#[template(path = "pg/candidate_lists/pages/view.html")]
struct CandidateListViewTemplate {
    full_list: FullCandidateList,
    duplicate_districts: Vec<ElectoralDistrict>,
    max_candidates_reached: bool,
    import_capped: bool,
    ignored_columns: String,
}

pub async fn view_candidate_list(
    _: ViewCandidateListPath,
    context: Context,
    full_list: FullCandidateList,
    store: PgStore,
    Query(query): Query<QueryParamState>,
) -> Result<impl IntoResponse, AppError> {
    let duplicate_districts = full_list.list.duplicate_districts(&store);

    Ok(HtmlTemplate(
        CandidateListViewTemplate {
            full_list,
            duplicate_districts,
            max_candidates_reached: query.is_max_candidates_reached(),
            import_capped: query.is_import_capped(),
            ignored_columns: query.ignored_columns().join(", "),
        },
        context,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Context, PgStore,
        structs::{candidate_lists::CandidateListId, persons::PersonId},
        test_utils::{
            paper_corrections_store, response_body_string, sample_candidate_list, sample_person,
            sample_person_with_last_name,
        },
    };
    use axum::response::IntoResponse;

    #[tokio::test]
    async fn view_candidate_list_renders_candidates() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        let person = sample_person(PersonId::new());

        list.create(&store).await?;
        person.create(&store).await?;
        list.clone().update_order(&store, &[person.id]).await?;

        let full_list = FullCandidateList::get(&store, list_id).expect("candidate list");

        let response = view_candidate_list(
            ViewCandidateListPath { list_id },
            Context::new_test_without_db(),
            full_list,
            store,
            Query(QueryParamState::default()),
        )
        .await?
        .into_response();

        let body = response_body_string(response).await;
        assert!(body.contains("Jansen"));
        assert!(body.contains(&list.add_candidate_path().to_string()));

        Ok(())
    }

    #[tokio::test]
    async fn view_candidate_list_warns_about_ignored_import_columns() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list_id = CandidateListId::new();
        sample_candidate_list(list_id).create(&store).await?;
        let full_list = FullCandidateList::get(&store, list_id).expect("candidate list");

        let query = QueryParamState::import_warnings(
            false,
            &["lijst_nummer".to_string(), "opmerking".to_string()],
        );
        let response = view_candidate_list(
            ViewCandidateListPath { list_id },
            Context::new_test_without_db(),
            full_list,
            store,
            Query(query),
        )
        .await?
        .into_response();

        let body = response_body_string(response).await;
        assert!(body.contains(
            "The following columns in the CSV file were not recognised and have been skipped: lijst_nummer, opmerking."
        ));
        assert!(!body.contains("Only the first"));

        Ok(())
    }

    /// A full list hides the "add candidate" buttons, except while correcting
    /// paper documents: there the list may grow past the hard maximum.
    #[tokio::test]
    async fn view_candidate_list_keeps_add_buttons_while_correcting() -> Result<(), AppError> {
        for correcting in [false, true] {
            let store = if correcting {
                paper_corrections_store().await?
            } else {
                PgStore::new_for_test()
            };
            let list_id = CandidateListId::new();
            let mut list = sample_candidate_list(list_id);

            let mut full = Vec::new();
            for index in 0..crate::MAX_CANDIDATES {
                let person =
                    sample_person_with_last_name(PersonId::new(), &format!("Bakker{index}"));
                person.create(&store).await?;
                full.push(person.id);
            }
            list.candidates = full;
            list.create(&store).await?;

            let full_list = FullCandidateList::get(&store, list_id).expect("candidate list");

            let response = view_candidate_list(
                ViewCandidateListPath { list_id },
                Context::new_test_from_store(&store),
                full_list,
                store,
                Query(QueryParamState::default()),
            )
            .await?
            .into_response();

            let body = response_body_string(response).await;
            assert_eq!(
                body.contains(&list.add_candidate_path().to_string()),
                correcting,
                "paper corrections mode: {correcting}"
            );
        }

        Ok(())
    }
}
