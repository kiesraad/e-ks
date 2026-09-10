use axum::{extract::State, http::HeaderValue, response::IntoResponse};

use crate::{
    AppError, AppRequestState, CsbMainStore,
    core::{ModelLocale, constants::DEFAULT_DATE_FORMAT},
    csb::examination::{
        model_inputs::{I4Inputs, i4_inputs},
        numbering::list_numbering,
        pages::{CsbI4DocxDownloadPath, CsbI4DownloadPath},
    },
    models::{
        Pdf,
        i4::{I4, NumberedOnDistricts, NumberedOnVotes},
    },
    utils::no_cache_headers,
};

const PDF_CONTENT_TYPE: &str = "application/pdf";
const DOCX_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

/// Collect the store data the I 4 model needs.
async fn i4_model<S: AppRequestState>(main_store: CsbMainStore, state: &S) -> Result<I4, AppError> {
    let election = main_store.election;
    let registry = state.csb_store_registry();
    let I4Inputs {
        found_omissions,
        recovered_omissions,
        invalid_lists,
        removed_candidates,
        removed_appellations,
        corrected_appellations,
        valid_lists,
    } = i4_inputs(registry, &election).await?;
    let numbering = list_numbering(registry, &main_store).await?;

    Ok(I4 {
        election_name: election.formal_title(ModelLocale::Nl),
        election_date: election
            .election_date()
            .format(DEFAULT_DATE_FORMAT)
            .to_string(),
        public_session: election.public_session().into(),
        found_omissions,
        recovered_omissions,
        invalid_lists,
        removed_candidates,
        removed_appellations,
        corrected_appellations,
        valid_lists,
        numbered_based_on_votes: numbering
            .on_votes()
            .map(|group| NumberedOnVotes {
                position: group.position,
                appellation: group.appellation.clone(),
                previous_votes: group.previous_votes.unwrap_or_default(),
            })
            .collect(),
        numbered_based_on_districts: numbering
            .by_lot()
            .map(|group| NumberedOnDistricts {
                position: group.position,
                appellation: group.appellation.clone(),
                districts: group.district_count as u64,
            })
            .collect(),
        // Objections are recorded during the public session.
        objections: None,
        response_objections: None,
    })
}

pub async fn gen_i4<S: AppRequestState>(
    _: CsbI4DownloadPath,
    main_store: CsbMainStore,
    State(state): State<S>,
) -> Result<impl IntoResponse, AppError> {
    let model = i4_model(main_store, &state).await?;
    let filename = model.filename();
    let bytes = model.generate_bytes().await?;

    let headers = no_cache_headers::generate_attachment_headers(
        &filename,
        HeaderValue::from_static(PDF_CONTENT_TYPE),
    )?;

    Ok((headers, bytes).into_response())
}

