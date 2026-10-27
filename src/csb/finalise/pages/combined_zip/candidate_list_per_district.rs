use crate::{
    AppError, AppRequestState, CsbContext, CsbMainStore,
    core::ModelLocale,
    csb::finalise::{
        numbering::list_numbering,
        pages::combined_zip::{
            Files,
            eml::{candidate_lists_stem, eml_hash, eml230b_filename},
        },
    },
    models::{
        candidate_list_per_district::CandidateListPerDistrict,
        csb_model_inputs::published_lists_by_district,
    },
};

/// One candidate list document per district, named after the district's EML
/// 230b in `eml230bs` and carrying its hash
pub(super) async fn candidate_list_per_district_model<S: AppRequestState>(
    context: &CsbContext,
    main_store: &CsbMainStore,
    state: &S,
    eml230bs: &Files,
) -> Result<Vec<CandidateListPerDistrict>, AppError> {
    let election = main_store.election;
    let registry = state.csb_store_registry();
    let mut models = Vec::new();

    let mut numbering = list_numbering(registry, main_store).await?;
    let stream_ids = numbering.stream_ids();

    numbering.record_if_incomplete(context, main_store).await?;

    for (district, lists) in published_lists_by_district(registry, &election, &stream_ids).await? {
        let eml_name = eml230b_filename(&election, Some(district))?;
        let hash = eml230bs
            .iter()
            .find(|(name, _)| *name == eml_name)
            .map(|(_, eml)| eml_hash(eml));

        models.push(CandidateListPerDistrict {
            election_name: election.formal_title(ModelLocale::Nl),
            election_date: election.election_date(),
            electoral_district: district.title().to_string(),
            lists,
            file_stem: candidate_lists_stem(&election, Some(district))?,
            hash,
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

        let models = candidate_list_per_district_model(&context, &store, &state, &Files::new())
            .await
            .expect("models");
        assert_eq!(store.election.electoral_districts().len(), models.len())
    }
}
