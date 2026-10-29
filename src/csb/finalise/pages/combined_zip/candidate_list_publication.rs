use crate::{
    AppError, AppRequestState, CsbContext, CsbMainStore,
    core::ModelLocale,
    csb::finalise::numbering::list_numbering,
    models::{
        candidate_list_publication::CandidateListPublication, csb_model_inputs::published_lists,
    },
    structs::csb::HearingModel,
};

/// Collect the store data the candidate list publication model needs.
pub(super) async fn candidate_list_publication_model<S: AppRequestState>(
    context: &CsbContext,
    main_store: &CsbMainStore,
    state: &S,
) -> Result<CandidateListPublication, AppError> {
    let election = main_store.election;
    let registry = state.csb_store_registry();
    let mut numbering = list_numbering(registry, main_store).await?;

    numbering.record_if_incomplete(context, main_store).await?;

    let configured_session = election.public_session();
    let public_session = main_store
        .get_hearing_details(HearingModel::I4)
        .unwrap_or_else(|| configured_session.into());

    Ok(CandidateListPublication {
        election_name: election.formal_title(ModelLocale::Nl),
        election_date: election.election_date(),
        valid_lists: published_lists(registry, &election, &numbering.stream_ids()).await?,
        session_location: configured_session.location.to_string(),
        public_session,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    use crate::{
        AppState, CsbMainAction, CsbUser, ElectionConfig, ElectoralDistrict, StreamId,
        structs::{
            candidate_lists::CandidateList,
            csb::{OmissionCategory, OmissionStatus, sample_omission},
        },
        test_utils::{sample_political_group_named, seed_csb_group},
    };

    /// Import a group named `appellation` with one list in `districts`. The
    /// list has no candidates: these tests are about which lists appear
    /// where, not about their content.
    async fn seed_group(
        state: &AppState,
        appellation: &str,
        districts: &[ElectoralDistrict],
    ) -> StreamId {
        let list = CandidateList {
            electoral_districts: districts.iter().copied().collect::<BTreeSet<_>>(),
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

    /// A main store with `order` recorded as the list order.
    async fn main_store_with_order(order: Vec<StreamId>) -> Result<CsbMainStore, AppError> {
        let main_store = CsbMainStore::new_for_test();
        main_store
            .update(CsbMainAction::UpdateListOrder(order).by(CsbUser::new_test()))
            .await?;
        Ok(main_store)
    }

    /// Per district, each list's number and appellation.
    fn rows(model: &CandidateListPublication) -> Vec<(&str, Vec<(usize, &str)>)> {
        model
            .valid_lists
            .iter()
            .filter(|&district| !district.lists.is_empty())
            .map(|district| {
                (
                    district.electoral_district.as_str(),
                    district
                        .lists
                        .iter()
                        .map(|numbered| (numbered.number, numbered.list.appellation.as_str()))
                        .collect(),
                )
            })
            .collect()
    }

    /// Per district the lists follow the recorded order; a group only
    /// appears in the districts it has a list in
    #[tokio::test]
    async fn document_uses_global_ordering_even_when_list_not_present_in_district()
    -> Result<(), AppError> {
        let context = CsbContext::new_test();
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

        let model = candidate_list_publication_model(&context, &main_store, &state).await?;

        assert_eq!(
            rows(&model),
            [
                ("Groningen", vec![(1, "Alleen Groningen"), (2, "Overal")]),
                ("Drenthe", vec![(2, "Overal")]),
            ]
        );
        assert_eq!(
            model.election_date,
            chrono::NaiveDate::from_ymd_opt(2027, 5, 24).unwrap()
        );

        Ok(())
    }

    /// A list scrapped in one district drops out of that district only; the
    /// lists after it don't move up a number.
    #[tokio::test]
    async fn document_uses_global_ordering_even_when_list_scrapped_in_district()
    -> Result<(), AppError> {
        let context = CsbContext::new_test();
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

        let model = candidate_list_publication_model(&context, &main_store, &state).await?;

        assert_eq!(
            rows(&model),
            [
                (
                    "Groningen",
                    vec![(1, "Eerste"), (2, "Geschrapt"), (3, "Derde")]
                ),
                ("Drenthe", vec![(1, "Eerste"), (3, "Derde")]),
            ]
        );

        Ok(())
    }

    /// The first download records the starting order when none was recorded
    /// yet, and numbers the lists by it; further downloads leave it alone.
    #[tokio::test]
    async fn candidate_list_publication_model_records_the_list_order_once() -> Result<(), AppError>
    {
        let state = AppState::new_for_tests().await;
        // Created in reverse, so the alphabetical starting order differs
        let second = seed_group(&state, "Tweede", &[ElectoralDistrict::Groningen]).await;
        let first = seed_group(&state, "Eerste", &[ElectoralDistrict::Groningen]).await;
        let main_store = CsbMainStore::new_for_test();
        assert_eq!(main_store.list_order_events(), 0);

        let numbered = [("Groningen", vec![(1, "Eerste"), (2, "Tweede")])];
        let model =
            candidate_list_publication_model(&CsbContext::new_test(), &main_store, &state).await?;
        assert_eq!(rows(&model), numbered);
        assert_eq!(main_store.list_order_events(), 1);
        assert_eq!(main_store.list_order(), [first, second]);

        let model =
            candidate_list_publication_model(&CsbContext::new_test(), &main_store, &state).await?;
        assert_eq!(rows(&model), numbered);
        assert_eq!(main_store.list_order_events(), 1);

        Ok(())
    }
}
