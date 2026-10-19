use crate::{
    AppError, AppRequestState, CsbContext, CsbMainStore,
    constants::DEFAULT_DATE_FORMAT,
    core::ModelLocale,
    csb::finalise::numbering::list_numbering,
    models::{
        candidate_list_per_district::CandidateListPerDistrict, csb_model_inputs::published_lists,
    },
};

pub(super) async fn candidate_list_per_district_model<S: AppRequestState>(
    context: &CsbContext,
    main_store: &CsbMainStore,
    state: &S,
) -> Result<Vec<CandidateListPerDistrict>, AppError> {
    let election = main_store.election;
    let registry = state.csb_store_registry();
    let mut models = Vec::new();

    let mut numbering = list_numbering(registry, main_store).await?;
    let stream_ids = numbering.stream_ids();

    numbering.record_if_incomplete(context, main_store).await?;

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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{AppState, CsbContext, CsbMainStore};

    #[tokio::test]
    async fn models_generates_model_foreach_district() {
        let store = CsbMainStore::new_for_test();
        let context = CsbContext::new_test();
        let state = AppState::new_for_tests().await;

        let models = candidate_list_per_district_model(&context, &store, &state)
            .await
            .expect("models");
        assert_eq!(store.election.electoral_districts().len(), models.len())
    }
}
