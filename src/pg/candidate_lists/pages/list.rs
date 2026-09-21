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
    all_districts_assigned: bool,
}

pub async fn list_candidate_lists(
    _: CandidateListsPath,
    context: Context,
    store: PgStore,
) -> Result<impl IntoResponse, AppError> {
    let mut candidate_lists = Vec::new();
    for summary in CandidateListSummary::list(&store) {
        let problems = summary.get_problems(());
        candidate_lists.push(CandidateListWithProblems {
            data: summary,
            problems,
        });
    }
    let persons = store.get_persons();
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

    let all_districts_assigned =
        CandidateList::available_districts(&store, &context.election).is_empty();

    Ok(HtmlTemplate(
        CandidateListIndexTemplate {
            candidate_lists,
            total_persons: persons.len(),
            persons_with_problems,
            person_problem_severity,
            all_districts_assigned,
        },
        context,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Context, PgStore,
        structs::candidate_lists::CandidateListId,
        test_utils::{response_body_string, sample_candidate_list},
    };
    use axum::{http::StatusCode, response::IntoResponse};

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
}
