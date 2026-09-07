use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
};

use crate::{
    AppError, Context, CsbContext, CsbStore, Form, HtmlTemplate, Overlay, QueryParamState,
    csb::examination::{
        extractors::CsbPoliticalGroup,
        pages::{
            CsbAppellationCorrectionPath, CsbPersonCorrectionPath, OmissionListQuery,
            correction::{
                CorrectionForm, parse_person_correction, return_path,
                structs::{CorrectionDisplay, CorrectionFieldType, FieldValues},
            },
        },
    },
    filters,
    form::FormData,
    structs::{common::Appellation, csb::Correction},
};
#[derive(Template)]
#[template(path = "csb/examination/pages/correction.html")]
struct CsbCorrectionTemplate {
    overlay: Overlay,
    close_action: String,
    /// Translated label for the field being corrected
    label: String,
    /// The value from the original imported data
    imported_value: String,
    /// The paper-corrected value, shown only when it differs from the imported
    paper_corrected_value: Option<String>,
    /// What the BRP holds for this field, when it differs
    brp_value: Option<String>,
    field_type: CorrectionFieldType,
    form: FormData<CorrectionForm>,
}

/// Render the correction overlay for the political-group appellation.
pub async fn appellation_name_correction(
    _: CsbAppellationCorrectionPath,
    context: CsbContext,
    store: CsbStore,
    Query(query): Query<QueryParamState>,
) -> Result<Response, AppError> {
    let political_group = CsbPoliticalGroup::new_from_csb_store(&store);
    let locale = context.session.locale;
    let field_values = FieldValues::for_appellation(&store);
    let value = field_values.prefill();
    Ok(render_correction(
        context,
        query,
        political_group.general_information_path().to_string(),
        field_values.into_display(
            crate::trans!("political_group.appellation", locale),
            CorrectionFieldType::Text,
        ),
        FormData::new_with_data(CorrectionForm { value }),
    ))
}

/// Handle the submitted appellation correction form.
pub async fn appellation_correction_submit(
    _: CsbAppellationCorrectionPath,
    context: CsbContext,
    store: CsbStore,
    Query(query): Query<QueryParamState>,
    Form(form): Form<CorrectionForm>,
) -> Result<Response, AppError> {
    let political_group = CsbPoliticalGroup::new_from_csb_store(&store);
    let close_action = political_group.general_information_path().to_string();
    let locale = context.session.locale;

    match form.value.parse::<Appellation>() {
        Err(err) => Ok(render_correction(
            context,
            query,
            close_action,
            FieldValues::for_appellation(&store).into_display(
                crate::trans!("political_group.appellation", locale),
                CorrectionFieldType::Text,
            ),
            FormData::new_with_errors(form, vec![("value".to_string(), err)]),
        )),
        Ok(appellation) => {
            store
                .update(crate::CsbAction::UpdateCorrection(Correction::Appellation(
                    appellation,
                )))
                .await?;
            Ok(query.redirect_or(close_action))
        }
    }
}

/// Render the correction overlay for a specific personal-data field of a candidate.
pub async fn person_correction(
    path: CsbPersonCorrectionPath,
    context: CsbContext,
    store: CsbStore,
    Query(query): Query<QueryParamState>,
    Query(list_query): Query<OmissionListQuery>,
) -> Result<Response, AppError> {
    let political_group = CsbPoliticalGroup::new_from_csb_store(&store);
    let close_action = return_path(&political_group, path.person_id, list_query.list);
    let locale = context.session.locale;

    let field_values = FieldValues::for_person(&store, path.person_id, path.field, locale);
    let value = field_values.prefill();
    Ok(render_correction(
        context,
        query,
        close_action,
        field_values.into_person_display(path.field, locale),
        FormData::new_with_data(CorrectionForm { value }),
    ))
}

/// Handle the submitted person-field correction form.
pub async fn person_correction_submit(
    path: CsbPersonCorrectionPath,
    context: CsbContext,
    store: CsbStore,
    Query(query): Query<QueryParamState>,
    Query(list_query): Query<OmissionListQuery>,
    Form(form): Form<CorrectionForm>,
) -> Result<Response, AppError> {
    let political_group = CsbPoliticalGroup::new_from_csb_store(&store);
    let close_action = return_path(&political_group, path.person_id, list_query.list);
    let locale = context.session.locale;

    let correction = parse_person_correction(path.field, &form.value);

    match correction {
        Err(err) => Ok(render_correction(
            context,
            query,
            close_action,
            FieldValues::for_person(&store, path.person_id, path.field, locale)
                .into_person_display(path.field, locale),
            FormData::new_with_errors(form, vec![("value".to_string(), err)]),
        )),
        Ok(person_correction) => {
            store
                .update(crate::CsbAction::UpdateCorrection(Correction::Person(
                    path.person_id,
                    person_correction,
                )))
                .await?;
            Ok(query.redirect_or(close_action))
        }
    }
}