/// The same I 4 as [`gen_i4`], exported as a Word document.
pub async fn gen_i4_docx<S: AppRequestState>(
    _: CsbI4DocxDownloadPath,
    main_store: CsbMainStore,
    State(state): State<S>,
) -> Result<impl IntoResponse, AppError> {
    let model = i4_model(main_store, &state).await?;
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
        AppState, CsbAction, CsbMainAction, CsbUser, ElectionConfig, ElectoralDistrict,
        PgStoreData, StreamId,
        structs::{
            candidate_lists::CandidateList,
            csb::{
                OmissionCategory, OmissionStatus, sample_omission,
                sample_registered_political_group,
            },
            list_designation::ListDesignation,
            persons::PersonId,
            political_groups::PoliticalGroup,
        },
        test_utils::sample_person,
    };

    /// Import a group named `appellation` with one list in Groningen.
    async fn seed_group(state: &AppState, appellation: &str) -> StreamId {
        let stream_id = StreamId::new();
        let store = state
            .csb_store_for_stream(stream_id, ElectionConfig::EK27)
            .await
            .unwrap()
            .acting_as_test_user();
        let mut snapshot = PgStoreData {
            political_group: PoliticalGroup {
                appellation: Some(appellation.parse().unwrap()),
                list_designation: Some(ListDesignation::Standalone),
                ..Default::default()
            },
            ..PgStoreData::default()
        };
        let list = CandidateList {
            electoral_districts: vec![ElectoralDistrict::Groningen],
            ..Default::default()
        };
        snapshot.candidate_lists.insert(list.id, list);
        store
            .update(CsbAction::Import {
                hash: [0u8; 32],
                source_stream_id: StreamId::new(),
                snapshot: Box::new(snapshot),
            })
            .await
            .unwrap();
        stream_id
    }

    #[tokio::test]
    async fn gen_i4_returns_pdf_response() -> Result<(), AppError> {
        let main_store = CsbMainStore::new_for_test();
        let state = AppState::new_for_tests().await;
        let response = gen_i4(CsbI4DownloadPath, main_store, State(state))
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
            "attachment; filename=\"i4-proces-verbaal.pdf\""
        );
        assert_eq!(
            headers.get(header::CACHE_CONTROL).expect("cache control"),
            "no-store, no-cache, must-revalidate, max-age=0"
        );

        Ok(())
    }

    #[tokio::test]
    async fn gen_i4_docx_returns_word_response() -> Result<(), AppError> {
        let main_store = CsbMainStore::new_for_test();
        let state = AppState::new_for_tests().await;
        let response = gen_i4_docx(CsbI4DocxDownloadPath, main_store, State(state))
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
            "attachment; filename=\"i4-proces-verbaal.docx\""
        );

        // A .docx is a ZIP archive.
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        assert!(body.starts_with(b"PK"), "body is not a ZIP archive");

        Ok(())
    }

    /// The numbering sections follow the registered groups and the recorded
    /// lot order.
    #[tokio::test]
    async fn i4_model_numbers_the_lists_on_votes_and_by_lot() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let seated = seed_group(&state, "Gezeteld").await;
        let first_by_lot = seed_group(&state, "Eerste Loting").await;
        let second_by_lot = seed_group(&state, "Tweede Loting").await;

        let main_store = CsbMainStore::new_for_test();
        main_store
            .update(
                CsbMainAction::CreateRegisteredPoliticalGroup(sample_registered_political_group(
                    "Gezeteld", 1234, 3,
                ))
                .by(CsbUser::new_test()),
            )
            .await?;
        main_store
            .update(
                CsbMainAction::UpdateListOrder(vec![seated, second_by_lot, first_by_lot])
                    .by(CsbUser::new_test()),
            )
            .await?;

        let model = i4_model(main_store, &state).await?;

        let on_votes: Vec<_> = model
            .numbered_based_on_votes
            .iter()
            .map(|g| (g.position, g.appellation.as_str(), g.previous_votes))
            .collect();
        assert_eq!(on_votes, [(Some(1), "Gezeteld", 1234)]);
        let by_lot: Vec<_> = model
            .numbered_based_on_districts
            .iter()
            .map(|g| (g.position, g.appellation.as_str(), g.districts))
            .collect();
        assert_eq!(
            by_lot,
            [(Some(2), "Tweede Loting", 1), (Some(3), "Eerste Loting", 1)]
        );

        Ok(())
    }

    /// Render once with omissions, a scrapped candidate and a valid list.
    #[tokio::test]
    async fn gen_i4_renders_the_omissions_and_the_valid_lists() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let store = state
            .csb_store_for_stream(StreamId::new(), ElectionConfig::EK27)
            .await?
            .acting_as_test_user();

        let first = sample_person(PersonId::new());
        let second = sample_person(PersonId::new());
        let mut snapshot = PgStoreData {
            political_group: PoliticalGroup {
                appellation: Some("Kiesraad Demo".parse().unwrap()),
                list_designation: Some(ListDesignation::Standalone),
                ..Default::default()
            },
            ..PgStoreData::default()
        };
        snapshot.persons.insert(first.id, first.clone());
        snapshot.persons.insert(second.id, second.clone());
        let list = CandidateList {
            electoral_districts: vec![ElectoralDistrict::Groningen],
            candidates: vec![first.id, second.id],
            ..Default::default()
        };
        let list_id = list.id;
        snapshot.candidate_lists.insert(list.id, list);
        store
            .update(CsbAction::Import {
                hash: [0u8; 32],
                source_stream_id: StreamId::new(),
                snapshot: Box::new(snapshot),
            })
            .await?;

        let recovered = sample_omission(OmissionCategory::Candidate {
            person: first.id,
            lists: vec![list_id],
        });
        recovered.create(&store).await?;
        recovered
            .set_status(&store, OmissionStatus::Recovered)
            .await?;
        let not_recovered = sample_omission(OmissionCategory::Candidate {
            person: second.id,
            lists: vec![list_id],
        });
        not_recovered.create(&store).await?;
        not_recovered
            .set_status(&store, OmissionStatus::NotRecovered)
            .await?;

        let main_store = CsbMainStore::new_for_test();
        let response = gen_i4(CsbI4DownloadPath, main_store, State(state))
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
