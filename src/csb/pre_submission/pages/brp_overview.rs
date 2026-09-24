//! The overview of one pre-submitted package, as PDF and Word download: the
//! candidates the BRP check found wanting or the application flagged, with
//! their details and what was found.

use axum::response::Response;

use crate::{
    AppError, CsbStream, ElectionConfig, Locale,
    core::ModelLocale,
    csb::{
        examination::structs::BrpCheckState,
        pre_submission::{
            extractors::PreSubmissionStore,
            pages::{CsbPreSubmissionBrpOverviewDocxPath, CsbPreSubmissionBrpOverviewPdfPath},
        },
    },
    models::{
        Pdf,
        brp_overview::{BrpOverview, CandidateDetail, OverviewCandidate},
    },
    projection::WithCorrections,
    structs::{brp::BrpCheckedField, common::Problematic, persons::Person},
};

/// Collect the store data the overview needs. The document is Dutch, whatever
/// the session's locale, as it is handed to the political group.
pub(super) fn brp_overview_model(store: &CsbStream) -> BrpOverview {
    let locale = Locale::Nl;
    let election = store.election;
    let findings = store.get_brp_findings();

    // Counted over every listed candidate, not only the ones reported on.
    let mut candidates_without_brp_errors = 0;
    let mut candidates_with_brp_errors = 0;
    let mut brp_error_count = 0;
    let mut problem_count = 0;

    let candidates = store
        .listed_candidates()
        .into_iter()
        .filter_map(|candidate| {
            let findings: Vec<String> = match findings.get(&candidate.person.id) {
                Some(found) if found.is_empty() => {
                    candidates_without_brp_errors += 1;
                    Vec::new()
                }
                Some(found) => {
                    candidates_with_brp_errors += 1;
                    brp_error_count += found.len();
                    found
                        .iter()
                        .map(|finding| finding.message(locale))
                        .collect()
                }
                // Not checked: reported by `complete`, counted nowhere.
                None => Vec::new(),
            };
            let problems = candidate_problems(&candidate.person, election, locale);
            problem_count += problems.len();
            if findings.is_empty() && problems.is_empty() {
                return None;
            }

            Some(OverviewCandidate {
                position: candidate.position,
                name: candidate.person.name.display(),
                details: candidate_details(&candidate.person, locale),
                findings,
                problems,
            })
        })
        .collect();

    let state = BrpCheckState::for_political_group(store);

    BrpOverview {
        election_name: election.formal_title(ModelLocale::Nl),
        appellation: store.get_appellation(WithCorrections::All),
        election_code: election.filename_slug(),
        date: chrono::Utc::now().date_naive(),
        complete: !state.is_not_checked() && !state.is_incomplete(),
        candidates_without_brp_errors,
        candidates_with_brp_errors,
        brp_error_count,
        problem_count,
        candidates,
    }
}

/// What the application flagged about the candidate's details, translated:
/// the potential problems first, the informational ones after them.
fn candidate_problems(person: &Person, election: ElectionConfig, locale: Locale) -> Vec<String> {
    let problems = person.get_problems(election);
    problems
        .potential_problems
        .iter()
        .map(|problem| problem.translate(&locale))
        .chain(
            problems
                .info_problems
                .iter()
                .map(|problem| problem.translate(&locale)),
        )
        .collect()
}

/// The candidate's details, one row per field the BRP check compares.
fn candidate_details(person: &Person, locale: Locale) -> Vec<CandidateDetail> {
    BrpCheckedField::IN_TABLE_ORDER
        .into_iter()
        .map(|field| CandidateDetail {
            label: field.label(locale),
            value: field.value_of(person, locale),
        })
        .collect()
}

/// The overview of one pre-submitted package, as PDF.
pub async fn gen_brp_overview(
    _: CsbPreSubmissionBrpOverviewPdfPath,
    store: PreSubmissionStore,
) -> Result<Response, AppError> {
    brp_overview_model(&store).pdf_response().await
}

