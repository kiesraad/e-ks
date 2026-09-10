use askama::Template;
use axum::response::{IntoResponse, Response};

use crate::{
    AppError, Context, CsbContext, CsbMainStore, HtmlTemplate,
    csb::{
        examination::{
            CsbI4DocxDownloadPath, CsbI4DownloadPath, extractors::CsbPoliticalGroups,
            numbering::ListNumbering,
        },
        finalise::paths::{CsbFinalisePath, CsbListOrderPath},
    },
    filters,
};

#[derive(Template)]
#[template(path = "csb/finalise/pages/overview.html")]
struct CsbFinaliseTemplate {
    numbering: ListNumbering,
    /// Whether the district count tells the lists apart; a single-district
    /// election has nothing to show there.
    has_multiple_districts: bool,
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

    Ok(HtmlTemplate(
        CsbFinaliseTemplate {
            numbering,
            has_multiple_districts,
        },
        context,
    )
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use std::collections::HashMap;

    use crate::{
        CsbMainAction, CsbUser, ElectoralDistrict, StreamId,
        csb::examination::extractors::CsbPoliticalGroup,
        structs::{candidate_lists::CandidateListId, csb::sample_registered_political_group},
        test_utils::{response_body_string, sample_political_group},
    };

    fn group(appellation: &str) -> CsbPoliticalGroup {
        CsbPoliticalGroup {
            political_group: crate::structs::political_groups::PoliticalGroup {
                appellation: Some(appellation.parse().unwrap()),
                ..sample_political_group()
            },
            stream_id: StreamId::new(),
            brp: crate::csb::examination::structs::BrpCheckState::NotChecked,
            mode: crate::structs::csb::CsbPhase::Examination,
            is_examination_finished: true,
            is_deleted: false,
            scrapped: Default::default(),
            restoration_count: 0,
            omission_count: 0,
            recovery: Default::default(),
            first_candidate_name: None,
            candidate_list_districts: HashMap::from([(
                CandidateListId::new(),
                vec![ElectoralDistrict::Groningen],
            )]),
        }
    }

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

    /// The I 4 download buttons point at the PDF and Word endpoints.
    #[tokio::test]
    async fn overview_links_the_i4_downloads() {
        let body = render(CsbMainStore::new_for_test(), vec![]).await;

        assert!(body.contains(r#"href="/csb/examination/i4.pdf""#));
        assert!(body.contains(r#"href="/csb/examination/i4.docx""#));
        assert!(body.contains("Hearing details"));
        assert!(body.contains("Add objection"));
    }

    #[tokio::test]
    async fn overview_renders_without_lists_to_number() {
        let body = render(CsbMainStore::new_for_test(), vec![]).await;

        assert!(body.contains("There are no lists to number."));
        assert!(!body.contains("list-order-table"));
    }

    /// The sortable table lists every group with its stream as row id, the
    /// seated group first with its votes and seats.
    #[tokio::test]
    async fn overview_lists_every_group_seated_first() {
        let main_store = CsbMainStore::new_for_test();
        main_store
            .update(
                CsbMainAction::CreateRegisteredPoliticalGroup(sample_registered_political_group(
                    "Gezeteld", 1000, 2,
                ))
                .by(CsbUser::new_test()),
            )
            .await
            .unwrap();
        let by_lot = group("Nieuwkomer");
        let stream_id = by_lot.stream_id;

        let body = render(main_store, vec![by_lot, group("Gezeteld")]).await;

        assert!(body.contains(r#"data-sortable-update-url="/csb/finalise/order""#));
        assert!(body.contains(r#"data-sortable-update-key="stream_ids""#));
        assert!(body.contains(&format!(r#"data-id="{stream_id}""#)));
        assert!(body.contains("1000"));
        assert!(body.contains("Seats at the previous election"));
        let position = |name: &str| body.find(name).expect("group in table");
        assert!(position("Gezeteld") < position("Nieuwkomer"));
        assert!(body.contains(r#"<span class="badge position-badge">2</span>"#));
    }
}
