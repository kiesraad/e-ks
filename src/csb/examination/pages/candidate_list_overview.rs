use axum::{extract::State, response::Response};

use crate::{
    AppError, AppRequestState, CsbMainStore,
    constants::DEFAULT_DATE_FORMAT,
    core::ModelLocale,
    csb::examination::{
        numbering::list_numbering,
        paths::{CsbCandidateListOverviewDocxDownloadPath, CsbCandidateListOverviewDownloadPath},
    },
    models::{
        Pdf, candidate_list_overview::CandidateListOverview, csb_model_inputs::lists_overview,
    },
};

async fn candidate_list_overview_model<S: AppRequestState>(
    main_store: CsbMainStore,
    state: &S,
) -> Result<CandidateListOverview, AppError> {
    let election = main_store.election;
    let registry = state.csb_store_registry();
    let numbering = list_numbering(registry, &main_store).await?;
    if numbering
        .groups
        .iter()
        .any(|group| group.position.is_none())
    {
        // TODO: should not result in error, see #1319
        return Err(AppError::IncompleteData("List order not recorded"));
    }

    Ok(CandidateListOverview {
        election_name: election.formal_title(ModelLocale::Nl),
        election_date: election
            .election_date()
            .format(DEFAULT_DATE_FORMAT)
            .to_string(),
        electoral_districts: election.electoral_districts().to_vec(),
        lists: lists_overview(registry, &election, &numbering.stream_ids()).await?,
    })
}

/// The publication of the candidate lists, as PDF.
pub async fn gen_candidate_list_overview<S: AppRequestState>(
    _: CsbCandidateListOverviewDownloadPath,
    main_store: CsbMainStore,
    State(state): State<S>,
) -> Result<Response, AppError> {
    candidate_list_overview_model(main_store, &state)
        .await?
        .pdf_response()
        .await
}

/// The same publication as [`gen_osv3_2`], exported as a Word document.
pub async fn gen_candidate_list_overview_docx<S: AppRequestState>(
    _: CsbCandidateListOverviewDocxDownloadPath,
    main_store: CsbMainStore,
    State(state): State<S>,
) -> Result<Response, AppError> {
    candidate_list_overview_model(main_store, &state)
        .await?
        .docx_response()
        .await
}