/// The same overview as [`gen_brp_overview`], exported as a Word document.
pub async fn gen_brp_overview_docx(
    _: CsbPreSubmissionBrpOverviewDocxPath,
    store: PreSubmissionStore,
) -> Result<Response, AppError> {
    brp_overview_model(&store).docx_response().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::to_bytes,
        http::{StatusCode, header},
    };

    use crate::{
        CsbAction, CsbStore,
        models::DOCX_CONTENT_TYPE,
        structs::{
            brp::{BrpFinding, BrpStatus, BrpValue},
            candidate_lists::CandidateListId,
            persons::PersonId,
        },
        test_utils::{sample_candidate_list, sample_person_with_last_name, sample_political_group},
    };

    /// The sample group with three candidates: the first found wanting by the
    /// BRP on two fields, the second agreeing with it, the third not checked
    /// and without a BSN.
    async fn store_with_candidates() -> PreSubmissionStore {
        let store = CsbStore::new_for_test();
        store.set_political_group(sample_political_group());

        let mut list = sample_candidate_list(CandidateListId::new());
        let mut ids = Vec::new();
        for last_name in ["Eerste", "Tweede", "Derde"] {
            let mut person = sample_person_with_last_name(PersonId::new(), last_name);
            if last_name == "Derde" {
                person.personal_data.bsn = None;
            }
            ids.push(person.id);
            store.add_person(person);
        }
        list.candidates = ids.clone();
        store.add_candidate_list(list);

        store
            .update(CsbAction::BrpPersonChecked {
                person: ids[0],
                findings: vec![
                    BrpFinding::NotDutch,
                    BrpFinding::Mismatch {
                        brp_value: BrpValue::PlaceOfResidence("Utrecht".parse().unwrap()),
                    },
                ],
            })
            .await
            .unwrap();
        store
            .update(CsbAction::BrpPersonChecked {
                person: ids[1],
                findings: Vec::new(),
            })
            .await
            .unwrap();
        store
            .update(CsbAction::SetBrpStatus(BrpStatus::Finished))
            .await
            .unwrap();

        PreSubmissionStore(store)
    }

    #[tokio::test]
    async fn the_model_lists_the_candidates_with_something_to_report() {
        let store = store_with_candidates().await;

        let model = brp_overview_model(&store);

        assert_eq!(model.appellation, "Kiesraad Demo");
        assert_eq!(model.election_code, "ek27");
        // One candidate was never checked.
        assert!(!model.complete);
        assert_eq!(model.candidates_without_brp_errors, 1);
        assert_eq!(model.candidates_with_brp_errors, 1);
        assert_eq!(model.brp_error_count, 2);
        assert_eq!(model.problem_count, 1);

        let names: Vec<(usize, &str)> = model
            .candidates
            .iter()
            .map(|candidate| (candidate.position, candidate.name.as_str()))
            .collect();
        assert_eq!(names, [(1, "Eerste, H.A.H.A."), (3, "Derde, H.A.H.A.")]);

        let first = &model.candidates[0];
        assert_eq!(first.findings.len(), 2);
        assert!(
            first.findings[1].contains("Utrecht"),
            "{:?}",
            first.findings
        );
        assert!(first.problems.is_empty());
        let labels: Vec<&str> = first
            .details
            .iter()
            .map(|detail| detail.label.as_str())
            .collect();
        assert_eq!(
            labels,
            [
                "Voorletters",
                "Voorvoegsel",
                "Achternaam",
                "Geslacht",
                "Geboortedatum",
                "Burgerservicenummer (BSN)",
                "Woonplaats"
            ]
        );
        assert_eq!(first.details[2].value, "Eerste");

        // The third has no BRP findings, only the application's own warning.
        let third = &model.candidates[1];
        assert!(third.findings.is_empty());
        assert_eq!(third.problems.len(), 1, "{:?}", third.problems);
        assert_eq!(third.details[5].value, "");
    }

    #[tokio::test]
    async fn gen_brp_overview_returns_a_pdf_named_after_the_group() -> Result<(), AppError> {
        let store = store_with_candidates().await;

        let response = gen_brp_overview(
            CsbPreSubmissionBrpOverviewPdfPath {
                stream_id: store.stream_id,
            },
            store,
        )
        .await?;

        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers().clone();
        assert_eq!(
            headers.get(header::CONTENT_TYPE).expect("content type"),
            "application/pdf"
        );
        assert_eq!(
            headers
                .get(header::CONTENT_DISPOSITION)
                .expect("content disposition"),
            "attachment; filename=\"brp-overzicht-kiesraad-demo-ek27.pdf\""
        );
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        assert!(body.starts_with(b"%PDF"), "body is not a PDF");

        Ok(())
    }

    #[tokio::test]
    async fn gen_brp_overview_docx_returns_a_word_document() -> Result<(), AppError> {
        let store = store_with_candidates().await;

        let response = gen_brp_overview_docx(
            CsbPreSubmissionBrpOverviewDocxPath {
                stream_id: store.stream_id,
            },
            store,
        )
        .await?;

        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers().clone();
        assert_eq!(
            headers.get(header::CONTENT_TYPE).expect("content type"),
            DOCX_CONTENT_TYPE
        );
        assert_eq!(
            headers
                .get(header::CONTENT_DISPOSITION)
                .expect("content disposition"),
            "attachment; filename=\"brp-overzicht-kiesraad-demo-ek27.docx\""
        );
        // A .docx is a ZIP archive.
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        assert!(body.starts_with(b"PK"), "body is not a ZIP archive");

        Ok(())
    }
}
