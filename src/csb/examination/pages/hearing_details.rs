use askama::Template;
use axum::response::{IntoResponse, Redirect, Response};

use crate::{
    AppError, Context, CsbContext, CsbMainAction, CsbMainStore, ElectionConfig, Form, HtmlTemplate,
    csb::{
        examination::{CsbFinishExaminationPath, CsbHearingDetailsPath, forms::HearingDetailsForm},
        finalise::CsbFinalisePath,
    },
    filters,
    form::FormData,
    structs::csb::{HearingDetails, HearingModel},
};

#[derive(Template)]
#[template(path = "csb/examination/pages/hearing_details.html")]
struct CsbHearingDetailsTemplate {
    election_config: ElectionConfig,
    /// Which proces-verbaal these details belong to; drives the breadcrumbs
    /// and the page the form returns to.
    model: HearingModel,
    form: FormData<HearingDetailsForm>,
}

/// Where saving returns to: the page each model is downloaded from.
fn return_path(model: HearingModel) -> String {
    match model {
        HearingModel::I1 => CsbFinishExaminationPath.to_string(),
        HearingModel::I4 => CsbFinalisePath.to_string(),
    }
}

pub async fn hearing_details(
    CsbHearingDetailsPath { model }: CsbHearingDetailsPath,
    store: CsbMainStore,
    context: CsbContext,
) -> Result<Response, AppError> {
    let hearing_details = store.get_hearing_details(model).unwrap_or_default();
    let form_data = FormData::new_with_data(HearingDetailsForm::from(hearing_details));
    Ok(HtmlTemplate(
        CsbHearingDetailsTemplate {
            election_config: context.election,
            model,
            form: form_data,
        },
        context,
    )
    .into_response())
}

pub async fn hearing_details_submit(
    CsbHearingDetailsPath { model }: CsbHearingDetailsPath,
    store: CsbMainStore,
    context: CsbContext,
    Form(form): Form<HearingDetailsForm>,
) -> Result<Response, AppError> {
    match form.validate_create() {
        Err(form_data) => Ok(HtmlTemplate(
            CsbHearingDetailsTemplate {
                election_config: context.election,
                model,
                form: form_data,
            },
            context,
        )
        .into_response()),
        Ok(validated_target) => {
            let hearing_details = HearingDetails::from(validated_target);
            store
                .update(
                    CsbMainAction::UpdateHearingDetails(model, hearing_details).by(context.user()?),
                )
                .await?;
            Ok(Redirect::to(&return_path(model)).into_response())
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{StatusCode, header};

    use crate::test_utils::response_body_string;

    use super::*;

    async fn render(model: HearingModel) -> String {
        let response = hearing_details(
            CsbHearingDetailsPath { model },
            CsbMainStore::new_for_test(),
            CsbContext::new_test(),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        response_body_string(response).await
    }

    #[tokio::test]
    async fn hearing_details_renders_location_from_election_config() {
        let body = render(HearingModel::I1).await;
        assert!(body.contains("&#39;s-Gravenhage"));
        assert!(body.contains(r#"name="hearing-location""#));
    }

    /// The model is a path segment, so it has to survive a real request: the
    /// axum path deserializer drives it through `deserialize_str`.
    #[tokio::test]
    async fn the_model_is_extracted_from_the_url() {
        use axum::{Router, body::Body, extract::Request};
        use axum_extra::routing::RouterExt;
        use tower::ServiceExt;

        async fn echo(CsbHearingDetailsPath { model }: CsbHearingDetailsPath) -> String {
            model.to_string()
        }

        let app: Router = Router::new().typed_get(echo);

        for model in [HearingModel::I1, HearingModel::I4] {
            let request = Request::builder()
                .uri(CsbHearingDetailsPath { model }.to_string())
                .body(Body::empty())
                .expect("request");
            let response = app.clone().oneshot(request).await.expect("response");

            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response_body_string(response).await, model.to_string());
        }

        let request = Request::builder()
            .uri("/csb/examination/hearing-details/i2")
            .body(Body::empty())
            .expect("request");
        let response = app.oneshot(request).await.expect("response");
        assert_ne!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn hearing_details_renders_a_chair_and_a_signer_input_per_slot() {
        let body = render(HearingModel::I1).await;
        assert_eq!(body.matches(r#"name="chair""#).count(), 1);
        assert_eq!(body.matches(r#"name="signers""#).count(), 10);
        assert!(body.contains(r#"id="signer_0""#));
        assert!(body.contains(r#"id="signer_9""#));
    }

    #[tokio::test]
    async fn hearing_details_renders_csrf_field() {
        let body = render(HearingModel::I1).await;
        assert!(body.contains("name=\"csrf_token\""));
    }

    /// Both models render the same form; only the trail back differs.
    #[tokio::test]
    async fn hearing_details_breadcrumbs_follow_the_model() {
        let i1 = render(HearingModel::I1).await;
        assert!(i1.contains("/csb/examination/finish"));
        assert!(i1.contains("/csb/examination/hearing-details/i1"));

        let i4 = render(HearingModel::I4).await;
        assert!(i4.contains("/csb/finalise"));
        assert!(i4.contains("/csb/examination/hearing-details/i4"));
    }

    fn submitted_form() -> HearingDetailsForm {
        HearingDetailsForm {
            date_of_hearing: "31-12-1999".to_string(),
            time_of_hearing: "12:34".to_string(),
            chair: "Vera Voorzitter".to_string(),
            signers: vec![
                "Jan Klaassen".to_string(),
                "Malle Babbe".to_string(),
                String::new(),
            ],
        }
    }

    async fn submit(store: &CsbMainStore, model: HearingModel) -> Response {
        hearing_details_submit(
            CsbHearingDetailsPath { model },
            store.clone(),
            CsbContext::new_test(),
            Form(submitted_form()),
        )
        .await
        .unwrap()
    }

    fn redirect_location(response: &Response) -> &str {
        response
            .headers()
            .get(header::LOCATION)
            .expect("location header")
            .to_str()
            .expect("location header value")
    }

    #[tokio::test]
    async fn hearing_details_submit_persists_and_redirects() {
        let store = CsbMainStore::new_for_test();
        let response = submit(&store, HearingModel::I1).await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(redirect_location(&response), "/csb/examination/finish");

        let hearing_details = store
            .get_hearing_details(HearingModel::I1)
            .expect("stored hearing details");
        assert_eq!(hearing_details.chair, "Vera Voorzitter");
        assert_eq!(hearing_details.members.len(), 2);
        assert_eq!(
            hearing_details
                .date_time
                .format("%Y-%m-%d %H:%M")
                .to_string(),
            "1999-12-31 12:34"
        );
    }

    #[tokio::test]
    async fn hearing_details_submit_returns_to_the_finalise_page_for_the_i4() {
        let store = CsbMainStore::new_for_test();
        let response = submit(&store, HearingModel::I4).await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(redirect_location(&response), "/csb/finalise");
    }

    /// The two hearings are recorded separately: saving one leaves the other
    /// untouched.
    #[tokio::test]
    async fn hearing_details_are_stored_per_model() {
        let store = CsbMainStore::new_for_test();
        submit(&store, HearingModel::I1).await;

        assert!(store.get_hearing_details(HearingModel::I1).is_some());
        assert!(store.get_hearing_details(HearingModel::I4).is_none());

        submit(&store, HearingModel::I4).await;
        assert!(store.get_hearing_details(HearingModel::I4).is_some());
    }
}
