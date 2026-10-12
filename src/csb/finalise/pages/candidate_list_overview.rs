use axum::{extract::State, response::Response};

use crate::{
    AppError, AppRequestState, CsbContext, CsbMainStore,
    constants::DEFAULT_DATE_FORMAT,
    core::ModelLocale,
    csb::finalise::{
        numbering::list_numbering,
        paths::{CsbCandidateListOverviewDocxDownloadPath, CsbCandidateListOverviewDownloadPath},
    },
    models::{
        Pdf, candidate_list_overview::CandidateListOverview, csb_model_inputs::lists_overview,
    },
};

async fn candidate_list_overview_model<S: AppRequestState>(
    context: CsbContext,
    main_store: CsbMainStore,
    state: &S,
) -> Result<CandidateListOverview, AppError> {
    let election = main_store.election;
    let registry = state.csb_store_registry();
    let numbering = list_numbering(registry, &main_store).await?;

    numbering
        .record_if_incomplete(&context, &main_store)
        .await?;

    Ok(CandidateListOverview {
        election_name: election.formal_title(ModelLocale::Nl),
        election_date: election
            .election_date()
            .format(DEFAULT_DATE_FORMAT)
            .to_string(),
        electoral_districts: election.electoral_districts().to_vec(),
        groups: lists_overview(registry, &election, &numbering.stream_ids()).await?,
    })
}

/// The overview of the candidate lists, as PDF
pub async fn gen_candidate_list_overview<S: AppRequestState>(
    _: CsbCandidateListOverviewDownloadPath,
    context: CsbContext,
    main_store: CsbMainStore,
    State(state): State<S>,
) -> Result<Response, AppError> {
    candidate_list_overview_model(context, main_store, &state)
        .await?
        .pdf_response()
        .await
}

/// The overview of the candidate lists, as Word
pub async fn gen_candidate_list_overview_docx<S: AppRequestState>(
    _: CsbCandidateListOverviewDocxDownloadPath,
    context: CsbContext,
    main_store: CsbMainStore,
    State(state): State<S>,
) -> Result<Response, AppError> {
    candidate_list_overview_model(context, main_store, &state)
        .await?
        .docx_response()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    use crate::{
        AppState, ElectionConfig, ElectoralDistrict, StreamId,
        structs::candidate_lists::CandidateList,
        test_utils::{sample_political_group_named, seed_csb_group},
    };

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

    /// The first download records the list order when none was recorded yet;
    /// further downloads leave it alone.
    #[tokio::test]
    async fn candidate_list_overview_model_records_the_list_order_once() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let first = seed_group(&state, "Eerste").await;
        let second = seed_group(&state, "Tweede").await;
        let main_store = CsbMainStore::new_for_test();
        assert_eq!(main_store.list_order_events(), 0);

        candidate_list_overview_model(CsbContext::new_test(), main_store.clone(), &state).await?;
        assert_eq!(main_store.list_order_events(), 1);
        let mut recorded = main_store.list_order();
        recorded.sort();
        let mut expected = vec![first, second];
        expected.sort();
        assert_eq!(recorded, expected);

        candidate_list_overview_model(CsbContext::new_test(), main_store.clone(), &state).await?;
        assert_eq!(main_store.list_order_events(), 1);

        Ok(())
    }
}
