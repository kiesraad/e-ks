use askama::Template;
use axum::response::IntoResponse;

use crate::{
    AppResponse, Context, HtmlTemplate, PgStore, common::PgIndexPath, filters, structs::{
        candidate_lists::CandidateListSummary, common::{PotentialProblems, Severity}, problems::AllProblems,
    },
};

#[derive(Template)]
#[template(path = "pg/common/pages/index.html")]
pub struct IndexTemplate {
    general_problems: usize,
    general_problems_severity: &'static str,
    general_list_problems: usize,
    problematic_lists: usize,
    problematic_lists_severity: &'static str,
}

pub async fn index(
    _: PgIndexPath,
    context: Context,
    store: PgStore,
) -> AppResponse<impl IntoResponse> {
    let political_group = store.get_political_group();
    let general_information_empty = political_group.is_general_information_empty(&store);

    let (general_problems, general_problems_severity) = if general_information_empty {
        (0, "")
    } else {
        let (general_problems, general_infos) = AllProblems::find_general_problems(&store);
        let problems = general_problems.flatten();
        let severity_class = if problems.is_empty() {
            (!general_infos.is_empty()).then_some(Severity::Info)
        } else {
            problems.iter().map(|p| p.severity()).max()
        }
        .map(|severity| severity.class())
        .unwrap_or("success");

        (problems.len() + general_infos.len(), severity_class)
    };
    let mut list_problems =
        AllProblems::find_list_problems(&CandidateListSummary::list(&store), &store);

    // Don't show NoCandidateList problem on the home page, only on the finalise page
    list_problems
        .general
        .retain(|problem| *problem != PotentialProblems::NoCandidateList);

    let (problematic_lists, general_list_problems, problematic_lists_severity) =
        if list_problems.is_empty() {
            (0, 0, "")
        } else {
            let list_count = list_problems.per_list.len();
            let general_count = list_problems.general.len();
            let severity_class = list_problems
                .highest_severity()
                .map(|s| s.class())
                .unwrap_or_default();
            (list_count, general_count, severity_class)
        };

    Ok(HtmlTemplate(
        IndexTemplate {
            general_problems,
            general_problems_severity,
            general_list_problems,
            problematic_lists,
            problematic_lists_severity,
        },
        context,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum_extra::routing::TypedPath;

    use crate::{
        ElectionConfig, QueryParamState,
        core::AnyLocale,
        structs::{list_designation::ListDesignation, political_groups::PoliticalGroup},
        test_utils::{response_body_string, sample_political_group},
    };

    #[tokio::test]
    async fn index_renders_html() {
        let store = PgStore::new_for_test();
        let body = index(PgIndexPath, Context::new_test_from_store(&store), store)
            .await
            .into_response();
        let body = response_body_string(body).await;
        assert!(body.contains(ElectionConfig::EK27.title(AnyLocale::En)));
    }

    fn general_information_card_link(initial: bool) -> String {
        format!(
            "\"{}\"",
            if initial {
                ListDesignation::update_path()
                    .with_query_params(QueryParamState::initial())
                    .to_string()
                    .replace('&', "&#38;")
            } else {
                ListDesignation::update_path().to_string()
            }
        )
    }

    #[tokio::test]
    async fn general_information_link_has_initial_when_empty() {
        let store = PgStore::new_for_test();

        // Reset to an empty political group
        PoliticalGroup::default().update(&store).await.unwrap();
        let pg = store.get_political_group();
        assert!(pg.is_general_information_empty(&store));

        let body = index(PgIndexPath, Context::new_test_from_store(&store), store)
            .await
            .into_response();
        let body = response_body_string(body).await;

        assert!(body.contains(&general_information_card_link(true)));
        assert!(!body.contains(&general_information_card_link(false)));
    }

    #[tokio::test]
    async fn general_information_link_has_no_initial_when_not_empty() {
        let store = PgStore::new_for_test();

        // Sample political group with filled in values
        sample_political_group().update(&store).await.unwrap();
        let pg = store.get_political_group();
        assert!(!pg.is_general_information_empty(&store));

        let body = index(PgIndexPath, Context::new_test_from_store(&store), store)
            .await
            .into_response();
        let body = response_body_string(body).await;

        assert!(!body.contains(&general_information_card_link(true)));
        assert!(body.contains(&general_information_card_link(false)));
    }
}
