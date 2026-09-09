use axum::{extract::State, http::HeaderValue, response::IntoResponse};

use crate::{
    AppError, AppRequestState, CsbMainStore,
    core::{ModelLocale, constants::DEFAULT_DATE_FORMAT},
    csb::examination::{
        model_inputs::{found_omissions, submitted_lists},
        pages::{CsbI1DocxDownloadPath, CsbI1DownloadPath},
    },
    models::{Pdf, i1::I1},
    utils::no_cache_headers,
};

const PDF_CONTENT_TYPE: &str = "application/pdf";
const DOCX_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

/// Collect the store data the I 1 model needs.
async fn i1_model<S: AppRequestState>(main_store: CsbMainStore, state: &S) -> Result<I1, AppError> {
    let election = main_store.election;
    let registry = state.csb_store_registry();
    let submitted_lists = submitted_lists(registry, &election).await?;
    let found_omissions = found_omissions(registry, &election).await?;

    Ok(I1 {
        election_name: election.formal_title(ModelLocale::Nl),
        election_date: election
            .election_date()
            .format(DEFAULT_DATE_FORMAT)
            .to_string(),
        session: election.public_session().into(),
        submitted_lists,
        found_omissions,
    })
}

pub async fn gen_i1<S: AppRequestState>(
    _: CsbI1DownloadPath,
    main_store: CsbMainStore,
    State(state): State<S>,
) -> Result<impl IntoResponse, AppError> {
    let model = i1_model(main_store, &state).await?;
    let filename = model.filename();
    let bytes = model.generate_bytes().await?;

    let headers = no_cache_headers::generate_attachment_headers(
        &filename,
        HeaderValue::from_static(PDF_CONTENT_TYPE),
    )?;

    Ok((headers, bytes).into_response())
}

/// The same I 1 as [`gen_i1`], exported as a Word document.
pub async fn gen_i1_docx<S: AppRequestState>(
    _: CsbI1DocxDownloadPath,
    main_store: CsbMainStore,
    State(state): State<S>,
) -> Result<impl IntoResponse, AppError> {
    let model = i1_model(main_store, &state).await?;
    let filename = model.docx_filename();
    let bytes = model.generate_docx_bytes().await?;

    let headers = no_cache_headers::generate_attachment_headers(
        &filename,
        HeaderValue::from_static(DOCX_CONTENT_TYPE),
    )?;

    Ok((headers, bytes).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::to_bytes,
        http::{StatusCode, header},
        response::IntoResponse,
    };

    use crate::{
        AppState, CsbAction, ElectionConfig, PgStoreData, StreamId,
        structs::{
            candidate_lists::CandidateList,
            csb::{OmissionCategory, sample_omission},
            list_designation::ListDesignation,
            persons::PersonId,
            political_groups::PoliticalGroup,
        },
        test_utils::sample_person,
    };

    #[tokio::test]
    async fn gen_i1_returns_pdf_response() -> Result<(), AppError> {
        let main_store = CsbMainStore::new_for_test();
        let state = AppState::new_for_tests().await;
        let response = gen_i1(CsbI1DownloadPath, main_store, State(state))
            .await?
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers();
        assert_eq!(
            headers.get(header::CONTENT_TYPE).expect("content type"),
            "application/pdf"
        );
        assert_eq!(
            headers
                .get(header::CONTENT_DISPOSITION)
                .expect("content disposition"),
            "attachment; filename=\"i1-proces-verbaal.pdf\""
        );
        assert_eq!(
            headers.get(header::CACHE_CONTROL).expect("cache control"),
            "no-store, no-cache, must-revalidate, max-age=0"
        );

        Ok(())
    }

    #[tokio::test]
    async fn gen_i1_docx_returns_word_response() -> Result<(), AppError> {
        let main_store = CsbMainStore::new_for_test();
        let state = AppState::new_for_tests().await;
        let response = gen_i1_docx(CsbI1DocxDownloadPath, main_store, State(state))
            .await?
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers();
        assert_eq!(
            headers.get(header::CONTENT_TYPE).expect("content type"),
            DOCX_CONTENT_TYPE
        );
        assert_eq!(
            headers
                .get(header::CONTENT_DISPOSITION)
                .expect("content disposition"),
            "attachment; filename=\"i1-proces-verbaal.docx\""
        );

        // A .docx is a ZIP archive.
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        assert!(body.starts_with(b"PK"), "body is not a ZIP archive");

        Ok(())
    }

    /// The empty-registry case above never fills the submitted-lists and
    /// omissions sections; drive the handler once with a group that has both.
    #[tokio::test]
    async fn gen_i1_renders_the_imported_lists_and_omissions() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let store = state
            .csb_store_for_stream(StreamId::new(), ElectionConfig::EK27)
            .await?
            .acting_as_test_user();

        let person = sample_person(PersonId::new());
        let mut snapshot = PgStoreData {
            political_group: PoliticalGroup {
                appellation: Some("Kiesraad Demo".parse().unwrap()),
                list_designation: Some(ListDesignation::Standalone),
                ..Default::default()
            },
            ..PgStoreData::default()
        };
        snapshot.persons.insert(person.id, person.clone());
        let list = CandidateList {
            electoral_districts: vec![crate::ElectoralDistrict::Groningen],
            candidates: vec![person.id],
            ..Default::default()
        };
        snapshot.candidate_lists.insert(list.id, list);
        store
            .update(CsbAction::Import {
                hash: [0u8; 32],
                source_stream_id: StreamId::new(),
                snapshot: Box::new(snapshot),
            })
            .await?;
        sample_omission(OmissionCategory::PoliticalGroup)
            .create(&store)
            .await?;

        let main_store = CsbMainStore::new_for_test();
        let response = gen_i1(CsbI1DownloadPath, main_store, State(state))
            .await?
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        assert!(body.starts_with(b"%PDF"), "body is not a PDF");

        Ok(())
    }
}
