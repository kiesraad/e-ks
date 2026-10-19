use axum::{extract::State, response::Response};

use crate::{
    AppError, AppRequestState, CsbContext, CsbMainStore,
    core::{ModelLocale, constants::DEFAULT_DATE_FORMAT},
    csb::finalise::{
        numbering::list_numbering,
        paths::{CsbI4DraftDocxDownloadPath, CsbI4DraftDownloadPath},
    },
    models::{
        Pdf,
        csb_model_inputs::i4_inputs,
        i4::{I4, I4Status, NumberedOnDistricts, NumberedOnVotes},
        inputs::PublicSession,
    },
    structs::csb::HearingModel,
};

/// Collect the store data the I 4 model needs.
pub(in crate::csb::finalise::pages) async fn i4_model<S: AppRequestState>(
    context: &CsbContext,
    main_store: &CsbMainStore,
    state: &S,
    document_status: I4Status,
) -> Result<I4, AppError> {
    let election = main_store.election;
    let registry = state.csb_store_registry();
    let mut numbering = list_numbering(registry, main_store).await?;

    // A draft does not record the numbering
    if document_status.is_final() {
        numbering
            .record_if_incomplete(context, main_store)
            .await?;
    }

    let i4_inputs = i4_inputs(registry, &election, &numbering.stream_ids()).await?;

    let mut public_session = PublicSession::from(election.public_session());
    if let Some(hearing_details) = main_store.get_hearing_details(HearingModel::I4) {
        public_session = public_session.with_hearing_details(hearing_details);
    }

    Ok(I4 {
        election_name: election.formal_title(ModelLocale::Nl),
        election_date: election
            .election_date()
            .format(DEFAULT_DATE_FORMAT)
            .to_string(),
        public_session,
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
        objections: main_store
            .get_all_objections()
            .into_iter()
            .map(|o| o.objection_text.to_string())
            .collect(),
        response_objections: None,
        document_status,
        ..i4_inputs.into()
    })
}

pub async fn gen_i4_draft<S: AppRequestState>(
    _: CsbI4DraftDownloadPath,
    context: CsbContext,
    main_store: CsbMainStore,
    State(state): State<S>,
) -> Result<Response, AppError> {
    i4_model(&context, &main_store, &state, I4Status::Draft)
        .await?
        .pdf_response()
        .await
}

