use axum::{extract::State, response::Response};

use crate::{
    AppError, AppRequestState, CsbContext, CsbMainAction, CsbMainStore,
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

    // Set the numbering if it has not been set before. This occurs when this is
    // the first download and no drag event has taken place
    if !numbering.is_complete() {
        main_store
            .update(CsbMainAction::UpdateListOrder(numbering.stream_ids()).by(context.user()?))
            .await?;
    }

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
