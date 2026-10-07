use crate::{
    AppError, AppRequestState, CsbMainStore, csb::examination::numbering::list_numbering,
    models::candidate_list_overview::CandidateListOverview,
};

async fn candidate_list_summary_model<S: AppRequestState>(
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

    todo!("finish this function")
}
