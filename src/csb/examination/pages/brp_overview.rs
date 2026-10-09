//! The overview of one group's BRP findings for the political group, as PDF,
//! Word and Markdown download: the same document the pre-submission check
//! offers, now for an imported list under examination.

use axum::response::Response;

use crate::{
    AppError, CsbStore,
    csb::{
        examination::pages::{
            CsbBrpOverviewDocxDownloadPath, CsbBrpOverviewDownloadPath,
            CsbBrpOverviewMarkdownDownloadPath,
        },
        pre_submission::pages::brp_overview::brp_overview_model,
    },
    models::Pdf,
};

/// The overview of one examined group, as PDF.
pub async fn gen_brp_overview(
    _: CsbBrpOverviewDownloadPath,
    store: CsbStore,
) -> Result<Response, AppError> {
    brp_overview_model(&store).pdf_response().await
}

/// The same overview as [`gen_brp_overview`], exported as a Word document.
pub async fn gen_brp_overview_docx(
    _: CsbBrpOverviewDocxDownloadPath,
    store: CsbStore,
) -> Result<Response, AppError> {
    brp_overview_model(&store).docx_response().await
}

/// The same overview as [`gen_brp_overview`], exported as Markdown.
pub async fn gen_brp_overview_markdown(
    _: CsbBrpOverviewMarkdownDownloadPath,
    store: CsbStore,
) -> Result<Response, AppError> {
    brp_overview_model(&store).markdown_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::to_bytes,
        http::{StatusCode, header},
    };

    use crate::{
        CsbAction,
        models::{DOCX_CONTENT_TYPE, MARKDOWN_CONTENT_TYPE},
        structs::{
            brp::{BrpFindingKind, BrpStatus},
            candidate_lists::CandidateListId,
            persons::PersonId,
        },
        test_utils::{sample_candidate_list, sample_person_with_last_name, sample_political_group},
    };

    /// The sample group with one candidate, checked with a finding.
    async fn store_with_a_checked_candidate() -> CsbStore {
        let store = CsbStore::new_for_test();
        store.set_political_group(sample_political_group());
        let person = sample_person_with_last_name(PersonId::new(), "Kandidaat");
        let person_id = person.id;
        let mut list = sample_candidate_list(CandidateListId::new());
        list.candidates = vec![person_id];
        store.add_person(person);
        store.add_candidate_list(list);

        store
            .update(CsbAction::BrpPersonChecked {
                person: person_id,
                findings: vec![BrpFindingKind::NotDutch.into()],
            })
            .await
            .unwrap();
        store
            .update(CsbAction::SetBrpStatus(BrpStatus::Finished))
            .await
            .unwrap();

        store
    }

    #[tokio::test]
    async fn gen_brp_overview_returns_a_pdf_named_after_the_group() -> Result<(), AppError> {
        let store = store_with_a_checked_candidate().await;

        let response = gen_brp_overview(
            CsbBrpOverviewDownloadPath {
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
        let store = store_with_a_checked_candidate().await;

        let response = gen_brp_overview_docx(
            CsbBrpOverviewDocxDownloadPath {
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

    #[tokio::test]
    async fn gen_brp_overview_markdown_returns_the_document_as_markdown() -> Result<(), AppError> {
        let store = store_with_a_checked_candidate().await;

        let response = gen_brp_overview_markdown(
            CsbBrpOverviewMarkdownDownloadPath {
                stream_id: store.stream_id,
            },
            store,
        )
        .await?;

        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers().clone();
        assert_eq!(
            headers.get(header::CONTENT_TYPE).expect("content type"),
            MARKDOWN_CONTENT_TYPE
        );
        assert_eq!(
            headers
                .get(header::CONTENT_DISPOSITION)
                .expect("content disposition"),
            "attachment; filename=\"brp-overzicht-kiesraad-demo-ek27.md\""
        );
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let body = String::from_utf8(body.to_vec()).expect("utf-8");
        assert!(
            body.starts_with("# Overzicht controle voorinlevering"),
            "{body}"
        );
        assert!(body.contains("#### Kandidaat nr"), "{body}");
        assert!(body.contains("Kandidaat"), "{body}");

        Ok(())
    }
}
