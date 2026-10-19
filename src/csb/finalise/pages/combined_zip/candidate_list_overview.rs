use crate::{
    AppError, AppRequestState, CsbContext, CsbMainStore,
    constants::DEFAULT_DATE_FORMAT,
    core::ModelLocale,
    csb::finalise::numbering::list_numbering,
    models::{candidate_list_overview::CandidateListOverview, csb_model_inputs::lists_overview},
};

pub(super) async fn candidate_list_overview_model<S: AppRequestState>(
    context: &CsbContext,
    main_store: &CsbMainStore,
    state: &S,
) -> Result<CandidateListOverview, AppError> {
    let election = main_store.election;
    let registry = state.csb_store_registry();
    let mut numbering = list_numbering(registry, main_store).await?;

    numbering.record_if_incomplete(context, main_store).await?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    use crate::{
        AppState, ElectionConfig, ElectoralDistrict, StreamId,
        structs::{candidate_lists::CandidateList, persons::PersonId},
        test_utils::{sample_person, sample_political_group_named, seed_csb_group},
    };

    /// Import a group named `appellation` with one candidate on one list in
    /// Groningen.
    async fn seed_group(state: &AppState, appellation: &str) -> StreamId {
        let candidate = sample_person(PersonId::new());
        let list = CandidateList {
            electoral_districts: BTreeSet::from([ElectoralDistrict::Groningen]),
            candidates: vec![candidate.id],
            ..Default::default()
        };
        seed_csb_group(
            state,
            ElectionConfig::EK27,
            sample_political_group_named(appellation),
            vec![candidate],
            vec![list],
        )
        .await
        .stream_id
    }

    /// Each group's number and appellation.
    fn numbers(model: &CandidateListOverview) -> Vec<(usize, &str)> {
        model
            .groups
            .iter()
            .map(|group| (group.number, group.appellation.as_str()))
            .collect()
    }

    /// The first download records the starting order when none was recorded
    /// yet, and numbers the lists by it; further downloads leave it alone.
    #[tokio::test]
    async fn candidate_list_overview_model_records_the_list_order_once() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        // Created in reverse, so the alphabetical starting order differs
        let second = seed_group(&state, "Tweede").await;
        let first = seed_group(&state, "Eerste").await;
        let main_store = CsbMainStore::new_for_test();
        assert_eq!(main_store.list_order_events(), 0);

        let numbered = [(1, "Eerste"), (2, "Tweede")];
        let model =
            candidate_list_overview_model(&CsbContext::new_test(), &main_store, &state).await?;
        assert_eq!(numbers(&model), numbered);
        assert_eq!(main_store.list_order_events(), 1);
        assert_eq!(main_store.list_order(), [first, second]);

        let model =
            candidate_list_overview_model(&CsbContext::new_test(), &main_store, &state).await?;
        assert_eq!(numbers(&model), numbered);
        assert_eq!(main_store.list_order_events(), 1);

        Ok(())
    }
}
