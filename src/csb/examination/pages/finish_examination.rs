use askama::Template;
use axum::response::{IntoResponse, Response};

use crate::{
    AppError, Context, CsbContext, HtmlTemplate,
    csb::examination::{
        extractors::{CsbPoliticalGroup, CsbPoliticalGroups},
        paths::{CsbFinishExaminationPath, CsbI1DocxDownloadPath, CsbI1DownloadPath},
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
    // A letter of omission is only written for a group that has omissions, and
    // only once its examination is finished, so those are the groups listed.
    let groups_with_omissions = political_groups
        .into_iter()
        .filter(|pg| !pg.is_deleted && pg.is_examination_finished && pg.omission_count > 0)
        .collect();

    Ok(HtmlTemplate(
        CsbFinishExaminationTemplate {
            political_groups: groups_with_omissions,
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

    /// A group that gets a letter of omission: examination finished, not
    /// deleted, at least one omission.
    fn letter_group() -> CsbPoliticalGroup {
        CsbPoliticalGroup {
            political_group: sample_political_group(),
            stream_id: StreamId::new(),
            brp: BrpCheckState::NotChecked,
            mode: crate::structs::csb::CsbPhase::Examination,
            is_examination_finished: true,
            is_deleted: false,
            scrapped: Default::default(),
            restoration_count: 0,
            omission_count: 1,
            recovery: Default::default(),
            first_candidate_name: None,
            candidate_list_districts: Default::default(),
        }
    }

    async fn render(groups: Vec<CsbPoliticalGroup>) -> String {
        let response = finish(
            CsbFinishExaminationPath,
            CsbContext::new_test(),
            CsbPoliticalGroups(groups),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        response_body_string(response).await
    }

    #[tokio::test]
    async fn finish_renders_finished_imported_political_group_name() {
        // The seeded group's appellation is rendered in the letters table.
        assert!(render(vec![letter_group()]).await.contains("Kiesraad Demo"));
    }

    /// The I 1 download buttons point at the PDF and Word endpoints.
    #[tokio::test]
    async fn finish_links_the_i1_downloads() {
        let body = render(vec![letter_group()]).await;

        assert!(body.contains(r#"href="/csb/examination/i1.pdf""#));
        assert!(body.contains(r#"href="/csb/examination/i1.docx""#));
    }

    /// The row links to the group's examination page through the typed path.
    #[tokio::test]
    async fn finish_links_the_political_group_page() {
        let group = letter_group();
        let stream_id = group.stream_id;
        let body = render(vec![group]).await;

        assert!(body.contains(&format!(r#"href="/csb/examination/{stream_id}""#)));
    }

    #[tokio::test]
    async fn finish_renders_without_political_groups() {
        let body = render(vec![]).await;
        assert!(body.contains("There are no letters of omission to create."));
    }

    /// Only finished, undeleted groups with omissions get a letter.
    #[tokio::test]
    async fn finish_skips_groups_without_a_letter_of_omission() {
        let unfinished = CsbPoliticalGroup {
            is_examination_finished: false,
            ..letter_group()
        };
        let deleted = CsbPoliticalGroup {
            is_deleted: true,
            ..letter_group()
        };
        let without_omissions = CsbPoliticalGroup {
            omission_count: 0,
            ..letter_group()
        };

        for group in [unfinished, deleted, without_omissions] {
            let body = render(vec![group]).await;
            assert!(!body.contains("Kiesraad Demo"));
            assert!(body.contains("There are no letters of omission to create."));
        }
    }
}
