use axum::{
    body::Body,
    extract::State,
    http::HeaderValue,
    response::{IntoResponse, Response},
};
use eml_nl::utils::ElectionId;
use tokio::io::{AsyncWrite, duplex};
use tokio_util::io::ReaderStream;

use crate::{
    AppError, AppRequestState, CsbContext, CsbMainStore,
    core::ZipResponseWriter,
    csb::finalise::{
        pages::{
            combined_zip::{
                candidate_list_overview::candidate_list_overview_model,
                candidate_list_per_district::candidate_list_per_district_model,
                candidate_list_publication::candidate_list_publication_model,
                eml::{eml110a_files, eml230_files},
            },
            i4::i4_model,
        },
        paths::CsbFinalZipDownloadPath,
    },
    models::{Pdf, documents::ZIP_CONTENT_TYPE, i4::I4Status},
    utils::no_cache_headers,
};

mod candidate_list_overview;
mod candidate_list_per_district;
mod candidate_list_publication;
mod eml;

 type Files = Vec<(String, Vec<u8>)>;

/// All finalisation documents in one ZIP available on the CSB finalise page
pub async fn download_final_zip<S: AppRequestState>(
    _: CsbFinalZipDownloadPath,
    context: CsbContext,
    main_store: CsbMainStore,
    State(state): State<S>,
) -> Result<Response, AppError> {
    let files = final_files(&context, &main_store, &state).await?;
    let filename = format!(
        "Afronding_Kandidaatstellen_{}.zip",
        main_store.election.filename_slug()
    );

    zip_response(&filename, files)
}

/// The filenames of the final documents, along with the raw bytes that represent them
async fn final_files<S: AppRequestState>(
    context: &CsbContext,
    main_store: &CsbMainStore,
    state: &S,
) -> Result<Files, AppError> {
    let election_id = ElectionId::try_from(main_store.election)?;

    let mut lists_per_district = Vec::new();
    for model in candidate_list_per_district_model(context, main_store, state).await? {
        lists_per_district.extend(pdf_and_docx(&model).await?);
    }
    let (eml230bs, eml230c) = eml230_files(context, state.csb_store_registry(), main_store).await?;
    let (eml110a, hash) = eml110a_files(main_store)?;

    let mut files = vec![
        (
            "Overzicht_Kandidatenlijsten.zip".to_string(),
            zip(&lists_per_district).await?,
        ),
        (
            format!("Totaallijsten_{}.zip", election_id.value()),
            zip(&eml230bs).await?,
        ),
        (
            format!("Kanidatenlijsten_{}.zip", election_id.value()),
            zip(&eml230c).await?,
        ),
        (
            format!("Verkiezingsdefinitie_{}.zip", election_id.value()),
            zip(&eml110a).await?,
        ),
    ];

    files.extend(
        pdf_and_docx(&candidate_list_publication_model(context, main_store, state).await?).await?,
    );
    files.extend(
        pdf_and_docx(&candidate_list_overview_model(context, main_store, state).await?).await?,
    );
    files.extend(
        pdf_and_docx(&i4_model(context, main_store, state, I4Status::Final(hash)).await?).await?,
    );

    Ok(files)
}

/// `model` rendered as PDF and as Word document
async fn pdf_and_docx(model: &impl Pdf) -> Result<[(String, Vec<u8>); 2], AppError> {
    Ok([
        (model.filename(), model.generate_bytes().await?),
        (model.docx_filename(), model.generate_docx_bytes().await?),
    ])
}

fn zip_response(filename: &str, files: Files) -> Result<Response, AppError> {
    let headers = no_cache_headers::generate_attachment_headers(
        filename,
        HeaderValue::from_static(ZIP_CONTENT_TYPE),
    )?;

    let (reader, writer) = duplex(64 * 1024);
    tokio::spawn(async move {
        if let Err(err) = write_zip(&files, writer).await {
            tracing::error!(error = ?err, "failed to stream ZIP");
        }
    });

    Ok((headers, Body::from_stream(ReaderStream::new(reader))).into_response())
}

async fn zip(files: &Files) -> Result<Vec<u8>, AppError> {
    let mut buffer = Vec::new();
    write_zip(files, &mut buffer).await?;

    Ok(buffer)
}