/// The same I 4 as [`gen_i4_draft`], exported as a Word document.
pub async fn gen_i4_draft_docx<S: AppRequestState>(
    _: CsbI4DraftDocxDownloadPath,
    context: CsbContext,
    main_store: CsbMainStore,
    State(state): State<S>,
) -> Result<Response, AppError> {
    i4_model(&context, &main_store, &state, I4Status::Draft)
        .await?
        .docx_response()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::DOCX_CONTENT_TYPE;
    use axum::{
        body::to_bytes,
        http::{StatusCode, header},
        response::IntoResponse,
    };
    use std::collections::BTreeSet;

    use crate::{
        AppState, CsbMainAction, CsbUser, ElectionConfig, ElectoralDistrict, StreamId,
        structs::{
            candidate_lists::CandidateList,
            csb::{
                HearingDetails, OmissionCategory, OmissionStatus, sample_omission,
                sample_registered_political_group,
            },
            persons::PersonId,
        },
        test_utils::{sample_person, sample_political_group_named, seed_csb_group},
    };

    fn empty_hash() -> String {
        String::with_capacity(0)
    }

    /// Import a group named `appellation` with one list in Groningen.
    async fn seed_group(state: &AppState, appellation: &str) -> StreamId {
        let list = CandidateList {
            electoral_districts: BTreeSet::from([ElectoralDistrict::Groningen]),
            ..Default::default()
        };
        seed_csb_group(
            state,
            ElectionConfig::EK27,
            sample_political_group_named(appellation),
            vec![],
            vec![list],
        )
        .await
        .stream_id
    }

    #[tokio::test]
    async fn i4_session_uses_the_stored_hearing_details() -> Result<(), AppError> {
        let context = CsbContext::new_test();
        let state = AppState::new_for_tests().await;
        let main_store = CsbMainStore::new_for_test();
        main_store
            .update(
                CsbMainAction::UpdateHearingDetails(
                    HearingModel::I4,
                    HearingDetails {
                        date_time: chrono::NaiveDate::from_ymd_opt(2027, 4, 9)
                            .expect("valid date")
                            .and_hms_opt(10, 15, 0)
                            .expect("valid time"),
                        chair: "Vera Voorzitter".to_string(),
                        members: vec!["Jan Klaassen".to_string()],
                    },
                )
                .by(CsbUser::new_test()),
            )
            .await?;

        let model = i4_model(&context, &main_store, &state, I4Status::Final(empty_hash())).await?;

        assert_eq!(model.public_session.date, "09-04-2027");
        assert_eq!(model.public_session.time, "10:15");
        assert_eq!(model.public_session.chair, "Vera Voorzitter");
        assert_eq!(model.public_session.members, vec!["Jan Klaassen"]);

        Ok(())
    }

    /// The I 1 hearing is a different hearing: it must not leak into the I 4.
    #[tokio::test]
    async fn i4_session_ignores_the_i1_hearing_details() -> Result<(), AppError> {
        let context = CsbContext::new_test();
        let state = AppState::new_for_tests().await;
        let main_store = CsbMainStore::new_for_test();
        main_store
            .update(
                CsbMainAction::UpdateHearingDetails(
                    HearingModel::I1,
                    HearingDetails {
                        chair: "Vera Voorzitter".to_string(),
                        members: vec!["Jan Klaassen".to_string()],
                        ..HearingDetails::default()
                    },
                )
                .by(CsbUser::new_test()),
            )
            .await?;

        let model = i4_model(&context, &main_store, &state, I4Status::Final(empty_hash())).await?;

        let configured = ElectionConfig::EK27.public_session();
        assert_eq!(model.public_session.date, configured.formatted_date());
        assert_eq!(model.public_session.chair, configured.chair);

        Ok(())
    }

    #[tokio::test]
    async fn gen_i4_returns_pdf_response() -> Result<(), AppError> {
        let context = CsbContext::new_test();
        let main_store = CsbMainStore::new_for_test();
        let state = AppState::new_for_tests().await;
        let response = gen_i4_draft(CsbI4DraftDownloadPath, context, main_store, State(state))
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
            "attachment; filename=\"Model_I4_Proces_Verbaal_Concept.pdf\""
        );
        assert_eq!(
            headers.get(header::CACHE_CONTROL).expect("cache control"),
            "no-store, no-cache, must-revalidate, max-age=0"
        );

        Ok(())
    }

    #[tokio::test]
    async fn gen_i4_docx_returns_word_response() -> Result<(), AppError> {
        let context = CsbContext::new_test();
        let main_store = CsbMainStore::new_for_test();
        let state = AppState::new_for_tests().await;
        let response = gen_i4_draft_docx(
            CsbI4DraftDocxDownloadPath,
            context,
            main_store,
            State(state),
        )
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
            "attachment; filename=\"Model_I4_Proces_Verbaal_Concept.docx\""
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
        let context = CsbContext::new_test();
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

        let model = i4_model(&context, &main_store, &state, I4Status::Final(empty_hash())).await?;

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

    /// The by-lot table's positions and appellations.
    fn by_lot(model: &I4) -> Vec<(Option<usize>, &str)> {
        model
            .numbered_based_on_districts
            .iter()
            .map(|g| (g.position, g.appellation.as_str()))
            .collect()
    }

    /// The first final download records the starting order when none was
    /// recorded yet, and numbers the lists by it; further downloads leave it
    /// alone.
    #[tokio::test]
    async fn i4_model_records_the_list_order_once() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        // Created in reverse, so the alphabetical starting order differs
        let second = seed_group(&state, "Tweede").await;
        let first = seed_group(&state, "Eerste").await;
        let main_store = CsbMainStore::new_for_test();
        assert_eq!(main_store.list_order_events(), 0);

        let numbered = [(Some(1), "Eerste"), (Some(2), "Tweede")];
        let model = i4_model(
            &CsbContext::new_test(),
            &main_store.clone(),
            &state,
            I4Status::Final(empty_hash()),
        )
        .await?;
        assert_eq!(by_lot(&model), numbered);
        assert_eq!(main_store.list_order_events(), 1);
        assert_eq!(main_store.list_order(), [first, second]);

        let model = i4_model(
            &CsbContext::new_test(),
            &main_store.clone(),
            &state,
            I4Status::Final(empty_hash()),
        )
        .await?;
        assert_eq!(by_lot(&model), numbered);
        let model = i4_model(
            &CsbContext::new_test(),
            &main_store.clone(),
            &state,
            I4Status::Draft,
        )
        .await?;
        assert_eq!(by_lot(&model), numbered);
        assert_eq!(main_store.list_order_events(), 1);

        Ok(())
    }

    /// A draft never records the list order, not even when none was recorded,
    /// and leaves the by-lot positions blank.
    #[tokio::test]
    async fn i4_draft_does_not_record_the_list_order() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        seed_group(&state, "Eerste").await;
        seed_group(&state, "Tweede").await;
        let main_store = CsbMainStore::new_for_test();

        i4_model(
            &CsbContext::new_test(),
            &main_store.clone(),
            &state,
            I4Status::Draft,
        )
        .await?;
        let model = i4_model(
            &CsbContext::new_test(),
            &main_store.clone(),
            &state,
            I4Status::Draft,
        )
        .await?;

        assert_eq!(by_lot(&model), [(None, "Eerste"), (None, "Tweede")]);
        assert_eq!(main_store.list_order_events(), 0);
        assert!(main_store.list_order().is_empty());

        Ok(())
    }

    /// Render once with omissions, a scrapped candidate and a valid list.
    #[tokio::test]
    async fn gen_i4_renders_the_omissions_and_the_valid_lists() -> Result<(), AppError> {
        let context = CsbContext::new_test();
        let state = AppState::new_for_tests().await;
        let first = sample_person(PersonId::new());
        let second = sample_person(PersonId::new());
        let list = CandidateList {
            electoral_districts: BTreeSet::from([ElectoralDistrict::Groningen]),
            candidates: vec![first.id, second.id],
            ..Default::default()
        };
        let list_id = list.id;
        let store = seed_csb_group(
            &state,
            ElectionConfig::EK27,
            sample_political_group_named("Kiesraad Demo"),
            vec![first.clone(), second.clone()],
            vec![list],
        )
        .await;

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
        let response = gen_i4_draft(CsbI4DraftDownloadPath, context, main_store, State(state))
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
