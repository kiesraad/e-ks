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
