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
    models::{Pdf, documents::ZIP_CONTENT_TYPE},
    utils::no_cache_headers,
};

mod candidate_list_overview;
mod candidate_list_per_district;
mod candidate_list_publication;
// mod election_definition;
mod eml;

pub(self) type Files = Vec<(String, Vec<u8>)>;

/// All finalisation documents in one ZIP available on the CSB finalise page
pub async fn download_final_zip<S: AppRequestState>(
    _: CsbFinalZipDownloadPath,
    context: CsbContext,
    main_store: CsbMainStore,
    State(state): State<S>,
) -> Result<Response, AppError> {
    let files = final_files(&context, &main_store, &state).await?;
    let filename = format!(
        "afronding-kandidaatstellen-{}.zip",
        main_store.election.filename_slug()
    );

    zip_response(&filename, files)
}

/// The filenames of the final documents, along with the raw bytes that represent them
async fn final_files<S: AppRequestState>(
    context: &CsbContext,
    main_store: &CsbMainStore,
    state: &S,
) -> Result<Vec<(String, Vec<u8>)>, AppError> {
    let election_id = ElectionId::try_from(main_store.election)?;

    let mut lists_per_district = Vec::new();
    for model in candidate_list_per_district_model(context, main_store, state).await? {
        lists_per_district.extend(pdf_and_docx(&model).await?);
    }
    let eml230 = eml230_files(context, state.csb_store_registry(), main_store).await?;
    let eml110a = eml110a_files(main_store)?;

    let mut files = vec![
        (
            "Overzicht_Kandidatenlijsten.zip".to_string(),
            zip(&lists_per_district).await?,
        ),
        (
            format!("eml-{}.zip", main_store.election.filename_slug()),
            zip(&eml230).await?,
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
    files.extend(pdf_and_docx(&i4_model(context, main_store, state, false).await?).await?);

    Ok(files)
}

/// `model` rendered as PDF and as Word document
async fn pdf_and_docx(model: &impl Pdf) -> Result<[(String, Vec<u8>); 2], AppError> {
    Ok([
        (model.filename(), model.generate_bytes().await?),
        (model.docx_filename(), model.generate_docx_bytes().await?),
    ])
}

fn zip_response(filename: &str, files: Vec<(String, Vec<u8>)>) -> Result<Response, AppError> {
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
