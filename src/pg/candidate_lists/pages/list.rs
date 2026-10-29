use crate::structs::candidate_lists::{CandidateList, CandidateListSummary};
use askama::Template;
use axum::response::IntoResponse;

use crate::{
    AppError, Context, HtmlTemplate, PgStore,
    candidate_lists::pages::CandidateListsPath,
    filters,
    structs::{
        candidate_lists::CandidateListWithProblems,
        common::{HasSeverity, Problematic, Severity},
        persons::Person,
    },
};

#[derive(Template)]
#[template(path = "pg/candidate_lists/pages/list.html")]
struct CandidateListIndexTemplate {
    candidate_lists: Vec<CandidateListWithProblems>,
    total_persons: usize,
    persons_with_problems: usize,
    person_problem_severity: &'static str,
}

pub async fn list_candidate_lists(
    _: CandidateListsPath,
    context: Context,
    store: PgStore,
) -> Result<impl IntoResponse, AppError> {
    let data = store.snapshot();
    let mut candidate_lists = Vec::new();
    for summary in CandidateListSummary::list(&data, store.election) {
        let problems = summary.get_problems(());
        candidate_lists.push(CandidateListWithProblems {
            data: summary,
            problems,
        });
    }
    let persons = data.get_persons();
    let problem_severities = persons
        .iter()
        .filter_map(|p| p.get_problems(context.election).highest_severity())
        .collect::<Vec<_>>();
    let persons_with_problems = problem_severities.len();
    let person_problem_severity = problem_severities
        .iter()
        .max()
        .map(Severity::class)
        .unwrap_or_default();

    Ok(HtmlTemplate(
        CandidateListIndexTemplate {
            candidate_lists,
            total_persons: persons.len(),
            persons_with_problems,
            person_problem_severity,
        },
        context,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Context, ElectionConfig, Locale, PgStore, Session, WaterCouncil,
        structs::{candidate_lists::CandidateListId, persons::PersonId},
        test_utils::{response_body_string, sample_candidate_list, sample_person},
    };
    use axum::{http::StatusCode, response::IntoResponse};

    #[tokio::test]
    async fn list_tile_is_warning_with_candidate_error() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let mut person = sample_person(PersonId::new());
        person.personal_data.date_of_birth = None;
        person.create(&store).await?;

        let mut list = sample_candidate_list(CandidateListId::new());
        list.candidates = vec![person.id];
        list.create(&store).await?;

        let response =
            list_candidate_lists(CandidateListsPath {}, Context::new_test_without_db(), store)
                .await?
                .into_response();
        let body = response_body_string(response).await;

        assert!(body.contains("badge badge-candidates-list warning"));

        Ok(())
    }

    #[tokio::test]
    async fn list_candidate_lists_shows_created_list() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let list = sample_candidate_list(CandidateListId::new());
        list.create(&store).await?;

        let response =
            list_candidate_lists(CandidateListsPath {}, Context::new_test_without_db(), store)
                .await?
                .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Utrecht"));

        Ok(())
    }

    #[tokio::test]
    async fn multi_district_add_card_links_to_create_form() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        let response =
            list_candidate_lists(CandidateListsPath {}, Context::new_test_without_db(), store)
                .await?
                .into_response();
        let body = response_body_string(response).await;

        let create_path = CandidateList::create_path().to_string();
        assert!(body.contains(&format!(r#"data-href="{create_path}""#)));
        assert!(body.contains(
            "it is possible to create different lists for different electoral districts."
        ));

        Ok(())
    }

    #[tokio::test]
    async fn candidate_register_is_always_visible() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        let response = list_candidate_lists(
            CandidateListsPath {},
            Context::new_test_without_db(),
            store.clone(),
        )
        .await?
        .into_response();
        let body = response_body_string(response).await;

        assert!(body.contains("Candidate register"));
        assert!(body.contains("candidates in total")); // Zero should be pluralised

        let person = sample_person(PersonId::new());
        person.create(&store).await?;

        let response =
            list_candidate_lists(CandidateListsPath {}, Context::new_test_without_db(), store)
                .await?
                .into_response();
        let body = response_body_string(response).await;
        assert!(body.contains("candidate in total")); // One should be singular

        Ok(())
    }

    /// The one list is created by a POST, so the card must not link to a GET.
    #[tokio::test]
    async fn single_district_add_card_posts_without_note() -> Result<(), AppError> {
        let election = ElectionConfig::WS27(WaterCouncil::AaEnMaas);
        let store = PgStore::new_for_test_with_election(election);
        let mut context = Context::new(&store, Session::new_test_with_locale(Locale::En));
        context.election = election;

        let response = list_candidate_lists(CandidateListsPath {}, context, store)
            .await?
            .into_response();
        let body = response_body_string(response).await;

        let create_path = CandidateList::create_path().to_string();
        assert!(body.contains(&format!(r#"action="{create_path}""#)));
        assert!(body.contains(r#"input type="hidden" name="csrf_token""#));
        // also covers data-href
        assert!(!body.contains(&format!(r#"href="{create_path}""#)));
        assert!(!body.contains("Multiple lists can be created"));

        Ok(())
    }
}
