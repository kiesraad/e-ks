use askama::Template;
use axum::response::{IntoResponse, Response};

use crate::{
    AppError, Context, CsbContext, CsbMainStore, HtmlTemplate,
    csb::{
        examination::extractors::CsbPoliticalGroups,
        finalise::{
            numbering::ListNumbering,
            paths::{CsbFinalisePath, CsbListOrderPath},
        },
    },
    filters,
    structs::csb::{HearingModel, Objection},
};

#[derive(Template)]
#[template(path = "csb/finalise/pages/overview.html")]
struct CsbFinaliseTemplate {
    numbering: ListNumbering,
    /// Whether the district count tells the lists apart; a single-district
    /// election has nothing to show there.
    has_multiple_districts: bool,
    objections: Vec<Objection>,
    /// The I 4 can only be downloaded once its hearing details are recorded.
    has_hearing_details: bool,
    unassessed_omission_count: usize,
}

/// The finalise page: the I 4 downloads, the sortable list order, and the
/// objections.
pub async fn overview(
    _: CsbFinalisePath,
    context: CsbContext,
    main_store: CsbMainStore,
    CsbPoliticalGroups(political_groups): CsbPoliticalGroups,
) -> Result<Response, AppError> {
    let numbering = ListNumbering::new(
        &political_groups,
        &main_store.registered_political_groups(),
        &main_store.list_order(),
    );
    let has_multiple_districts = !context.election.has_only_one_district();
    let objections = main_store.get_all_objections();
    let unassessed_omission_count = political_groups
        .into_iter()
        .filter(|political_group| !political_group.is_deleted)
        .map(|political_group| political_group.recovery.pending)
        .sum();

    Ok(HtmlTemplate(
        CsbFinaliseTemplate {
            numbering,
            has_multiple_districts,
            objections,
            has_hearing_details: main_store.get_hearing_details(HearingModel::I4).is_some(),
            unassessed_omission_count,
        },
        context,
    )
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    use crate::{
        CsbMainAction, CsbUser,
        csb::examination::extractors::CsbPoliticalGroup,
        structs::csb::{HearingDetails, sample_registered_political_group},
        test_utils::{contains_word, response_body_string},
    };

    async fn render(main_store: CsbMainStore, groups: Vec<CsbPoliticalGroup>) -> String {
        let response = overview(
            CsbFinalisePath,
            CsbContext::new_test(),
            main_store,
            CsbPoliticalGroups(groups),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        response_body_string(response).await
    }

    /// The I 4 hearing is recorded here, in phase 4, and nowhere else.
    #[tokio::test]
    async fn overview_links_the_i4_hearing_details() {
        let body = render(CsbMainStore::new_for_test(), vec![]).await;

        assert!(body.contains("Hearing details"));
        assert!(body.contains(r#"href="/csb/examination/hearing-details/i4""#));
    }

    /// The I 4 downloads are disabled until the hearing details are recorded.
    #[tokio::test]
    async fn overview_links_the_i4_downloads_only_with_hearing_details() {
        let body = render(CsbMainStore::new_for_test(), vec![]).await;
        assert!(!body.contains(r#"href="/csb/finalise/Model_I4_Proces_Verbaal_Concept.pdf""#));
        assert_eq!(body.matches(r#"aria-disabled="true""#).count(), 3);

        let main_store = CsbMainStore::new_for_test();
        main_store
            .update(
                CsbMainAction::UpdateHearingDetails(HearingModel::I4, HearingDetails::default())
                    .by(CsbUser::new_test()),
            )
            .await
            .unwrap();
        let body = render(main_store, vec![]).await;
        assert!(body.contains(r#"href="/csb/finalise/Model_I4_Proces_Verbaal_Concept.pdf""#));
        assert!(!body.contains("aria-disabled"));
    }

    #[tokio::test]
    async fn overview_renders_without_lists_to_number() {
        let body = render(CsbMainStore::new_for_test(), vec![]).await;

        assert!(body.contains("There are no lists to number."));
        assert!(!body.contains("list-order-table"));
    }

    /// The sortable table lists every group with its stream as row id, the
    /// seated group first with its votes and seats. A group without a seat
    /// is numbered by lot, so its votes are not shown.
    #[tokio::test]
    async fn overview_lists_every_group_seated_first() {
        let main_store = CsbMainStore::new_for_test();
        for group in [
            sample_registered_political_group("Gezeteld", 1000, 2),
            sample_registered_political_group("Zetelloos", 42424, 0),
        ] {
            main_store
                .update(
                    CsbMainAction::CreateRegisteredPoliticalGroup(group).by(CsbUser::new_test()),
                )
                .await
                .unwrap();
        }
        let by_lot = CsbPoliticalGroup::sample("Nieuwkomer");
        let stream_id = by_lot.stream_id;

        let body = render(
            main_store,
            vec![
                by_lot,
                CsbPoliticalGroup::sample("Gezeteld"),
                CsbPoliticalGroup::sample("Zetelloos"),
            ],
        )
        .await;

        assert!(body.contains(r#"data-sortable-update-url="/csb/finalise/order""#));
        assert!(body.contains(r#"data-sortable-update-key="stream_ids""#));
        assert!(body.contains(&format!(r#"data-id="{stream_id}""#)));
        assert!(contains_word(&body, "1000"));
        assert!(!contains_word(&body, "42424"));
        assert!(body.contains("Seats at the previous election"));
        let position = |name: &str| body.find(name).expect("group in table");
        assert!(position("Gezeteld") < position("Nieuwkomer"));
        assert!(body.contains(r#"<span class="badge position-badge">2</span>"#));
    }

    #[tokio::test]
    async fn overview_warns_when_there_are_unassessed_omissions() {
        let main_store = CsbMainStore::new_for_test();

        let mut group = CsbPoliticalGroup::sample("Sample Group");
        group.recovery.total = 1;
        group.recovery.pending = 1;
        let unassessed_response = render(main_store.clone(), vec![group]).await;

        let mut group = CsbPoliticalGroup::sample("Sample Group");
        group.recovery.total = 1;
        group.recovery.pending = 0;
        let assessed_response = render(main_store, vec![group]).await;

        assert!(unassessed_response.contains("unassessed omissions. Go back to phase 3 to assess them."));
        assert!(!assessed_response.contains("unassessed omissions. Go back to phase 3 to assess them."));
    }
}
