use askama::Template;
use axum::response::{IntoResponse, Response};

use crate::{
    AnyLocale, AppError, Context, CsbContext, CsbStore, ElectoralDistrict, HtmlTemplate,
    csb::examination::{
        extractors::CsbPoliticalGroup, pages::CsbCandidateListPath, structs::CsbCandidate,
    },
    filters,
    structs::{
        candidate_lists::{CandidateList, CandidateListId},
        csb::{CsbPhase, Omission},
    },
};

#[derive(Template)]
#[template(path = "csb/examination/pages/candidate_list.html")]
struct CsbCandidateListTemplate {
    political_group: CsbPoliticalGroup,
    list_id: CandidateListId,
    electoral_districts: Vec<ElectoralDistrict>,
    candidates: Vec<CsbCandidate>,
    omissions: Vec<Omission>,
    is_scrapped: bool,
    scrapped_districts: Vec<ElectoralDistrict>,
    all_districts_scrapped: bool,
}

pub async fn overview(
    CsbCandidateListPath { list_id, .. }: CsbCandidateListPath,
    context: CsbContext,
    store: CsbStore,
) -> Result<Response, AppError> {
    render(list_id, context, store, CsbPhase::Examination).await
}

/// The candidate list page, shared between the examination and the recovery
/// ("Herstelde lijsten") phase.
pub(in crate::csb) async fn render(
    list_id: CandidateListId,
    context: CsbContext,
    store: CsbStore,
    mode: CsbPhase,
) -> Result<Response, AppError> {
    let political_group = CsbPoliticalGroup::new_from_csb_store(&store).with_mode(mode);
    let corrected_list = store.get_candidate_list(list_id, crate::projection::WithCorrections::All);
    // For paper-added lists there is no imported side; use an empty-candidate
    // placeholder so all candidates render as paper-corrected additions.
    let imported_list = store
        .get_candidate_list(list_id, crate::projection::WithCorrections::None)
        .or_else(|| {
            corrected_list.as_ref().map(|corrected| CandidateList {
                candidates: Vec::new(),
                ..corrected.clone()
            })
        })
        .ok_or(AppError::GenericNotFound)?;

    let candidates = CsbCandidate::rows_for_list(
        &store,
        &imported_list,
        AnyLocale::from(context.session.locale),
    );

    let omissions = store.get_candidate_list_omissions(list_id)?;

    // The corrected electoral districts take precedence over the imported ones.
    let electoral_districts = corrected_list
        .map(|corrected| corrected.electoral_districts)
        .unwrap_or(imported_list.electoral_districts);

    let scrapped_districts = store.get_candidate_list_scrapped_districts(list_id);
    let all_districts_scrapped =
        !electoral_districts.is_empty() && scrapped_districts.len() == electoral_districts.len();

    Ok(HtmlTemplate(
        CsbCandidateListTemplate {
            political_group,
            list_id,
            electoral_districts,
            candidates,
            omissions,
            is_scrapped: store.is_candidate_list_scrapped(list_id)?,
            scrapped_districts,
            all_districts_scrapped,
        },
        context,
    )
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::http::StatusCode;

    use crate::{
        structs::{csb::OmissionCategory, persons::PersonId},
        test_utils::{response_body_string, sample_candidate_list, sample_person},
    };

    #[tokio::test]
    async fn renders_candidates_with_brp_column_and_add_omission_button() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let person = sample_person(PersonId::new());
        let person_id = person.id;
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id];
        store.add_person(person);
        store.add_candidate_list(list);

        let response = overview(
            CsbCandidateListPath { stream_id, list_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // The candidate is listed and the BRP column header renders.
        assert!(body.contains("Jansen"));
        assert!(body.contains("BRP check"));
        // The add omission button links to the candidate list omission dialog.
        assert!(body.contains(&format!(
            "/csb/examination/{stream_id}/omission/candidate-list/"
        )));
        // Rows link to the candidate detail page.
        assert!(body.contains(&format!(
            "/csb/examination/{stream_id}/list/{list_id}/candidate/{person_id}"
        )));
        // The list's electoral districts are shown (the sample list covers Utrecht).
        assert!(body.contains("Electoral districts"));
        assert!(body.contains("Utrecht"));
    }

    #[tokio::test]
    async fn renders_added_candidate_list_omissions_as_badges() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let list_id = CandidateListId::new();
        store.add_candidate_list(sample_candidate_list(list_id));
        Omission::new(
            OmissionCategory::CandidateList(vec![list_id]),
            "Too many candidates".parse().unwrap(),
            "The list holds more candidates than allowed."
                .parse()
                .unwrap(),
            None,
        )
        .create(&store)
        .await
        .unwrap();

        let response = overview(
            CsbCandidateListPath { stream_id, list_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("restoration-tag"));
        // The badge shows the short title, not the long description.
        assert!(body.contains("Too many candidates"));
        assert!(!body.contains("The list holds more candidates than allowed."));
    }

    #[tokio::test]
    async fn renders_paper_added_list_with_candidates_as_corrections() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let person = sample_person(PersonId::new());
        let person_id = person.id;
        store.add_person(person);
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id];
        store.set_paper_corrected_candidate_list(list);

        let response = overview(
            CsbCandidateListPath { stream_id, list_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // The paper-added list has no imported side, so its candidates render
        // as paper-corrected additions.
        assert!(body.contains("paper-corrected-value"));
        assert!(body.contains("Jansen"));
        // The corrected list's electoral districts are shown.
        assert!(body.contains("Utrecht"));
    }

    #[tokio::test]
    async fn shows_corrected_electoral_districts() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let list_id = CandidateListId::new();
        store.add_candidate_list(sample_candidate_list(list_id));
        let mut corrected = sample_candidate_list(list_id);
        corrected.electoral_districts = vec![ElectoralDistrict::Groningen];
        store.set_paper_corrected_candidate_list(corrected);

        let response = overview(
            CsbCandidateListPath { stream_id, list_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // The corrected districts replace the imported ones.
        assert!(body.contains("Groningen"));
        assert!(!body.contains("Utrecht"));
    }

    #[tokio::test]
    async fn recovery_mode_hides_the_brp_column() {
        let store = CsbStore::new_for_test();
        let list_id = CandidateListId::new();

        let person = sample_person(PersonId::new());
        let person_id = person.id;
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id];
        store.add_person(person);
        store.add_candidate_list(list);

        let response = render(list_id, CsbContext::new_test(), store, CsbPhase::Recovery)
            .await
            .unwrap()
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // The candidates still render, but the BRP check is examination-only.
        assert!(body.contains("Jansen"));
        assert!(!body.contains("BRP"));
    }

    #[tokio::test]
    async fn returns_not_found_for_unknown_list() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let result = overview(
            CsbCandidateListPath {
                stream_id,
                list_id: CandidateListId::new(),
            },
            CsbContext::new_test(),
            store,
        )
        .await;

        assert!(matches!(result, Err(AppError::GenericNotFound)));
    }

    #[tokio::test]
    async fn examination_candidate_list_never_renders_scrapped() {
        use crate::csb::examination::pages::candidate_list::render;

        let store = CsbStore::new_for_test();

        let person = sample_person(PersonId::new());
        let person_id = person.id;
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id];
        store.add_person(person);
        store.add_candidate_list(list);

        // An irreparable omission is unresolved from the moment it is added,
        // but scrapping must not leak into the examination phase.
        let mut omission = Omission::new(
            OmissionCategory::Candidate {
                person: person_id,
                lists: vec![list_id],
            },
            "Unregistered".parse().unwrap(),
            "The candidate cannot be registered.".parse().unwrap(),
            None,
        );
        omission.recoverable = false;
        omission.create(&store).await.unwrap();

        let response = render(
            list_id,
            CsbContext::new_test(),
            store,
            CsbPhase::Examination,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(!body.contains("Scrapped"));
    }
}
