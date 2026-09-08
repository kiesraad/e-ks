use askama::Template;
use axum::response::{IntoResponse, Response};

use crate::{
    AppError, Context, CsbContext, CsbStore, HtmlTemplate,
    csb::examination::{
        extractors::CsbPoliticalGroup,
        pages::CsbGeneralInformationPath,
        structs::{
            PaperCorrectedNameAuthorisation, PaperCorrectedPoliticalGroupInfo,
            PaperCorrectedSubmitter, paper_corrected_list_submitter,
            paper_corrected_name_authorisations, paper_corrected_substitute_submitters,
        },
    },
    filters,
    structs::csb::{CsbPhase, Omission},
};

#[derive(Template)]
#[template(path = "csb/examination/pages/general_information.html")]
struct CsbGeneralInformationTemplate {
    political_group: CsbPoliticalGroup,
    group_info: PaperCorrectedPoliticalGroupInfo,
    name_authorisations: Vec<PaperCorrectedNameAuthorisation>,
    list_submitter: Option<PaperCorrectedSubmitter>,
    substitute_submitters: Vec<PaperCorrectedSubmitter>,
    political_group_omissions: Vec<Omission>,
    appellation_omissions: Vec<Omission>,
}

/// Render the placeholder general information (basisgegevens) page for a
/// single political group under examination.
pub async fn overview(
    _: CsbGeneralInformationPath,
    context: CsbContext,
    store: CsbStore,
) -> Result<Response, AppError> {
    render(context, store, CsbPhase::Examination).await
}

