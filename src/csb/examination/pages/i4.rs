use axum::{extract::State, http::HeaderValue, response::IntoResponse};

use crate::{
    AppError, AppRequestState, CsbMainStore,
    core::{ModelLocale, constants::DEFAULT_DATE_FORMAT},
    csb::examination::{
        model_inputs::{I4Inputs, i4_inputs},
        pages::CsbI4DownloadPath,
    },
    models::{Pdf, i4::I4},
    utils::no_cache_headers,
};

const PDF_CONTENT_TYPE: &str = "application/pdf";

pub async fn gen_i4<S: AppRequestState>(
    _: CsbI4DownloadPath,
    main_store: CsbMainStore,
    State(state): State<S>,
) -> Result<impl IntoResponse, AppError> {
    let election = main_store.election;
    let I4Inputs {
        found_omissions,
        recovered_omissions,
        invalid_lists,
        removed_candidates,
        removed_appellations,
        corrected_appellations,
        valid_lists,
    } = i4_inputs(state.csb_store_registry(), &election).await?;

    let model = I4 {
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
        numbered_based_on_votes: Vec::new(),
        numbered_based_on_districts: Vec::new(),
        // Numbering and objections are recorded during the public session.
        objections: None,
        response_objections: None,
    };
    let filename = model.filename();
    let bytes = model.generate_bytes().await?;

    let headers = no_cache_headers::generate_attachment_headers(
        &filename,
        HeaderValue::from_static(PDF_CONTENT_TYPE),
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
        AppState, CsbAction, ElectionConfig, ElectoralDistrict, PgStoreData, StreamId,
        structs::{
            candidate_lists::CandidateList,
            csb::{OmissionCategory, OmissionStatus, sample_omission},
            list_designation::ListDesignation,
            persons::PersonId,
            political_groups::PoliticalGroup,
        },
        test_utils::sample_person,
    };

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
