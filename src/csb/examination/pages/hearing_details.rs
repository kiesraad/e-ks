use askama::Template;
use axum::response::{IntoResponse, Redirect, Response};

use crate::{
    AppError, Context, CsbContext, CsbMainAction, CsbMainStore, ElectionConfig, Form, HtmlTemplate,
    csb::examination::{
        CsbFinishExaminationPath, CsbHearingDetailsPath, forms::HearingDetailsForm,
    },
    filters,
    form::FormData,
    structs::csb::HearingDetails,
};

#[derive(Template)]
#[template(path = "csb/examination/pages/hearing_details.html")]
struct CsbHearingDetailsTemplate {
    election_config: ElectionConfig,
    form: FormData<HearingDetailsForm>,
}

pub async fn hearing_details(
    _: CsbHearingDetailsPath,
    store: CsbMainStore,
    context: CsbContext,
) -> Result<Response, AppError> {
    let hearing_details = store.get_hearing_details().unwrap_or_default();
    let form_data = FormData::new_with_data(HearingDetailsForm::from(hearing_details));
    Ok(HtmlTemplate(
        CsbHearingDetailsTemplate {
            election_config: context.election,
            form: form_data,
        },
        context,
    )
    .into_response())
}

pub async fn hearing_details_submit(
    _: CsbHearingDetailsPath,
    store: CsbMainStore,
    context: CsbContext,
    Form(form): Form<HearingDetailsForm>,
) -> Result<Response, AppError> {
    match form.validate_create() {
        Err(form_data) => Ok(HtmlTemplate(
            // TODO propagate validation errors into template
            // TODO unit tests
            CsbHearingDetailsTemplate {
                election_config: context.election,
                form: form_data,
            },
            context,
        )
        .into_response()),
        Ok(validated_target) => {
            let hearing_details = HearingDetails::from(validated_target);
            store
                .update(CsbMainAction::UpdateHearingDetails(hearing_details).by(context.user()?))
                .await?;
            Ok(Redirect::to(&CsbFinishExaminationPath.to_string()).into_response())
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{StatusCode, header};

    use crate::test_utils::response_body_string;

    use super::*;

    async fn render() -> String {
        let response = hearing_details(
            CsbHearingDetailsPath,
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
        let body = dbg!(render().await);
        assert!(body.contains("&#39;s-Gravenhage"));
        assert!(body.contains(r#"input class="disabled" name="hearing-location""#));
    }

    #[tokio::test]
    async fn hearing_details_renders_csrf_field() {
        let body = render().await;
        assert!(body.contains("name=\"csrf_token\""));
    }

    #[tokio::test]
    async fn hearing_details_submit_persists_and_redirects() {
        let store = CsbMainStore::new_for_test();

        let hearing_details_form = HearingDetailsForm {
            date_of_hearing: "31-12-1999".to_string(),
            time_of_hearing: "12:34".to_string(),
            signer_0: "Jan Klaassen".to_string(),
            signer_1: "Malle Babbe".to_string(),
            signer_2: String::new(),
            signer_3: String::new(),
            signer_4: String::new(),
            signer_5: String::new(),
            signer_6: String::new(),
            signer_7: String::new(),
            signer_8: String::new(),
            signer_9: String::new(),
        };

        let response = hearing_details_submit(
            CsbHearingDetailsPath,
            store.clone(),
            CsbContext::new_test(),
            Form(hearing_details_form),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get(header::LOCATION)
            .expect("location header")
            .to_str()
            .expect("location header value");
        assert_eq!(location, "/csb/examination/finish");

        let hearing_details = store.get_hearing_details().unwrap();
        assert_eq!(hearing_details.members.len(), 2);
        assert_eq!(
            hearing_details
                .date_time
                .format("%Y-%m-%d %H:%M")
                .to_string(),
            "1999-12-31 12:34"
        );
    }
}
