use axum::{
    extract::Query,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::routing::TypedPath;
use uuid::Uuid;

use crate::{
    AppError, CsbContext, CsbStore, CsbStream, Form, HtmlTemplate, Locale, Overlay,
    QueryParamState, StreamId,
    csb::examination::{
        OmissionForm,
        extractors::CsbPoliticalGroup,
        pages::{
            CsbAddOmissionPath, CsbDeleteOmissionPath, CsbOmissionOverviewPath, OmissionListQuery,
        },
    },
    form::{FieldErrors, FormData, ValidationError},
    projection::WithCorrections,
    structs::{
        candidate_lists::CandidateListId,
        csb::{OmissionCategory, OmissionType},
        list_designation::ListDesignation,
        persons::PersonId,
    },
    trans,
};

mod urls;
mod views;

use urls::{overview_url_for, return_path};
use views::{CsbAddOmissionTemplate, CsbOmissionOverviewTemplate, omission_views, preset_views};

/// The entity an omission dialog operates on, together with the list context
/// carried through its URLs and presets. Bundled so the handlers and the
/// URL/preset helpers pass one value around instead of repeating the same set
/// of fields in every signature.
#[derive(Clone, Copy)]
pub(super) struct OmissionTarget {
    pub(super) stream_id: StreamId,
    pub(super) omission_type: OmissionType,
    pub(super) reference: Uuid,
    pub(super) list: Option<CandidateListId>,
}

impl OmissionTarget {
    fn from_add_path(path: CsbAddOmissionPath, query: OmissionListQuery) -> Self {
        Self {
            stream_id: path.stream_id,
            omission_type: path.omission_type,
            reference: path.reference,
            list: query.list,
        }
    }

    /// A blank list has no appellation, so it cannot get appellation omissions.
    /// The overview stays reachable: a paper correction can blank a list after
    /// its omissions were added.
    fn ensure_can_add(&self, store: &CsbStream) -> Result<(), AppError> {
        let is_blank = store
            .get_political_group(WithCorrections::All)
            .list_designation
            == Some(ListDesignation::Blank);
        if self.omission_type == OmissionType::Appellation && is_blank {
            return Err(AppError::GenericNotFound);
        }
        Ok(())
    }

    fn from_overview_path(path: CsbOmissionOverviewPath, query: OmissionListQuery) -> Self {
        Self {
            stream_id: path.stream_id,
            omission_type: path.omission_type,
            reference: path.reference,
            list: query.list,
        }
    }

    /// Render the add-omission form tab. Shared by the initial GET and the
    /// re-render after an invalid submit; only the form data differs.
    fn render_add_form(
        &self,
        form: FormData<OmissionForm>,
        query: &QueryParamState,
        context: CsbContext,
        store: &CsbStream,
    ) -> Result<Response, AppError> {
        let available_districts = self
            .omission_type
            .needs_districts()
            .then(|| views::available_electoral_districts(store))
            .filter(|options| options.len() > 1)
            .unwrap_or_default();
        let available_candidate_lists = self
            .omission_type
            .needs_candidate_lists()
            .then(|| {
                views::candidate_list_options(
                    store,
                    (self.omission_type == OmissionType::Candidate)
                        .then(|| PersonId::from(self.reference)),
                )
            })
            .filter(|options| options.len() > 1)
            .unwrap_or_default();

        let political_group = CsbPoliticalGroup::new_from_csb_store(store);
        Ok(HtmlTemplate(
            CsbAddOmissionTemplate {
                form,
                overlay: Overlay::new(query),
                close_action: return_path(self, &political_group),
                presets: preset_views(self, store),
                omission_target: self.to_owned(),
                available_districts,
                available_candidate_lists,
                title_suffix: self.generate_title_suffix(store, context.session.locale)?,
            },
            context,
        )
        .into_response())
    }

    fn generate_title_suffix(&self, store: &CsbStream, locale: Locale) -> Result<String, AppError> {
        let first_candidate = store.get_first_candidate_name(WithCorrections::All, None);
        let appellation = store
            .get_political_group(WithCorrections::All)
            .csb_appellation(first_candidate.as_ref());
        let first_part = match self.omission_type {
            OmissionType::PoliticalGroup => trans!("common.general_information", locale),
            OmissionType::Appellation => {
                match store
                    .get_political_group(WithCorrections::All)
                    .list_designation
                {
                    Some(ListDesignation::Standalone) | None => {
                        trans!("political_group.appellation", locale)
                    }
                    Some(ListDesignation::Combined) => {
                        trans!("political_group.appellation_combined", locale)
                    }
                    Some(ListDesignation::Blank) => {
                        // the user can still get to the add/overview omission page via
                        // all restorations. Users shouldn't apply paper corrections
                        // after adding omissions, but we cannot guarantee this.
                        trans!("political_group.appellation_any", locale)
                    }
                }
            }
            OmissionType::CandidateList => trans!("candidate_list.title_single", locale),
            OmissionType::DeclarationsOfSupport => {
                trans!("csb.declarations_of_support.title", locale)
            }
            OmissionType::Candidate => store
                .get_person(PersonId::from(self.reference), WithCorrections::All)
                .ok_or(AppError::GenericNotFound)?
                .name
                .display(),
        };
        Ok(format!("{} ({})", first_part, appellation))
    }
}

/// Render the "add omission" overlay dialog.
pub async fn add_omission(
    path: CsbAddOmissionPath,
    context: CsbContext,
    store: CsbStore,
    Query(query): Query<QueryParamState>,
    Query(list_query): Query<OmissionListQuery>,
) -> Result<Response, AppError> {
    let target = OmissionTarget::from_add_path(path, list_query);
    target.ensure_can_add(&store)?;
    let form = if target.omission_type == OmissionType::CandidateList {
        // Pre-fill the candidate list from the path
        FormData::new_with_data(OmissionForm {
            candidate_lists: vec![CandidateListId::from(target.reference)],
            ..Default::default()
        })
    } else if target.omission_type == OmissionType::Candidate {
        // Pre-fill the list the dialog was opened from
        let candidate_lists = target.list.map(|id| vec![id]).unwrap_or_default();
        FormData::new_with_data(OmissionForm {
            candidate_lists,
            ..Default::default()
        })
    } else {
        FormData::new()
    };
    target.render_add_form(form, &query, context, &store)
}

/// Render the omissions overview page for an entity: the list of omissions
/// already added, shown in the same dialog as the add-omission form but on its
/// own tab (and its own route).
pub async fn overview(
    path: CsbOmissionOverviewPath,
    context: CsbContext,
    store: CsbStore,
    Query(query): Query<QueryParamState>,
    Query(list_query): Query<OmissionListQuery>,
) -> Result<Response, AppError> {
    let omission_target = OmissionTarget::from_overview_path(path, list_query);
    let political_group = CsbPoliticalGroup::new_from_csb_store(&store);

    Ok(HtmlTemplate(
        CsbOmissionOverviewTemplate {
            overlay: Overlay::new(&query),
            close_action: return_path(&omission_target, &political_group),
            omissions: omission_views(&omission_target, &store)?,
            title_suffix: omission_target.generate_title_suffix(&store, context.session.locale)?,
            omission_target,
        },
        context,
    )
    .into_response())
}

/// Returns the selected values if non-empty, auto-fills if exactly one option
/// is available, or returns a validation error to re-render with.
fn selected_or_only_available<T: Clone>(
    active: bool,
    selected: &[T],
    available: impl FnOnce() -> Vec<T>,
    error_label: &str,
) -> Result<Vec<T>, FieldErrors> {
    if !active {
        return Ok(Vec::new());
    }
    if !selected.is_empty() {
        return Ok(selected.to_vec());
    }

    let available = available();
    if available.len() == 1 {
        Ok(available)
    } else {
        Err(vec![(
            error_label.to_string(),
            ValidationError::ChooseAtLeastOneOption,
        )])
    }
}

/// Handle the submitted "add omission" form: validate, attach the category
/// derived from the path parameters, persist, and redirect back.
pub async fn add_omission_submit(
    path: CsbAddOmissionPath,
    context: CsbContext,
    store: CsbStore,
    Query(query): Query<QueryParamState>,
    Query(list_query): Query<OmissionListQuery>,
    Form(form): Form<OmissionForm>,
) -> Result<Response, AppError> {
    let target = OmissionTarget::from_add_path(path, list_query);
    target.ensure_can_add(&store)?;

    // For candidate list and declarations-of-support omissions at least one district must be selected
    let districts = match selected_or_only_available(
        target.omission_type.needs_districts(),
        &form.electoral_districts,
        || views::available_electoral_districts(&store),
        "electoral_districts",
    ) {
        Ok(districts) => districts,
        Err(errors) => {
            let form = FormData::new_with_errors(form, errors);
            return target.render_add_form(form, &query, context, &store);
        }
    };

    // For candidate omissions at least one list must be selected
    let candidate_lists = match selected_or_only_available(
        target.omission_type.needs_candidate_lists(),
        &form.candidate_lists,
        || {
            views::candidate_list_options(&store, None)
                .into_iter()
                .map(|o| o.id)
                .collect()
        },
        "candidate_lists",
    ) {
        Ok(candidate_lists) => candidate_lists,
        Err(errors) => {
            let form = FormData::new_with_errors(form, errors);
            return target.render_add_form(form, &query, context, &store);
        }
    };

    match form.validate_create() {
        Err(form_data) => target.render_add_form(form_data, &query, context, &store),
        Ok(mut omission) => {
            omission.category = if target.omission_type == OmissionType::DeclarationsOfSupport {
                OmissionCategory::DeclarationsOfSupport(districts)
            } else {
                OmissionCategory::from_type_and_reference(
                    target.omission_type,
                    target.reference,
                    candidate_lists,
                )
            };
            omission.create(&store).await?;

            let political_group = CsbPoliticalGroup::new_from_csb_store(&store);
            Ok(query.redirect_or(return_path(&target, &political_group)))
        }
    }
}

/// Remove a single omission and return to the overview it was listed on,
/// keeping the dialog's `redirect_to` (where the close button returns to) and
/// the overlay marker across the redirect.
pub async fn delete_omission(
    path: CsbDeleteOmissionPath,
    store: CsbStore,
    Query(query): Query<QueryParamState>,
) -> Result<Response, AppError> {
    let CsbDeleteOmissionPath {
        stream_id,
        omission_id,
    } = path;
    let omission = store.get_omission(omission_id)?;
    let overview = overview_url_for(&omission.category, stream_id);
    omission.delete(&store).await?;
    Ok(Redirect::to(
        &overview
            .with_query_params(QueryParamState::overlay(
                query.redirect_url().map(str::to_owned),
                query.is_initial(),
            ))
            .to_string(),
    )
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::http::StatusCode;

    use crate::{
        ElectoralDistrict,
        structs::{
            candidate_lists::CandidateListId, csb::Omission, persons::PersonId,
            political_groups::PoliticalGroup,
        },
        test_utils::{response_body_string, sample_candidate_list, sample_political_group},
    };

    fn sample_form() -> OmissionForm {
        OmissionForm {
            title: "Waarborgsom ontbreekt".to_string(),
            description: "De waarborgsom ontbreekt.".to_string(),
            help_text: "Please pay the deposit.".to_string(),
            recoverable: true,
            electoral_districts: Vec::new(),
            candidate_lists: Vec::new(),
        }
    }

    /// Collapse whitespace so assertions can match attributes the template
    /// renders on separate lines.
    fn normalized(body: &str) -> String {
        body.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    #[tokio::test]
    async fn add_appellation_omission_is_not_found_for_a_blank_list() {
        let store = CsbStore::new_for_test();
        store.set_political_group(PoliticalGroup {
            list_designation: Some(ListDesignation::Blank),
            ..sample_political_group()
        });
        let stream_id = store.stream_id;
        let path = || CsbAddOmissionPath {
            stream_id,
            omission_type: OmissionType::Appellation,
            reference: stream_id.into(),
        };

        let result = add_omission(
            path(),
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            Query(OmissionListQuery::default()),
        )
        .await;
        assert!(matches!(result, Err(AppError::GenericNotFound)));

        let result = add_omission_submit(
            path(),
            CsbContext::new_test(),
            store.clone(),
            Query(QueryParamState::default()),
            Query(OmissionListQuery::default()),
            Form(sample_form()),
        )
        .await;
        assert!(matches!(result, Err(AppError::GenericNotFound)));
        assert_eq!(store.get_omission_count(), 0);
    }

    #[tokio::test]
    async fn add_omission_renders_csrf_and_fields() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let response = add_omission(
            CsbAddOmissionPath {
                stream_id,
                omission_type: OmissionType::Appellation,
                reference: stream_id.into(),
            },
            CsbContext::new_test(),
            store,
            Query(QueryParamState::default()),
            Query(OmissionListQuery::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("name=\"csrf_token\""));
        assert!(body.contains("name=\"title\""));
        assert!(body.contains("name=\"description\""));
        assert!(body.contains("name=\"help_text\""));
        // The dialog title renders with a resolved translation (the test session
        // uses the English locale).
        assert!(body.contains("Add omission"));
        // The pill shows the short preset title, while the full description and
        // help text ride along in data attributes for the client to fill in.
        assert!(body.contains("De aanduiding(en) is/zijn niet geregistreerd"));
        assert!(body.contains("data-title="));
        assert!(body.contains("data-description="));
        assert!(body.contains("data-help-text="));
        assert!(body.contains("data-omission-help-text"));
        // The recoverable flag rides along on the presets and is editable through
        // a checkbox in the form.
        assert!(body.contains("data-recoverable="));
        assert!(body.contains("name=\"recoverable\""));
        // An irreparable preset ("onherstelbaar verzuim") is highlighted as an
        // error and carries `data-recoverable="false"`.
        assert!(body.contains("De aanduiding is niet geregistreerd"));
        assert!(body.contains("omission-preset-unrecoverable"));
        assert!(body.contains("data-recoverable=\"false\""));
        // No unresolved translation keys leaked through.
        assert!(!body.contains("[csb.omission"));
        // The dialog carries a two-step sidebar linking to both tabs, with the
        // add-omission form active by default and the overview on its own route.
        assert!(body.contains("steps-nav"));
        assert!(body.contains(&format!(
            "/csb/examination/{stream_id}/omission/appellation/{stream_id}/overview"
        )));
        assert!(body.contains(">Overview</a>"));
    }

    #[tokio::test]
    async fn add_omission_offers_and_prefills_corrected_districts() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.electoral_districts = vec![ElectoralDistrict::Utrecht, ElectoralDistrict::Fryslan];
        store.add_candidate_list(list.clone());

        // Change Utrecht to Groningen
        list.electoral_districts = vec![ElectoralDistrict::Groningen, ElectoralDistrict::Fryslan];
        store.set_paper_corrected_candidate_list(list);

        let response = add_omission(
            CsbAddOmissionPath {
                stream_id,
                omission_type: OmissionType::DeclarationsOfSupport,
                reference: stream_id.into(),
            },
            CsbContext::new_test(),
            store,
            Query(QueryParamState::default()),
            Query(OmissionListQuery::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = normalized(&response_body_string(response).await);
        // The corrected districts are selectable, the replaced district is disabled
        assert!(body.contains(r#"data-district-nl="Groningen" />"#));
        assert!(body.contains(r#"data-district-nl="Fryslân" />"#));
        assert!(body.contains(r#"data-district-nl="Utrecht" disabled />"#));
    }

    #[tokio::test]
    async fn add_omission_offers_districts_of_paper_added_lists() {
        use crate::test_utils::sample_candidate_list;

        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let list_id = CandidateListId::new();
        // The list only exists in the corrected projection (added on paper)
        store.set_paper_corrected_candidate_list(sample_candidate_list(list_id));

        let render = |store| async move {
            let response = add_omission(
                CsbAddOmissionPath {
                    stream_id,
                    omission_type: OmissionType::DeclarationsOfSupport,
                    reference: stream_id.into(),
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
            normalized(&body)
        };

        // With only one district the selector is hidden
        let body = render(store.clone()).await;
        assert!(!body.contains("data-district-nl"));
        // A second list in Drenthe shows the selector
        let mut list2 = sample_candidate_list(CandidateListId::new());
        list2.electoral_districts = vec![crate::ElectoralDistrict::Drenthe];
        store.set_paper_corrected_candidate_list(list2);
        let body = render(store.clone()).await;
        assert!(body.contains(r#"data-district-nl="Utrecht" />"#));
        assert!(body.contains(r#"data-district-nl="Drenthe" />"#));
    }

    #[tokio::test]
    async fn candidate_dialog_offers_paper_added_lists() {
        use crate::test_utils::{sample_candidate_list, sample_person};

        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let person = sample_person(PersonId::new());
        let person_id = person.id;
        store.add_person(person);
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id];
        store.set_paper_corrected_candidate_list(list);

        let render = |store| async move {
            let response = add_omission(
                CsbAddOmissionPath {
                    stream_id,
                    omission_type: OmissionType::Candidate,
                    reference: person_id.into(),
                },
                CsbContext::new_test(),
                store,
                Query(QueryParamState::default()),
                Query(OmissionListQuery::default()),
            )
            .await
            .unwrap()
            .into_response();
            response_body_string(response).await
        };

        // With only one list the selector is hidden
        let body = render(store.clone()).await;
        assert!(!body.contains(&format!("omission_candidate_list_{list_id}")));
        // A second list this candidate is also on shows the selector
        let second_list_id = CandidateListId::new();
        let mut second_list = sample_candidate_list(second_list_id);
        second_list.candidates = vec![person_id];
        store.set_paper_corrected_candidate_list(second_list);
        let body = render(store.clone()).await;
        assert!(body.contains(&format!("omission_candidate_list_{list_id}")));
        assert!(body.contains(&format!("omission_candidate_list_{second_list_id}")));
    }

    #[tokio::test]
    async fn candidate_dialog_excludes_lists_the_candidate_is_not_on() {
        use crate::test_utils::{sample_candidate_list, sample_person};

        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let person = sample_person(PersonId::new());
        let person_id = person.id;
        store.add_person(person);
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id];
        store.add_candidate_list(list);
        // A second list this candidate does not appear on.
        let other_list_id = CandidateListId::new();
        store.add_candidate_list(sample_candidate_list(other_list_id));

        let response = add_omission(
            CsbAddOmissionPath {
                stream_id,
                omission_type: OmissionType::Candidate,
                reference: person_id.into(),
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

        // Only one list actually has this candidate, so the selector stays
        // hidden (nothing to choose between) and the other list is never
        // offered as an option.
        assert!(!body.contains(&format!("omission_candidate_list_{list_id}")));
        assert!(!body.contains(&format!("omission_candidate_list_{other_list_id}")));
    }

    #[tokio::test]
    async fn candidate_dialog_resolves_placeholders_for_paper_added_candidates() {
        use crate::test_utils::{sample_candidate_list, sample_person};

        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        // Both the candidate and the list only exist in the corrected
        // projection: they were added during paper corrections.
        let person = sample_person(PersonId::new());
        let person_id = person.id;
        store
            .data
            .write()
            .paper_corrected_data
            .persons
            .insert(person_id, person);
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id];
        store.set_paper_corrected_candidate_list(list);

        let response = add_omission(
            CsbAddOmissionPath {
                stream_id,
                omission_type: OmissionType::Candidate,
                reference: person_id.into(),
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
        // The name and position placeholders resolve through the corrected
        // projection.
        assert!(body.contains("Kandidaat nr. 1, Jansen, H.A.H.A. (Henk)"));
    }

    #[tokio::test]
    async fn overview_tab_lists_added_omissions_with_details() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let list = CandidateListId::new();

        // Store the list so get_candidate_list_omissions can look up its districts.
        store.add_candidate_list(sample_candidate_list(list));

        // A recoverable and an irreparable omission scoped to the list.
        Omission::new(
            OmissionCategory::CandidateList(vec![list]),
            "Waarborgsom ontbreekt".parse().unwrap(),
            "De waarborgsom ontbreekt.".parse().unwrap(),
            Some("Betaal de waarborgsom.".parse().unwrap()),
        )
        .create(&store)
        .await
        .unwrap();
        let mut irreparable = Omission::new(
            OmissionCategory::CandidateList(vec![list]),
            "Aanduiding niet geregistreerd".parse().unwrap(),
            "De aanduiding is niet geregistreerd.".parse().unwrap(),
            None,
        );
        irreparable.recoverable = false;
        irreparable.create(&store).await.unwrap();

        let response = overview(
            CsbOmissionOverviewPath {
                stream_id,
                omission_type: OmissionType::CandidateList,
                reference: list.into(),
            },
            CsbContext::new_test(),
            store,
            Query(QueryParamState::default()),
            Query(OmissionListQuery::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // The overview shows each omission's title, description and help text.
        assert!(body.contains("Waarborgsom ontbreekt"));
        assert!(body.contains("De waarborgsom ontbreekt."));
        assert!(body.contains("Betaal de waarborgsom."));
        // The recoverable flag is surfaced per omission.
        assert!(body.contains(">Recoverable</span>"));
        assert!(body.contains(">Not recoverable</span>"));
        assert!(body.contains("omission-item-unrecoverable"));
        // The overview drops the add-omission form (no description field, no
        // submit/save button).
        assert!(!body.contains("data-omission-description"));
        assert!(!body.contains("value=\"save\""));
        // The sidebar still links back to the add-omission form, marked as an
        // in-overlay navigation.
        assert!(body.contains("steps-nav"));
        assert!(body.contains(&format!(
            "/csb/examination/{stream_id}/omission/candidate-list/{list}?&#38;overlay=true\""
        )));
        // Each omission carries a remove button targeting its delete action.
        assert!(body.contains(&format!("/csb/examination/{stream_id}/delete-omission/")));
        assert!(body.contains(">Remove</button>"));
    }

    #[tokio::test]
    async fn delete_omission_removes_it_and_redirects_to_the_overview() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let list = CandidateListId::new();
        store.add_candidate_list(sample_candidate_list(list));

        let omission = Omission::new(
            OmissionCategory::CandidateList(vec![list]),
            "Waarborgsom ontbreekt".parse().unwrap(),
            "De waarborgsom ontbreekt.".parse().unwrap(),
            None,
        );
        omission.create(&store).await.unwrap();
        let omission_id = omission.id;
        assert_eq!(store.get_candidate_list_omissions(list).unwrap().len(), 1);

        let response = delete_omission(
            CsbDeleteOmissionPath {
                stream_id,
                omission_id,
            },
            store.clone(),
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        // The omission is gone...
        assert!(store.get_candidate_list_omissions(list).unwrap().is_empty());
        // ...and we return to the overview it was listed on, without replaying
        // the overlay open animation
        let location = response
            .headers()
            .get("Location")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(location.contains(&format!(
            "/csb/examination/{stream_id}/omission/candidate-list/{list}/overview"
        )));
        assert!(location.contains("overlay=true"));
    }

    #[tokio::test]
    async fn delete_omission_preserves_the_redirect_to() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let omission = Omission::new(
            OmissionCategory::PoliticalGroup,
            "Deposit missing".parse().unwrap(),
            "The deposit is missing.".parse().unwrap(),
            None,
        );
        omission.create(&store).await.unwrap();
        let omission_id = omission.id;

        let response = delete_omission(
            CsbDeleteOmissionPath {
                stream_id,
                omission_id,
            },
            store.clone(),
            Query(QueryParamState::redirect_to("/back/here".to_string())),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert!(store.get_political_group_omissions().is_empty());
        let location = response
            .headers()
            .get("Location")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(location.contains("redirect_to=%2Fback%2Fhere"));
        assert!(location.contains("overlay=true"));
    }

    #[tokio::test]
    async fn overview_shows_empty_state_without_omissions() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let response = overview(
            CsbOmissionOverviewPath {
                stream_id,
                omission_type: OmissionType::PoliticalGroup,
                reference: stream_id.into(),
            },
            CsbContext::new_test(),
            store,
            Query(QueryParamState::default()),
            Query(OmissionListQuery::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("No omissions have been added yet."));
    }

    #[tokio::test]
    async fn add_candidate_list_omission_persists_category() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let list = CandidateListId::new();
        let context = CsbContext::new_test();
        let form = OmissionForm {
            candidate_lists: vec![list],
            ..sample_form()
        };

        let response = add_omission_submit(
            CsbAddOmissionPath {
                stream_id,
                omission_type: OmissionType::CandidateList,
                reference: list.into(),
            },
            context,
            store.clone(),
            Query(QueryParamState::default()),
            Query(OmissionListQuery::default()),
            Form(form),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        // The dialog redirects back to the candidate list it was opened from.
        let location = response
            .headers()
            .get("Location")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(location.contains(&format!("/list/{list}")));

        let omission = store.get_omission_for_test();
        assert_eq!(omission.title.to_string(), "Waarborgsom ontbreekt");
        assert_eq!(
            omission.description.to_string(),
            "De waarborgsom ontbreekt."
        );
        assert!(matches!(
            &omission.category,
            OmissionCategory::CandidateList(lists) if lists == &[list]
        ));
    }

    #[tokio::test]
    async fn add_candidate_list_omission_without_list_selection_rerenders_form() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let list = CandidateListId::new();
        let context = CsbContext::new_test();
        // No candidate list selected and no auto-fill possible: should re-render with an error
        let form = sample_form();

        let response = add_omission_submit(
            CsbAddOmissionPath {
                stream_id,
                omission_type: OmissionType::CandidateList,
                reference: list.into(),
            },
            context,
            store.clone(),
            Query(QueryParamState::default()),
            Query(OmissionListQuery::default()),
            Form(form),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(store.get_political_group_omissions().is_empty());
    }

    #[tokio::test]
    async fn add_political_group_omission_persists_category() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let context = CsbContext::new_test();
        let form = sample_form();

        add_omission_submit(
            CsbAddOmissionPath {
                stream_id,
                omission_type: OmissionType::PoliticalGroup,
                reference: stream_id.into(),
            },
            context,
            store.clone(),
            Query(QueryParamState::default()),
            Query(OmissionListQuery::default()),
            Form(form),
        )
        .await
        .unwrap();

        assert_eq!(store.get_political_group_omissions().len(), 1);
    }

    #[tokio::test]
    async fn add_omission_persists_the_recoverable_flag() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let context = CsbContext::new_test();
        // An unchecked "recoverable" checkbox submits nothing, marking the
        // omission irreparable.
        let mut form = sample_form();
        form.recoverable = false;

        add_omission_submit(
            CsbAddOmissionPath {
                stream_id,
                omission_type: OmissionType::PoliticalGroup,
                reference: stream_id.into(),
            },
            context,
            store.clone(),
            Query(QueryParamState::default()),
            Query(OmissionListQuery::default()),
            Form(form),
        )
        .await
        .unwrap();

        let omission = store.get_omission_for_test();
        assert!(!omission.recoverable);
    }

    #[tokio::test]
    async fn add_omission_invalid_form_rerenders() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let context = CsbContext::new_test();
        let mut form = sample_form();
        // An empty description is invalid.
        form.description = String::new();

        let response = add_omission_submit(
            CsbAddOmissionPath {
                stream_id,
                omission_type: OmissionType::PoliticalGroup,
                reference: stream_id.into(),
            },
            context,
            store.clone(),
            Query(QueryParamState::default()),
            Query(OmissionListQuery::default()),
            Form(form),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(store.get_political_group_omissions().is_empty());
    }

    #[tokio::test]
    async fn candidate_dialog_interpolates_candidate_placeholders() {
        use crate::test_utils::{sample_candidate_list, sample_person};

        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        // Seed a candidate at position 1 of a list.
        let person = sample_person(PersonId::new());
        let person_id = person.id;
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id];
        store.add_person(person);
        store.add_candidate_list(list);

        let response = add_omission(
            CsbAddOmissionPath {
                stream_id,
                omission_type: OmissionType::Candidate,
                reference: person_id.into(),
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
        // The candidate's name and position are interpolated into the preset.
        assert!(body.contains("Kandidaat nr. 1, Jansen, H.A.H.A. (Henk)"));
        // The unresolved token is left for the committee to fill in manually.
        assert!(body.contains("{appellation}"));
        assert!(!body.contains("{candidate_name}"));
        // Both former "candidate" and "person" presets are shown.
        assert!(body.contains("Kopie ID ontbreekt"));
        // ...while a preset scoped to the listing on the list does not.
        assert!(!body.contains("onjuiste nadere aanduidingen"));
    }

    #[tokio::test]
    async fn candidate_position_is_scoped_to_the_referenced_list() {
        use crate::test_utils::{sample_candidate_list, sample_person};

        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        // The same candidate sits at different positions on two lists.
        let person = sample_person(PersonId::new());
        let person_id = person.id;
        store.add_person(person);

        let first_list_id = CandidateListId::new();
        let mut first_list = sample_candidate_list(first_list_id);
        first_list.candidates = vec![person_id];
        store.add_candidate_list(first_list);

        let second_list_id = CandidateListId::new();
        let mut second_list = sample_candidate_list(second_list_id);
        second_list.candidates = vec![PersonId::new(), person_id];
        store.add_candidate_list(second_list);

        // Opening the dialog for the second list resolves position 2, not 1.
        let response = add_omission(
            CsbAddOmissionPath {
                stream_id,
                omission_type: OmissionType::Candidate,
                reference: person_id.into(),
            },
            CsbContext::new_test(),
            store,
            Query(QueryParamState::default()),
            Query(OmissionListQuery {
                list: Some(second_list_id),
            }),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Kandidaat nr. 2, Jansen, H.A.H.A. (Henk)"));
        assert!(!body.contains("Kandidaat nr. 1, Jansen"));
    }

    #[tokio::test]
    async fn add_candidate_omission_persists_the_selected_lists() {
        use crate::test_utils::{sample_candidate_list, sample_person};

        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let context = CsbContext::new_test();

        let person = sample_person(PersonId::new());
        let person_id = person.id;
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id];
        store.add_person(person);
        store.add_candidate_list(list);

        let form = OmissionForm {
            candidate_lists: vec![list_id],
            ..sample_form()
        };

        let response = add_omission_submit(
            CsbAddOmissionPath {
                stream_id,
                omission_type: OmissionType::Candidate,
                reference: person_id.into(),
            },
            context,
            store.clone(),
            Query(QueryParamState::default()),
            Query(OmissionListQuery {
                list: Some(list_id),
            }),
            Form(form),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        // The dialog redirects back to the candidate detail page it was opened from.
        let location = response
            .headers()
            .get("Location")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(location.contains(&format!("/list/{list_id}/candidate/{person_id}")));

        let omission = store.get_omission_for_test();
        assert!(matches!(
            omission.category,
            OmissionCategory::Candidate { person, ref lists }
                if person == person_id && lists == &[list_id]
        ));
    }

    #[tokio::test]
    async fn add_candidate_omission_auto_fills_single_list() {
        use crate::test_utils::{sample_candidate_list, sample_person};

        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let context = CsbContext::new_test();

        let person = sample_person(PersonId::new());
        let person_id = person.id;
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id];
        store.add_person(person);
        store.add_candidate_list(list);

        // No list selected in the form: the single available list is auto-filled
        let response = add_omission_submit(
            CsbAddOmissionPath {
                stream_id,
                omission_type: OmissionType::Candidate,
                reference: person_id.into(),
            },
            context,
            store.clone(),
            Query(QueryParamState::default()),
            Query(OmissionListQuery {
                list: Some(list_id),
            }),
            Form(sample_form()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let omission = store.get_omission_for_test();
        let OmissionCategory::Candidate { person, ref lists } = omission.category else {
            panic!("Should be a candidate omission")
        };
        assert_eq!(person, person_id);
        assert_eq!(lists, &[list_id]);
    }
}