fn render_correction(
    context: CsbContext,
    query: QueryParamState,
    close_action: String,
    display: CorrectionDisplay,
    form: FormData<CorrectionForm>,
) -> Response {
    HtmlTemplate(
        CsbCorrectionTemplate {
            overlay: Overlay::new(&query),
            close_action,
            label: display.label,
            imported_value: display.imported_value,
            paper_corrected_value: display.paper_corrected_value,
            brp_value: display.brp_value,
            field_type: display.field_type,
            form,
        },
        context,
    )
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection::WithCorrections;
    use axum::http::StatusCode;

    use crate::{
        csb::examination::structs::CandidateCorrectionField,
        structs::persons::PersonId,
        test_utils::{
            response_body_string, sample_candidate_list, sample_person, sample_political_group,
        },
    };

    #[tokio::test]
    async fn the_brp_value_is_offered_while_correcting_the_field_it_belongs_to() {
        use crate::{
            CsbAction,
            structs::brp::{BrpFinding, BrpValue},
        };

        let store = crate::CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let person = sample_person(PersonId::new());
        let person_id = person.id;
        store.add_person(person);
        store
            .update(CsbAction::BrpPersonChecked {
                person: person_id,
                findings: vec![BrpFinding::Mismatch {
                    brp_value: BrpValue::PlaceOfResidence("Amsterdam".parse().unwrap()),
                }],
            })
            .await
            .unwrap();

        let response = person_correction(
            CsbPersonCorrectionPath {
                stream_id,
                person_id,
                field: CandidateCorrectionField::PlaceOfResidence,
            },
            CsbContext::new_test(),
            store,
            Query(QueryParamState::default()),
            Query(OmissionListQuery::default()),
        )
        .await
        .unwrap()
        .into_response();

        let body = response_body_string(response).await;
        // Named next to the imported value, and offered as the placeholder so
        // it is still there once the pre-filled value is cleared.
        assert!(body.contains("Value in the BRP"), "{body}");
        assert!(body.contains(r#"placeholder="Amsterdam""#), "{body}");
        // The field keeps its own current value as the prefill.
        assert!(body.contains(r#"value="Juinen""#));
    }

    #[tokio::test]
    async fn a_field_the_brp_agrees_on_gets_no_suggestion() {
        let store = crate::CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let person = sample_person(PersonId::new());
        let person_id = person.id;
        store.add_person(person);

        let response = person_correction(
            CsbPersonCorrectionPath {
                stream_id,
                person_id,
                field: CandidateCorrectionField::PlaceOfResidence,
            },
            CsbContext::new_test(),
            store,
            Query(QueryParamState::default()),
            Query(OmissionListQuery::default()),
        )
        .await
        .unwrap()
        .into_response();

        let body = response_body_string(response).await;
        assert!(!body.contains("Value in the BRP"));
    }

    #[tokio::test]
    async fn appellation_correction_renders_overlay() {
        use crate::test_utils::sample_political_group;

        let store = crate::CsbStore::new_for_test();
        let stream_id = store.stream_id;
        store.set_political_group(sample_political_group());

        let response = appellation_name_correction(
            CsbAppellationCorrectionPath { stream_id },
            CsbContext::new_test(),
            store,
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("name=\"value\""));
        assert!(body.contains("name=\"csrf_token\""));
    }

    #[tokio::test]
    async fn appellation_correction_submit_persists_correction() {
        let store = crate::CsbStore::new_for_test();
        let stream_id = store.stream_id;
        store.set_political_group(sample_political_group());

        let response = appellation_correction_submit(
            CsbAppellationCorrectionPath { stream_id },
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            Form(CorrectionForm {
                value: "Nieuwe Naam".to_string(),
            }),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            store.get_appellation(WithCorrections::Paper),
            "Kiesraad Demo"
        );
        assert_eq!(store.get_appellation(WithCorrections::All), "Nieuwe Naam");
    }

    #[tokio::test]
    async fn appellation_correction_submit_rerenders_on_invalid_value() {
        let store = crate::CsbStore::new_for_test();
        let stream_id = store.stream_id;
        store.set_political_group(sample_political_group());

        let response = appellation_correction_submit(
            CsbAppellationCorrectionPath { stream_id },
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            Form(CorrectionForm {
                value: String::new(),
            }),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(store.get_appellation(WithCorrections::All), "Kiesraad Demo");
    }

    #[tokio::test]
    async fn person_correction_renders_overlay_with_initials() {
        let store = crate::CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let person = sample_person(PersonId::new());
        let person_id = person.id;
        let list_id = crate::structs::candidate_lists::CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id];
        store.add_person(person);
        store.add_candidate_list(list);

        let response = person_correction(
            CsbPersonCorrectionPath {
                stream_id,
                person_id,
                field: CandidateCorrectionField::Initials,
            },
            CsbContext::new_test(),
            store,
            Query(QueryParamState::default()),
            Query(OmissionListQuery {
                list: Some(list_id),
            }),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("name=\"value\""));
        // Sample person has initials H.A.H.A.
        assert!(body.contains("H.A.H.A."));
    }

    #[tokio::test]
    async fn person_correction_submit_persists_initials() {
        let store = crate::CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let person = sample_person(PersonId::new());
        let person_id = person.id;
        store.add_person(person);

        let response = person_correction_submit(
            CsbPersonCorrectionPath {
                stream_id,
                person_id,
                field: CandidateCorrectionField::Initials,
            },
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            Query(OmissionListQuery::default()),
            Form(CorrectionForm {
                value: "X.Y.Z.".to_string(),
            }),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let corrected = store.get_person(person_id, WithCorrections::All).unwrap();
        assert_eq!(corrected.name.initials.to_string(), "X.Y.Z.");
    }

    #[tokio::test]
    async fn person_correction_submit_rerenders_on_invalid_initials() {
        let store = crate::CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let person = sample_person(PersonId::new());
        let person_id = person.id;
        store.add_person(person);

        let response = person_correction_submit(
            CsbPersonCorrectionPath {
                stream_id,
                person_id,
                field: CandidateCorrectionField::Initials,
            },
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            Query(OmissionListQuery::default()),
            Form(CorrectionForm {
                value: String::new(),
            }),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            store.get_person(person_id, WithCorrections::All),
            store.get_person(person_id, WithCorrections::None)
        );
    }
}
