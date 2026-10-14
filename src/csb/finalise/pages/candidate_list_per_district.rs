use axum::{
    body::Body,
    extract::State,
    http::HeaderValue,
    response::{IntoResponse, Response},
};
use tokio::io::{DuplexStream, duplex};
use tokio_util::io::ReaderStream;

use crate::{
    AppError, AppRequestState, CsbContext, CsbMainStore,
    constants::DEFAULT_DATE_FORMAT,
    core::{ModelLocale, ZipResponseWriter},
    csb::finalise::{CsbCandidateListPerDistrictDownloadPath, numbering::list_numbering},
    models::{
        Pdf, candidate_list_per_district::CandidateListPerDistrict,
        csb_model_inputs::published_lists, documents::ZIP_CONTENT_TYPE,
    },
    utils::no_cache_headers,
};

pub async fn download_zip<S: AppRequestState>(
    _: CsbCandidateListPerDistrictDownloadPath,
    main_store: CsbMainStore,
    context: CsbContext,
    State(state): State<S>,
) -> Result<Response, AppError> {
    let models = models(main_store, &state, context).await?;
    let mut docs = Vec::new();
    for m in &models {
        docs.push((m.filename(), m.generate_bytes().await?));
        docs.push((m.docx_filename(), m.generate_docx_bytes().await?));
    }

    let headers = no_cache_headers::generate_attachment_headers(
        "overzicht-kandidatenlijsten.zip",
        HeaderValue::from_static(ZIP_CONTENT_TYPE),
    )?;

    let (reader, writer) = duplex(64 * 1024);
    let body = Body::from_stream(ReaderStream::new(reader));

    tokio::spawn(async move {
        if let Err(err) = write_list_zip(docs, writer).await {
            tracing::error!(error = ?err, "failed to stream candidate list zip");
        }
    });

    Ok((headers, body).into_response())
}

async fn models<S: AppRequestState>(
    store: CsbMainStore,
    state: &S,
    context: CsbContext,
) -> Result<Vec<CandidateListPerDistrict>, AppError> {
    let election = store.election;
    let registry = state.csb_store_registry();
    let mut models = Vec::new();

    let mut numbering = list_numbering(registry, &store).await?;
    let stream_ids = numbering.stream_ids();

    numbering.record_if_incomplete(&context, &store).await?;

    let published_lists = published_lists(registry, &election, &stream_ids).await?;

    for district_list in published_lists {
        models.push(CandidateListPerDistrict {
            election_name: election.formal_title(ModelLocale::Nl),
            election_date: election
                .election_date()
                .format(DEFAULT_DATE_FORMAT)
                .to_string(),
            electoral_district: district_list.electoral_district,
            lists: district_list.lists,
        });
    }
    Ok(models)
}

async fn write_list_zip(
    files: Vec<(String, Vec<u8>)>,
    writer: DuplexStream,
) -> Result<(), AppError> {
    let mut zipper = ZipResponseWriter::new(writer);

    for (name, bytes) in files {
        zipper.add_file(&name, &bytes).await?;
    }

    zipper.finish().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::to_bytes, extract::State};
    use reqwest::{StatusCode, header};

    use crate::{
        AppState, CsbContext, CsbMainStore,
        csb::finalise::{
            CsbCandidateListPerDistrictDownloadPath,
            pages::candidate_list_per_district::download_zip,
        },
    };

    #[tokio::test]
    async fn endpoint_produces_zip() {
        let store = CsbMainStore::new_for_test();
        let context = CsbContext::new_test();
        let state = AppState::new_for_tests().await;

        let response = download_zip(
            CsbCandidateListPerDistrictDownloadPath,
            store,
            context,
            State(state),
        )
        .await
        .expect("response from zip endpoint");

        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers().clone();
        assert_eq!(
            headers.get(header::CONTENT_TYPE).expect("content type"),
            "application/zip"
        );
        assert_eq!(
            headers
                .get(header::CONTENT_DISPOSITION)
                .expect("content disposition"),
            "attachment; filename=\"overzicht-kandidatenlijsten.zip\""
        );
        assert_eq!(
            headers.get(header::CACHE_CONTROL).expect("cache control"),
            "no-store, no-cache, must-revalidate, max-age=0"
        );
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert!(body.starts_with(b"PK"), "body is not a ZIP archive");
    }

    #[tokio::test]
    async fn models_generates_model_foreach_district() {
        let store = CsbMainStore::new_for_test();
        let context = CsbContext::new_test();
        let state = AppState::new_for_tests().await;

        let models = models(store.clone(), &state, context)
            .await
            .expect("models");
        assert_eq!(store.election.electoral_districts().len(), models.len())
    }
}
