use std::collections::HashSet;

use askama::Template;
use axum::{
    body::Body,
    extract::State,
    http::HeaderValue,
    response::{IntoResponse, Response},
};
use tokio::io::{DuplexStream, duplex};
use tokio_util::io::ReaderStream;

use crate::{
    AppError, AppRequestState, Context, CsbContext, CsbMainStore, CsbStore, CsbStream,
    HtmlTemplate,
    core::{ModelLocale, ZipResponseWriter},
    csb::examination::{
        extractors::CsbPoliticalGroup,
        model_inputs::omission_letter_sections,
        pages::{
            CsbFinishExaminationPath, CsbOmissionLetterDocxDownloadPath,
            CsbOmissionLetterDownloadPath, CsbOmissionLetterPath, CsbOmissionLettersDownloadPath,
        },
        structs::AllOmissions,
    },
    filters,
    models::{Pdf, documents::ZIP_CONTENT_TYPE, inputs::Person, omission_letter::OmissionLetter},
    projection::WithCorrections,
    utils::no_cache_headers,
};

const PDF_CONTENT_TYPE: &str = "application/pdf";
const DOCX_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

/// The recovery period closes at 17:00 on its last day.
// TODO: move to the election configuration; the deadlines there are plain dates.
const RECOVERY_DEADLINE_TIME: &str = "17:00";

#[derive(Template)]
#[template(path = "csb/examination/pages/omission_letter.html")]
struct CsbOmissionLetterTemplate {
    political_group: CsbPoliticalGroup,
    all_omissions: AllOmissions,
    omission_count: usize,
}

/// The omission letter page of one political group: the letter's downloads
/// and, read-only, every omission that goes into it.
pub async fn overview(
    _: CsbOmissionLetterPath,
    context: CsbContext,
    store: CsbStore,
) -> Result<Response, AppError> {
    let political_group = CsbPoliticalGroup::new_from_csb_store(&store);
    let all_omissions = store.get_all_omissions(&political_group)?;

    Ok(HtmlTemplate(
        CsbOmissionLetterTemplate {
            omission_count: store.get_omission_count(),
            political_group,
            all_omissions,
        },
        context,
    )
    .into_response())
}

/// Collect the store data the omission letter ("verzuimbrief") of one
/// political group needs.
fn omission_letter_model(store: &CsbStream) -> Result<OmissionLetter, AppError> {
    let election = store.election;
    let session = election.public_session();

    Ok(OmissionLetter {
        election_name: election.formal_title(ModelLocale::Nl),
        location: session.location.to_string(),
        // Sent on the day of the examination ("vergadering van heden").
        date: election.document_review_date(),
        addressee: Person::from(store.get_list_submitter(WithCorrections::All)),
        appellation: store.get_appellation(WithCorrections::All),
        election_code: election.filename_slug(),
        omission_groups: omission_letter_sections(store, &election)?,
        recovery_deadline_date: election.omission_period_end_date(),
        recovery_deadline_time: RECOVERY_DEADLINE_TIME.to_string(),
        // TODO: use the committee's street address once configured.
        recovery_address: format!(
            "het secretariaat van het centraal stembureau te {}",
            session.location
        ),
        // Blank: signed by hand, as the models are.
        chair: String::new(),
        secretary: String::new(),
        // TODO: fill appendix 1 once the store counts declarations of support
        // per district; the letter drops it while empty.
        declarations_of_support: Vec::new(),
    })
}

/// The omission letter for one political group, as PDF.
pub async fn gen_omission_letter(
    _: CsbOmissionLetterDownloadPath,
    store: CsbStore,
) -> Result<impl IntoResponse, AppError> {
    let model = omission_letter_model(&store)?;
    let filename = model.filename();
    let bytes = model.generate_bytes().await?;

    let headers = no_cache_headers::generate_attachment_headers(
        &filename,
        HeaderValue::from_static(PDF_CONTENT_TYPE),
    )?;

    Ok((headers, bytes).into_response())
}

/// The same letter as [`gen_omission_letter`], exported as a Word document.
pub async fn gen_omission_letter_docx(
    _: CsbOmissionLetterDocxDownloadPath,
    store: CsbStore,
) -> Result<impl IntoResponse, AppError> {
    let model = omission_letter_model(&store)?;
    let filename = model.docx_filename();
    let bytes = model.generate_docx_bytes().await?;

    let headers = no_cache_headers::generate_attachment_headers(
        &filename,
        HeaderValue::from_static(DOCX_CONTENT_TYPE),
    )?;

    Ok((headers, bytes).into_response())
}

