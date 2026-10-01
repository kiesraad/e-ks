use axum::{extract::State, response::Response};

use crate::{
    AppError, AppRequestState, CsbMainStore,
    core::{ModelLocale, constants::DEFAULT_DATE_FORMAT},
    csb::examination::{
        model_inputs::published_lists,
        numbering::list_numbering,
        pages::{CsbOsv3_2DocxDownloadPath, CsbOsv3_2DownloadPath},
    },
    models::{Pdf, osv3_2::OSV3_2},
};

/// Collect the store data the OSV 3-2 model needs.
async fn osv3_2_model<S: AppRequestState>(
    main_store: CsbMainStore,
    state: &S,
) -> Result<OSV3_2, AppError> {
    let election = main_store.election;
    let registry = state.csb_store_registry();
    let numbering = list_numbering(registry, &main_store).await?;

    Ok(OSV3_2 {
        election_name: election.formal_title(ModelLocale::Nl),
        election_date: election
            .election_date()
            .format(DEFAULT_DATE_FORMAT)
            .to_string(),
        valid_lists: published_lists(registry, &election, &numbering).await?,
    })
}

/// The publication of the candidate lists, as PDF.
pub async fn gen_osv3_2<S: AppRequestState>(
    _: CsbOsv3_2DownloadPath,
    main_store: CsbMainStore,
    State(state): State<S>,
) -> Result<Response, AppError> {
    osv3_2_model(main_store, &state).await?.pdf_response().await
}

/// The same publication as [`gen_osv3_2`], exported as a Word document.
pub async fn gen_osv3_2_docx<S: AppRequestState>(
    _: CsbOsv3_2DocxDownloadPath,
    main_store: CsbMainStore,
    State(state): State<S>,
) -> Result<Response, AppError> {
    osv3_2_model(main_store, &state)
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
        AppState, CsbAction, CsbMainAction, CsbUser, ElectionConfig, ElectoralDistrict,
        PgStoreData, StreamId,
        structs::{
            candidate_lists::CandidateList,
            csb::{OmissionCategory, OmissionStatus, sample_omission},
            list_designation::ListDesignation,
            political_groups::PoliticalGroup,
        },
    };

    /// Import a group named `appellation` with one list in `districts`. The
    /// list has no candidates: these tests are about which lists appear
    /// where, not about their content.
    async fn seed_group(
        state: &AppState,
        appellation: &str,
        districts: &[ElectoralDistrict],
    ) -> StreamId {
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
            electoral_districts: districts.iter().copied().collect::<BTreeSet<_>>(),
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

    /// A main store with `order` recorded as the list order.
    async fn main_store_with_order(order: Vec<StreamId>) -> Result<CsbMainStore, AppError> {
        let main_store = CsbMainStore::new_for_test();
        main_store
            .update(CsbMainAction::UpdateListOrder(order).by(CsbUser::new_test()))
            .await?;
        Ok(main_store)
    }

    /// Per district, each list's number and appellation.
    fn rows(model: &OSV3_2) -> Vec<(&str, Vec<(usize, &str)>)> {
        model
            .valid_lists
            .iter()
            .map(|district| {
                let lists = district
                    .lists
                    .iter()
                    .map(|numbered| (numbered.number, numbered.list.appellation.as_str()))
                    .collect();
                (district.electoral_district.as_str(), lists)
            })
            .collect()
    }

    /// Per district the lists follow the recorded order; a group only
    /// appears in the districts it has a list in, and the numbers there run
    /// on without a gap.
    #[tokio::test]
    async fn osv3_2_model_orders_the_lists_per_district_by_number() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let both = seed_group(
            &state,
            "Overal",
            &[ElectoralDistrict::Groningen, ElectoralDistrict::Drenthe],
        )
        .await;
        let groningen =
            seed_group(&state, "Alleen Groningen", &[ElectoralDistrict::Groningen]).await;
        let main_store = main_store_with_order(vec![groningen, both]).await?;

        let model = osv3_2_model(main_store, &state).await?;

        assert_eq!(
            rows(&model),
            [
                ("Groningen", vec![(1, "Alleen Groningen"), (2, "Overal")]),
                ("Drenthe", vec![(1, "Overal")]),
            ]
        );
        assert_eq!(model.election_date, "24-05-2027");

        Ok(())
    }

    /// A list scrapped in one district drops out of that district only; the
    /// lists after it there move up a number, elsewhere they keep theirs.
    #[tokio::test]
    async fn osv3_2_model_numbers_on_past_a_list_scrapped_in_one_district() -> Result<(), AppError>
    {
        let state = AppState::new_for_tests().await;
        let districts = [ElectoralDistrict::Groningen, ElectoralDistrict::Drenthe];
        let first = seed_group(&state, "Eerste", &districts).await;
        let scrapped = seed_group(&state, "Geschrapt", &districts).await;
        let third = seed_group(&state, "Derde", &districts).await;
        let main_store = main_store_with_order(vec![first, scrapped, third]).await?;

        // Too few declarations of support in Drenthe scraps the list there.
        let store = state
            .csb_store_for_stream(scrapped, ElectionConfig::EK27)
            .await?
            .acting_as_test_user();
        let omission = sample_omission(OmissionCategory::DeclarationsOfSupport(vec![
            ElectoralDistrict::Drenthe,
        ]));
        omission.create(&store).await?;
        omission
            .set_status(&store, OmissionStatus::NotRecovered)
            .await?;

        let model = osv3_2_model(main_store, &state).await?;

        assert_eq!(
            rows(&model),
            [
                (
                    "Groningen",
                    vec![(1, "Eerste"), (2, "Geschrapt"), (3, "Derde")]
                ),
                ("Drenthe", vec![(1, "Eerste"), (2, "Derde")]),
            ]
        );

        Ok(())
    }

    /// Without a recorded order the lists numbered by lot have no number yet
    #[tokio::test]
    async fn osv3_2_model_requires_the_recorded_order() {
        let state = AppState::new_for_tests().await;
        seed_group(&state, "Ongenummerd", &[ElectoralDistrict::Groningen]).await;

        let result = osv3_2_model(CsbMainStore::new_for_test(), &state).await;

        assert!(matches!(result, Err(AppError::IncompleteData(_))));
    }

    #[tokio::test]
    async fn gen_osv3_2_returns_pdf_response() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let stream_id = seed_group(&state, "Kiesraad Demo", &[ElectoralDistrict::Groningen]).await;
        let main_store = main_store_with_order(vec![stream_id]).await?;

        let response = gen_osv3_2(CsbOsv3_2DownloadPath, main_store, State(state))
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
            "attachment; filename=\"OSV_3-2_publicatie_kandidatenlijsten.pdf\""
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
    async fn gen_osv3_2_docx_returns_word_response() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let response = gen_osv3_2_docx(
            CsbOsv3_2DocxDownloadPath,
            CsbMainStore::new_for_test(),
            State(state),
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
            "attachment; filename=\"OSV_3-2_publicatie_kandidatenlijsten.docx\""
        );
        // A .docx is a ZIP archive.
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        assert!(body.starts_with(b"PK"), "body is not a ZIP archive");

        Ok(())
    }
}