async fn write_zip(
    files: &[(String, Vec<u8>)],
    writer: impl AsyncWrite + Unpin,
) -> Result<(), AppError> {
    let mut zipper = ZipResponseWriter::new(writer);
    for (name, bytes) in files {
        zipper.add_file(name, bytes).await?;
    }

    zipper.finish().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{StatusCode, header};
    use std::collections::BTreeSet;

    use crate::{
        AppState, ElectionConfig, ElectoralDistrict, StreamId,
        structs::{candidate_lists::CandidateList, persons::PersonId},
        test_utils::{
            sample_person, sample_political_group_named, seed_csb_group, zip_entry_names,
        },
    };

    /// Import a group named `appellation` with one candidate on a list in
    /// Groningen and Drenthe.
    async fn seed_group(state: &AppState, appellation: &str) -> StreamId {
        let candidate = sample_person(PersonId::new());
        let list = CandidateList {
            electoral_districts: BTreeSet::from([
                ElectoralDistrict::Groningen,
                ElectoralDistrict::Drenthe,
            ]),
            candidates: vec![candidate.id],
            ..Default::default()
        };
        seed_csb_group(
            state,
            ElectionConfig::EK27,
            sample_political_group_named(appellation),
            vec![candidate],
            vec![list],
        )
        .await
        .stream_id
    }

    /// The entry names of the ZIP archive `bytes`, sorted.
    async fn entry_names(bytes: Vec<u8>) -> Vec<String> {
        let mut names = zip_entry_names(Response::new(Body::from(bytes))).await;
        names.sort();
        names
    }

    #[tokio::test]
    async fn download_final_zip_streams_every_document() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        seed_group(&state, "Eerste").await;
        seed_group(&state, "Tweede").await;

        let response = download_final_zip(
            CsbFinalZipDownloadPath,
            CsbContext::new_test(),
            CsbMainStore::new_for_test(),
            State(state),
        )
        .await?;

        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers();
        assert_eq!(
            headers.get(header::CONTENT_TYPE).expect("content type"),
            "application/zip"
        );
        assert_eq!(
            headers
                .get(header::CONTENT_DISPOSITION)
                .expect("content disposition"),
            "attachment; filename=\"Afronding_Kandidaatstellen_ek27.zip\""
        );
        let mut names = zip_entry_names(response).await;
        names.sort();
        assert_eq!(
            names,
            [
                "Kanidatenlijsten_EK2027.zip",
                "Model_I4_Proces_Verbaal.docx",
                "Model_I4_Proces_Verbaal.pdf",
                "Overzicht_Kandidatenlijsten.docx",
                "Overzicht_Kandidatenlijsten.pdf",
                "Overzicht_Kandidatenlijsten.zip",
                "Publicatie_Kandidatenlijsten.docx",
                "Publicatie_Kandidatenlijsten.pdf",
                "Totaallijsten_EK2027.zip",
                "Verkiezingsdefinitie_EK2027.zip"
            ]
        );

        Ok(())
    }

    /// Every document in the final ZIP records the list order when none was
    /// recorded yet; together they record it once.
    #[tokio::test]
    async fn final_files_record_the_list_order_once() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let first = seed_group(&state, "Eerste").await;
        let second = seed_group(&state, "Tweede").await;
        let main_store = CsbMainStore::new_for_test();

        final_files(&CsbContext::new_test(), &main_store, &state).await?;
        assert_eq!(main_store.list_order_events(), 1);
        let mut recorded = main_store.list_order();
        recorded.sort();
        let mut expected = vec![first, second];
        expected.sort();
        assert_eq!(recorded, expected);

        final_files(&CsbContext::new_test(), &main_store, &state).await?;
        assert_eq!(main_store.list_order_events(), 1);

        Ok(())
    }

    #[tokio::test]
    async fn zip_contains_every_file() -> Result<(), AppError> {
        let files = vec![
            ("a.txt".to_string(), b"a".to_vec()),
            ("b.txt".to_string(), b"b".to_vec()),
        ];

        assert_eq!(entry_names(zip(&files).await?).await, ["a.txt", "b.txt"]);

        Ok(())
    }

    #[tokio::test]
    async fn zip_of_no_files_is_an_empty_archive() -> Result<(), AppError> {
        assert!(
            entry_names(zip(&Vec::with_capacity(0)).await?)
                .await
                .is_empty()
        );

        Ok(())
    }
}
