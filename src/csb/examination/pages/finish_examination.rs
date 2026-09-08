use askama::Template;
use axum::response::{IntoResponse, Response};

use crate::{
    AppError, Context, CsbContext, HtmlTemplate,
    csb::examination::{
        extractors::{CsbPoliticalGroup, CsbPoliticalGroups},
        paths::CsbFinishExaminationPath,
    },
    filters,
};

#[derive(Template)]
#[template(path = "csb/examination/pages/finish_examination.html")]
struct CsbFinishExaminationTemplate {
    political_groups: Vec<CsbPoliticalGroup>,
}

pub async fn finish(
    _: CsbFinishExaminationPath,
    context: CsbContext,
    CsbPoliticalGroups(political_groups): CsbPoliticalGroups,
) -> Result<Response, AppError> {
    let finished_political_groups = political_groups
        .into_iter()
        .filter(|pg| pg.is_examination_finished)
        .collect();

    Ok(HtmlTemplate(
        CsbFinishExaminationTemplate {
            political_groups: finished_political_groups,
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
        StreamId,
        csb::examination::structs::BrpCheckState,
        test_utils::{response_body_string, sample_political_group},
    };

    #[tokio::test]
    async fn finish_renders_finished_imported_political_group_name() {
        let groups = CsbPoliticalGroups(vec![CsbPoliticalGroup {
            political_group: sample_political_group(),
            stream_id: StreamId::new(),
            brp: BrpCheckState::NotChecked,
            mode: crate::structs::csb::CsbPhase::Examination,
            is_examination_finished: true,
            is_deleted: false,
            restoration_count: 0,
            omission_count: 0,
            recovery: Default::default(),
            first_candidate_name: None,
            candidate_list_districts: Default::default(),
        }]);

        let response = finish(CsbFinishExaminationPath, CsbContext::new_test(), groups)
            .await
            .unwrap()
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        // The seeded group's appellation is rendered in the "Finished examination" table.
        let body = response_body_string(response).await;
        assert!(body.contains("Kiesraad Demo"));
    }

    #[tokio::test]
    async fn finish_renders_without_political_groups() {
        let response = finish(
            CsbFinishExaminationPath,
            CsbContext::new_test(),
            CsbPoliticalGroups(vec![]),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("There are no finished examinations of any political group."));
    }
}