/// The general information page, shared between the examination and the
/// recovery ("Herstelde lijsten") phase.
pub(in crate::csb) async fn render(
    context: CsbContext,
    store: CsbStore,
    mode: CsbPhase,
) -> Result<Response, AppError> {
    Ok(HtmlTemplate(
        CsbGeneralInformationTemplate {
            political_group: CsbPoliticalGroup::new_from_csb_store(&store).with_mode(mode),
            group_info: PaperCorrectedPoliticalGroupInfo::new(&store, context.session.locale, mode),
            name_authorisations: paper_corrected_name_authorisations(&store),
            list_submitter: paper_corrected_list_submitter(&store),
            substitute_submitters: paper_corrected_substitute_submitters(&store),
            political_group_omissions: store.get_political_group_omissions(),
            appellation_omissions: store.get_appellation_omissions(),
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
        CsbAction,
        structs::{
            csb::{OmissionCategory, sample_omission},
            list_designation::ListDesignation,
        },
        test_utils::{response_body_string, sample_political_group},
    };

    #[tokio::test]
    async fn renders_section_headings_and_appellation() {
        let store = CsbStore::new_for_test();
        store.set_political_group(sample_political_group());
        let stream_id = store.stream_id;

        let response = overview(
            CsbGeneralInformationPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // The political group section and the imported appellation
        // (the test session uses the English locale).
        assert!(body.contains("Political group information"));
        assert!(body.contains("Kiesraad Demo"));
        // The substitutes section heading is always present.
        assert!(body.contains("Substitute submitters data"));
    }

    /// A paper correction shows up next to the imported value: the imported
    /// value struck through, the corrected value highlighted.
    #[tokio::test]
    async fn renders_corrected_value_next_to_differing_imported_value() {
        use crate::{CsbAction, PgEvent};

        let store = CsbStore::new_for_test();
        store.set_political_group(sample_political_group());
        let stream_id = store.stream_id;

        let mut corrected_group = sample_political_group();
        corrected_group.appellation = Some("Gecorrigeerde Naam".parse().unwrap());
        store
            .update(CsbAction::PaperCorrectedUpdate(Box::new(
                PgEvent::UpdatePoliticalGroup(corrected_group),
            )))
            .await
            .unwrap();

        let response = overview(
            CsbGeneralInformationPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains(r#"<s class="imported-value">Kiesraad Demo</s>"#));
        assert!(
            body.contains(r#"<strong class="paper-corrected-value">Gecorrigeerde Naam</strong>"#)
        );
    }

    /// A substitute submitter deleted by the paper corrections disappears from
    /// the page instead of rendering struck through.
    #[tokio::test]
    async fn hides_substitute_submitter_deleted_by_the_corrections() {
        use crate::{
            CsbAction, PgEvent, structs::list_submitters::ListSubmitterId,
            test_utils::sample_list_submitter,
        };

        let store = CsbStore::new_for_test();
        store.set_political_group(sample_political_group());
        let stream_id = store.stream_id;

        let submitter = sample_list_submitter(ListSubmitterId::new());
        {
            let mut data = store.data.write();
            data.imported_data.substitute_submitters = vec![submitter.clone()];
            data.paper_corrected_data.substitute_submitters = vec![submitter.clone()];
        }

        store
            .update(CsbAction::PaperCorrectedUpdate(Box::new(
                PgEvent::DeleteSubstituteSubmitter {
                    substitute_submitter_id: submitter.id,
                },
            )))
            .await
            .unwrap();

        let response = overview(
            CsbGeneralInformationPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(!body.contains("Bos"));
    }

    #[tokio::test]
    async fn renders_without_imported_data() {
        // A fresh store has no imported political group or substitutes.
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let response = overview(
            CsbGeneralInformationPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("General Information"));
    }

    #[tokio::test]
    async fn renders_added_political_group_omissions_as_badges() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        Omission::new(
            OmissionCategory::PoliticalGroup,
            "Deposit missing".parse().unwrap(),
            "The deposit has not been paid.".parse().unwrap(),
            None,
        )
        .create(&store)
        .await
        .unwrap();

        let response = overview(
            CsbGeneralInformationPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("restoration-tag"));
        // The badge shows the short title, not the long description.
        assert!(body.contains("Deposit missing"));
        assert!(!body.contains("The deposit has not been paid."));
        // A recoverable omission is not highlighted as an error.
        assert!(!body.contains("restoration-tag-unrecoverable"));
    }

    #[tokio::test]
    async fn renders_non_recoverable_omission_as_error() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let mut omission = Omission::new(
            OmissionCategory::PoliticalGroup,
            "Unregistered appellation".parse().unwrap(),
            "The appellation is not registered.".parse().unwrap(),
            None,
        );
        omission.recoverable = false;
        omission.create(&store).await.unwrap();

        let response = overview(
            CsbGeneralInformationPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("restoration-tag-unrecoverable"));
    }

    #[tokio::test]
    async fn appellation_labels_for_designation_type() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let single_label = "Appellation:";
        let combined_label = "Combined appellation:";

        let cases = [
            (None, Some(single_label)),
            (Some(ListDesignation::Blank), None),
            (Some(ListDesignation::Combined), Some(combined_label)),
            (Some(ListDesignation::Standalone), Some(single_label)),
        ];
        for (designation, expected_label) in cases {
            let mut pg = sample_political_group();
            pg.list_designation = designation;
            store.set_political_group(pg);

            let response = overview(
                CsbGeneralInformationPath { stream_id },
                CsbContext::new_test(),
                store.clone(),
            )
            .await
            .unwrap()
            .into_response();

            let body = response_body_string(response).await;

            match expected_label {
                Some(label) => assert!(body.contains(label)),
                None => assert!(!body.contains(single_label) && !body.contains(combined_label)),
            }
        }
    }

    #[tokio::test]
    async fn appellation_omission_bar_is_hidden_for_blank_lists() {
        let store = CsbStore::new_for_test();
        let mut pg = sample_political_group();
        pg.list_designation = Some(ListDesignation::Blank);
        store.set_political_group(pg);

        let response = overview(
            CsbGeneralInformationPath {
                stream_id: store.stream_id,
            },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();
        let body = response_body_string(response).await;

        assert!(!body.contains("Omissions appellation</h2>"));
    }

    /// A paper correction can blank a list after appellation omissions were
    /// added; those stay visible, but no new ones can be added.
    #[tokio::test]
    async fn appellation_omission_bar_shows_existing_omissions_of_a_blank_list() {
        let store = CsbStore::new_for_test();
        let mut pg = sample_political_group();
        pg.list_designation = Some(ListDesignation::Blank);
        store.set_political_group(pg);
        store
            .update(CsbAction::CreateOmission(sample_omission(
                OmissionCategory::Appellation,
            )))
            .await
            .unwrap();

        let stream_id = store.stream_id;
        let response = overview(
            CsbGeneralInformationPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();
        let body = response_body_string(response).await;

        assert!(body.contains("Omissions appellation</h2>"));
        assert!(body.contains("test title"));
        // The overview is linked, the add dialog is not.
        assert!(body.contains(&format!("/omission/appellation/{stream_id}/overview")));
        assert!(!body.contains(&format!("/omission/appellation/{stream_id}\"")));
    }

    #[tokio::test]
    async fn appellation_omission_bar_shows_for_non_blank_lists() {
        let store = CsbStore::new_for_test();

        let response = overview(
            CsbGeneralInformationPath {
                stream_id: store.stream_id,
            },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();
        let body = response_body_string(response).await;

        assert!(body.contains("Omissions appellation</h2>"));
    }
}