/// Every omission letter of the election, as PDF and Word, in one ZIP: the
/// letters of the finished groups with omissions, as the finish page lists
/// them. The archive is streamed while the letters render one at a time, so
/// only one letter is held in memory and the download starts at once.
pub async fn gen_omission_letters_zip<S: AppRequestState>(
    _: CsbOmissionLettersDownloadPath,
    main_store: CsbMainStore,
    State(state): State<S>,
) -> Result<Response, AppError> {
    let election = main_store.election;

    // Built up front, so their errors surface before the response streams.
    let mut letters = Vec::new();
    for store in state
        .csb_store_registry()
        .stores_for_election(election)
        .await?
    {
        if store.is_deleted() || !store.is_examination_finished() || store.get_omission_count() == 0
        {
            continue;
        }
        letters.push(omission_letter_model(&store)?);
    }

    let filename = format!("verzuimbrieven-{}.zip", election.filename_slug());
    let headers = no_cache_headers::generate_attachment_headers(
        &filename,
        HeaderValue::from_static(ZIP_CONTENT_TYPE),
    )?;

    let (reader, writer) = duplex(64 * 1024);
    let body = Body::from_stream(ReaderStream::new(reader));

    tokio::spawn(async move {
        if let Err(err) = write_letters_zip(letters, writer).await {
            tracing::error!(error = ?err, "failed to stream omission letters zip");
        }
    });

    Ok((headers, body).into_response())
}

/// Render each letter in turn and add its PDF and Word file to the archive.
async fn write_letters_zip(
    letters: Vec<OmissionLetter>,
    writer: DuplexStream,
) -> Result<(), AppError> {
    let mut zipper = ZipResponseWriter::new(writer);
    let mut stems = HashSet::new();

    for letter in letters {
        let stem = unique_stem(&mut stems, &letter.filename());
        zipper
            .add_file(&format!("{stem}.pdf"), &letter.generate_bytes().await?)
            .await?;
        zipper
            .add_file(
                &format!("{stem}.docx"),
                &letter.generate_docx_bytes().await?,
            )
            .await?;
    }

    zipper.finish().await
}

/// The entry name without extension, made unique within the archive: groups
/// can share a slug (two blank lists are both `verzuimbrief`).
fn unique_stem(used: &mut HashSet<String>, pdf_filename: &str) -> String {
    let stem = pdf_filename.strip_suffix(".pdf").unwrap_or(pdf_filename);
    let mut candidate = stem.to_string();
    let mut n = 2;
    while !used.insert(candidate.clone()) {
        candidate = format!("{stem}-{n}");
        n += 1;
    }
    candidate
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
        AppState, CsbAction, ElectionConfig, ElectoralDistrict, PgStoreData, StreamId,
        structs::{
            csb::{Omission, OmissionCategory, sample_omission},
            list_designation::ListDesignation,
            list_submitters::ListSubmitterId,
            political_groups::PoliticalGroup,
        },
        test_utils::{response_body_string, sample_list_submitter, zip_entry_names},
    };

    /// The import snapshot of a group named `appellation`, with a list
    /// submitter to address the letter to.
    fn import_action(appellation: &str) -> CsbAction {
        CsbAction::Import {
            hash: [0u8; 32],
            source_stream_id: StreamId::new(),
            snapshot: Box::new(PgStoreData {
                political_group: PoliticalGroup {
                    appellation: Some(appellation.parse().unwrap()),
                    list_designation: Some(ListDesignation::Standalone),
                    ..Default::default()
                },
                list_submitter: sample_list_submitter(ListSubmitterId::new()),
                ..PgStoreData::default()
            }),
        }
    }

    /// Seed an imported group in the registry of `state`, optionally with an
    /// omission and a finished examination.
    async fn seed_group(
        state: &AppState,
        appellation: &str,
        finished: bool,
        with_omission: bool,
    ) -> Result<(), AppError> {
        let store = state
            .csb_store_for_stream(StreamId::new(), ElectionConfig::EK27)
            .await?
            .acting_as_test_user();
        store.update(import_action(appellation)).await?;
        if with_omission {
            sample_omission(OmissionCategory::PoliticalGroup)
                .create(&store)
                .await?;
        }
        if finished {
            store.update(CsbAction::SetFinished(true)).await?;
        }
        Ok(())
    }

    /// An imported group with an appellation and a list submitter to address.
    async fn store_with_group() -> CsbStore {
        let store = CsbStore::new_for_test();
        store
            .update(import_action("Kiesraad Demo"))
            .await
            .expect("import the political group");
        store
    }

    #[tokio::test]
    async fn gen_omission_letter_returns_a_pdf_named_after_the_group() -> Result<(), AppError> {
        let store = store_with_group().await;
        sample_omission(OmissionCategory::PoliticalGroup)
            .create(&store)
            .await?;
        sample_omission(OmissionCategory::DeclarationsOfSupport(vec![
            ElectoralDistrict::Bonaire,
        ]))
        .create(&store)
        .await?;

        let response = gen_omission_letter(
            CsbOmissionLetterDownloadPath {
                stream_id: store.stream_id,
            },
            store,
        )
        .await?
        .into_response();

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
            "attachment; filename=\"verzuimbrief-kiesraad-demo-ek27.pdf\""
        );
        assert_eq!(
            headers.get(header::CACHE_CONTROL).expect("cache control"),
            "no-store, no-cache, must-revalidate, max-age=0"
        );

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        assert!(body.starts_with(b"%PDF"), "body is not a PDF");

        Ok(())
    }

    #[tokio::test]
    async fn gen_omission_letter_docx_returns_a_word_document() -> Result<(), AppError> {
        let store = store_with_group().await;
        sample_omission(OmissionCategory::PoliticalGroup)
            .create(&store)
            .await?;

        let response = gen_omission_letter_docx(
            CsbOmissionLetterDocxDownloadPath {
                stream_id: store.stream_id,
            },
            store,
        )
        .await?
        .into_response();

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
            "attachment; filename=\"verzuimbrief-kiesraad-demo-ek27.docx\""
        );

        // A .docx is a ZIP archive.
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        assert!(body.starts_with(b"PK"), "body is not a ZIP archive");

        Ok(())
    }

    /// The page lists every omission read-only and links both downloads.
    #[tokio::test]
    async fn overview_lists_the_omissions_and_links_the_downloads() -> Result<(), AppError> {
        let store = store_with_group().await;
        let stream_id = store.stream_id;
        Omission::new(
            OmissionCategory::PoliticalGroup,
            "Deposit missing".parse().unwrap(),
            "The deposit has not been paid.".parse().unwrap(),
            Some("Pay the deposit at the secretariat.".parse().unwrap()),
        )
        .create(&store)
        .await?;
        Omission::new(
            OmissionCategory::DeclarationsOfSupport(vec![
                ElectoralDistrict::Groningen,
                ElectoralDistrict::Fryslan,
            ]),
            "Declarations of support missing".parse().unwrap(),
            "Too few declarations of support were handed in."
                .parse()
                .unwrap(),
            None,
        )
        .create(&store)
        .await?;

        let response = overview(
            CsbOmissionLetterPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await?
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;

        assert!(body.contains("Kiesraad Demo"));
        assert!(body.contains(&format!(
            r#"href="/csb/examination/{stream_id}/verzuimbrief.pdf""#
        )));
        assert!(body.contains(&format!(
            r#"href="/csb/examination/{stream_id}/verzuimbrief.docx""#
        )));
        assert!(body.contains(r#"href="/csb/examination/finish""#));

        // Both omissions, with the districts the second one applies to and
        // the letter note of the first; the second has none.
        assert!(body.contains("Deposit missing"));
        assert!(body.contains("The deposit has not been paid."));
        assert!(body.contains("Pay the deposit at the secretariat."));
        assert_eq!(body.matches("Note in omission letter").count(), 1);
        assert!(body.contains("Declarations of support missing"));
        assert!(body.contains("1. Groningen"));
        assert!(body.contains("2. Frysl"));
        assert!(!body.contains("Utrecht"));

        // Read-only: no recovery decisions and no omission editing overlays.
        assert!(!body.contains(r#"value="recovered""#));
        assert!(!body.contains("omission-status-control"));
        assert!(!body.contains("/overview?"));

        Ok(())
    }

    #[tokio::test]
    async fn overview_renders_empty_state_without_omissions() -> Result<(), AppError> {
        let store = store_with_group().await;
        let stream_id = store.stream_id;

        let response = overview(
            CsbOmissionLetterPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await?
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("No omissions have been added yet."));

        Ok(())
    }

    /// The archive holds the PDF and Word letter of every finished group with
    /// omissions and nothing of the other groups; equal names are numbered.
    #[tokio::test]
    async fn gen_omission_letters_zip_streams_a_letter_per_finished_group() -> Result<(), AppError>
    {
        let state = AppState::new_for_tests().await;
        seed_group(&state, "Kiesraad Demo", true, true).await?;
        seed_group(&state, "Kiesraad Demo", true, true).await?;
        seed_group(&state, "Nog Bezig", false, true).await?;
        seed_group(&state, "Zonder Verzuim", true, false).await?;

        let response = gen_omission_letters_zip(
            CsbOmissionLettersDownloadPath,
            CsbMainStore::new_for_test(),
            State(state),
        )
        .await?
        .into_response();

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
            "attachment; filename=\"verzuimbrieven-ek27.zip\""
        );

        let mut names = zip_entry_names(response).await;
        names.sort();
        assert_eq!(
            names,
            [
                "verzuimbrief-kiesraad-demo-ek27-2.docx",
                "verzuimbrief-kiesraad-demo-ek27-2.pdf",
                "verzuimbrief-kiesraad-demo-ek27.docx",
                "verzuimbrief-kiesraad-demo-ek27.pdf",
            ]
        );

        Ok(())
    }

    /// Without omissions the letter reports that none were found.
    #[tokio::test]
    async fn gen_omission_letter_renders_without_omissions() -> Result<(), AppError> {
        let store = store_with_group().await;

        let response = gen_omission_letter(
            CsbOmissionLetterDownloadPath {
                stream_id: store.stream_id,
            },
            store,
        )
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
