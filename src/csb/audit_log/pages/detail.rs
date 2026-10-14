use askama::Template;
use axum::{
    extract::{Query, State},
    response::IntoResponse,
};

use crate::{
    AppError, AppRequestState, Context, CsbContext, CsbMainStore, CsbMainStoreData, CsbStoreData,
    ElectoralDistrict, Event, HasCsbUser, HtmlTemplate, Locale, Overlay, QueryParamState, StreamId,
    csb::audit_log::{
        links::{CsbMainLinks, CsbStreamLinks},
        pages::CsbAuditLogDetailPath,
        structs::CsbAuditLogEntry,
    },
    filters,
    projection::{CSB_MAIN_STREAM_ID, CsbAction, CsbStream, WithCorrections},
    store::{StoreData, StoreEvent},
    structs::{
        audit_log::{
            AuditEvent, ChangeGroup, FieldKey, RenderContext, SubjectLinks, bool_label,
            render_groups,
        },
        brp::{BrpFinding, BrpLookup, BrpLookupOutcome, BrpQuery},
        candidate_lists::{CandidateList, CandidateListId},
        common::Bsn,
        csb::{OmissionCategory, OmissionPart},
        persons::{Person, PersonId},
    },
    trans,
};

struct CsbEventDetail {
    entry: CsbAuditLogEntry,
    details: String,
    changes: Vec<ChangeGroup>,
    /// Labelled values the event payload alone cannot give: the candidate a
    /// BRP event is about, or what a BRP request sent and got back.
    facts: Vec<Fact>,
    /// The candidate's values set against the BRP's, for a BRP check. A
    /// comparison, not a diff: nothing changed, so it is not rendered as one.
    comparison: Vec<ComparisonRow>,
}

/// One labelled value in the detail view.
struct Fact {
    label: String,
    value: String,
}

/// One checked field: the value on the list, and the value or message from
/// the BRP.
struct ComparisonRow {
    field: String,
    list_value: String,
    brp_value: String,
}

/// What an event's detail view shows besides its field changes, read from the
/// stream as it stood before the event.
#[derive(Default)]
struct EventContext {
    facts: Vec<Fact>,
    comparison: Vec<ComparisonRow>,
}

impl CsbEventDetail {
    /// Look up `event_id` in a stream's events and build its detail view. The
    /// event's changes and `context` are read against the stream replayed up
    /// to the event before it.
    fn find<D>(
        events: &[StoreEvent<D::Event>],
        stream_id: StreamId,
        event_id: usize,
        stream_label: String,
        locale: Locale,
        links: &dyn SubjectLinks,
        context: impl FnOnce(&D::Event, &D) -> EventContext,
    ) -> Result<Self, AppError>
    where
        D: StoreData,
        D::Event: Event + AuditEvent<State = D> + HasCsbUser + Clone,
    {
        let index = events
            .iter()
            .position(|e| e.event_id == event_id)
            .ok_or(AppError::GenericNotFound)?;
        let event = &events[index];

        let mut before = D::default();
        for earlier in &events[..index] {
            before.apply(earlier.clone());
        }
        let changes = event.payload.changes(&before);
        let EventContext { facts, comparison } = context(&event.payload, &before);
        let entry = CsbAuditLogEntry::from_event(event, stream_id, stream_label, locale, links);

        Ok(Self {
            entry,
            details: event.payload.details(),
            changes: render_groups(&changes, &RenderContext { locale, links }),
            facts,
            comparison,
        })
    }

    /// Nothing to show below the event's metadata.
    fn is_empty(&self) -> bool {
        self.changes.is_empty() && self.facts.is_empty() && self.comparison.is_empty()
    }
}

/// What a BRP or omission event is about, which its payload alone cannot
/// say: the candidate by name, what a request sent and got back, the values a
/// check set against each other, and what an omission concerns.
fn stream_context(action: &CsbAction, before: &CsbStoreData, locale: Locale) -> EventContext {
    match action {
        CsbAction::BrpLookup(lookup) => EventContext {
            facts: lookup_facts(before, lookup, locale),
            ..Default::default()
        },
        CsbAction::BrpPersonChecked { person, findings } => {
            let candidate = before.pg(WithCorrections::All).get_person(*person).ok();
            let mut facts = vec![candidate_fact(candidate, *person, locale)];
            if findings.is_empty() {
                facts.push(Fact {
                    label: trans!("audit_log.detail.brp.result", locale),
                    value: trans!("audit_log.detail.brp.agrees", locale),
                });
            }
            EventContext {
                facts,
                comparison: findings
                    .iter()
                    .map(|finding| comparison_row(candidate, finding, locale))
                    .collect(),
            }
        }
        CsbAction::SetBrpFindingHandled {
            person,
            finding,
            handled,
        } => {
            let candidate = before.pg(WithCorrections::All).get_person(*person).ok();
            EventContext {
                facts: vec![
                    candidate_fact(candidate, *person, locale),
                    Fact {
                        label: trans!("audit_log.detail.brp.finding", locale),
                        value: finding.message(locale),
                    },
                    Fact {
                        label: trans!("audit_log.detail.brp.handled", locale),
                        value: bool_label(*handled, locale),
                    },
                ],
                comparison: Vec::new(),
            }
        }
        CsbAction::CreateOmission(_)
        | CsbAction::UpdateOmission(_)
        | CsbAction::DeleteOmission { .. }
        | CsbAction::SetOmissionStatus { .. }
        | CsbAction::SetOmissionPartStatus { .. } => EventContext {
            facts: omission_facts(action, before, locale),
            ..Default::default()
        },
        _ => EventContext::default(),
    }
}

/// What an omission is about, which its fields leave out, and the part a
/// decision on one part of it is for.
fn omission_facts(action: &CsbAction, before: &CsbStoreData, locale: Locale) -> Vec<Fact> {
    let (omission, part) = match action {
        CsbAction::CreateOmission(o) | CsbAction::UpdateOmission(o) => (Some(o), None),
        CsbAction::DeleteOmission { omission_id }
        | CsbAction::SetOmissionStatus { omission_id, .. } => {
            (before.get_omission(*omission_id).ok(), None)
        }
        CsbAction::SetOmissionPartStatus {
            omission_id, part, ..
        } => (before.get_omission(*omission_id).ok(), Some(part)),
        _ => (None, None),
    };

    let mut facts = omission
        .map(|o| scope_facts(&o.category, before, locale))
        .unwrap_or_default();
    if let Some(part) = part {
        facts.push(Fact {
            label: trans!("audit_log.detail.omission.part", locale),
            value: match part {
                OmissionPart::ElectoralDistrict(district) => district.title().to_string(),
                OmissionPart::CandidateList(list_id) => list_name(before, *list_id),
            },
        });
    }
    facts
}

fn scope_facts(category: &OmissionCategory, before: &CsbStoreData, locale: Locale) -> Vec<Fact> {
    let lists = |lists: &[CandidateListId]| Fact {
        label: FieldKey::CandidateLists.label(locale),
        value: lists
            .iter()
            .map(|list_id| list_name(before, *list_id))
            .collect::<Vec<_>>()
            .join("; "),
    };
    let (kind, scope) = match category {
        OmissionCategory::PoliticalGroup => (
            trans!("audit_log.detail.omission.kind_political_group", locale),
            None,
        ),
        OmissionCategory::Appellation => (
            trans!("audit_log.detail.omission.kind_appellation", locale),
            None,
        ),
        // No districts means all of them.
        OmissionCategory::DeclarationsOfSupport(districts) => (
            trans!(
                "audit_log.detail.omission.kind_declarations_of_support",
                locale
            ),
            Some(Fact {
                label: FieldKey::ElectoralDistricts.label(locale),
                value: if districts.is_empty() {
                    trans!("audit_log.detail.omission.all_districts", locale)
                } else {
                    districts
                        .iter()
                        .map(ElectoralDistrict::title)
                        .collect::<Vec<_>>()
                        .join(", ")
                },
            }),
        ),
        OmissionCategory::CandidateList(list_ids) => (
            trans!("audit_log.detail.omission.kind_candidate_list", locale),
            Some(lists(list_ids)),
        ),
        OmissionCategory::Candidate {
            person,
            lists: list_ids,
        } => (
            format!(
                "{}: {}",
                trans!("audit_log.detail.omission.kind_candidate", locale),
                candidate_name(
                    before.pg(WithCorrections::All).get_person(*person).ok(),
                    *person
                )
            ),
            Some(lists(list_ids)),
        ),
    };
    let about = Fact {
        label: trans!("audit_log.detail.omission.about", locale),
        value: kind,
    };
    std::iter::once(about).chain(scope).collect()
}

/// A candidate list by its districts, or by id when the stream no longer
/// holds it.
fn list_name(before: &CsbStoreData, list_id: CandidateListId) -> String {
    before
        .pg(WithCorrections::Paper)
        .get_candidate_list(list_id)
        .map_or_else(|_| list_id.to_string(), CandidateList::districts_name)
}

/// The candidate by name, or by id when the stream no longer holds them.
fn candidate_fact(candidate: Option<&Person>, person_id: PersonId, locale: Locale) -> Fact {
    Fact {
        label: trans!("audit_log.detail.brp.candidate", locale),
        value: candidate_name(candidate, person_id),
    }
}

fn candidate_name(candidate: Option<&Person>, person_id: PersonId) -> String {
    candidate
        .map(|person| person.name.display())
        .unwrap_or_else(|| person_id.to_string())
}

/// One finding as a row setting the list against the BRP: the candidate's
/// value of the field it is about, and the value or message from the BRP.
fn comparison_row(
    candidate: Option<&Person>,
    finding: &BrpFinding,
    locale: Locale,
) -> ComparisonRow {
    let field = finding.field();
    ComparisonRow {
        field: field
            .map(|field| field.label(locale))
            .unwrap_or_else(|| trans!("audit_log.detail.brp.finding", locale)),
        list_value: match (field, candidate) {
            (Some(field), Some(candidate)) => field.value_of(candidate, locale),
            _ => String::new(),
        },
        brp_value: finding
            .brp_value()
            .map(|value| value.display(locale))
            .unwrap_or_else(|| finding.message(locale)),
    }
}

/// What one request to the BRP was for, sent, and got back.
fn lookup_facts(before: &CsbStoreData, lookup: &BrpLookup, locale: Locale) -> Vec<Fact> {
    let candidates = lookup
        .persons
        .iter()
        .map(|person_id| {
            candidate_name(
                before.pg(WithCorrections::All).get_person(*person_id).ok(),
                *person_id,
            )
        })
        .collect::<Vec<_>>()
        .join(", ");

    let mut facts = vec![Fact {
        label: trans!("audit_log.detail.brp.candidates", locale),
        value: candidates,
    }];
    facts.extend(query_facts(&lookup.query, locale));
    facts.push(outcome_fact(&lookup.outcome, locale));
    facts
}

/// The request as it was sent: its kind, the data in it, and the fields asked
/// for.
fn query_facts(query: &BrpQuery, locale: Locale) -> Vec<Fact> {
    let (kind, data, fields) = match query {
        BrpQuery::ConsultWithBsn { bsn, fields } => (
            trans!("audit_log.detail.brp.query_consult", locale),
            vec![Fact {
                label: trans!("audit_log.detail.brp.bsns_sent", locale),
                value: bsn.iter().map(Bsn::expose).collect::<Vec<_>>().join(", "),
            }],
            fields,
        ),
        BrpQuery::SearchByLastNameAndDateOfBirth {
            last_name,
            date_of_birth,
            last_name_prefix,
            gender,
            include_deceased,
            fields,
        } => (
            trans!("audit_log.detail.brp.query_search", locale),
            search_criteria_facts(
                last_name,
                date_of_birth,
                last_name_prefix.as_deref(),
                gender.as_deref(),
                *include_deceased,
                locale,
            ),
            fields,
        ),
    };

    let mut facts = vec![Fact {
        label: trans!("audit_log.detail.brp.query", locale),
        value: kind,
    }];
    facts.extend(data);
    facts.push(Fact {
        label: trans!("audit_log.detail.brp.fields", locale),
        value: fields
            .iter()
            .map(|field| field.api_name())
            .collect::<Vec<_>>()
            .join(", "),
    });
    facts
}

/// The personal details a search was made on, as they were sent.
fn search_criteria_facts(
    last_name: &str,
    date_of_birth: &str,
    last_name_prefix: Option<&str>,
    gender: Option<&str>,
    include_deceased: bool,
    locale: Locale,
) -> Vec<Fact> {
    let mut facts = vec![Fact {
        label: FieldKey::LastName.label(locale),
        value: last_name.to_string(),
    }];
    if let Some(prefix) = last_name_prefix {
        facts.push(Fact {
            label: FieldKey::LastNamePrefix.label(locale),
            value: prefix.to_string(),
        });
    }
    facts.push(Fact {
        label: FieldKey::DateOfBirth.label(locale),
        value: date_of_birth.to_string(),
    });
    if let Some(gender) = gender {
        facts.push(Fact {
            label: FieldKey::Gender.label(locale),
            value: gender.to_string(),
        });
    }
    facts.push(Fact {
        label: trans!("audit_log.detail.brp.include_deceased", locale),
        value: bool_label(include_deceased, locale),
    });
    facts
}

/// What came back: the persons the BRP returned, or why it did not answer.
fn outcome_fact(outcome: &BrpLookupOutcome, locale: Locale) -> Fact {
    match outcome {
        BrpLookupOutcome::Returned { bsns } => Fact {
            label: trans!("audit_log.detail.brp.returned", locale),
            value: if bsns.is_empty() {
                trans!("audit_log.detail.brp.returned_none", locale)
            } else {
                bsns.join(", ")
            },
        },
        BrpLookupOutcome::Failed { error } => Fact {
            label: trans!("audit_log.detail.brp.error", locale),
            value: error.clone(),
        },
    }
}

#[derive(Template)]
#[template(path = "csb/audit_log/pages/detail.html")]
struct CsbAuditLogDetailTemplate {
    detail: CsbEventDetail,
    overlay: Overlay,
}

/// The detail view of an event on a political group's stream.
fn stream_event_detail(
    import_stores: &[CsbStream],
    pre_submission_stores: &[CsbStream],
    stream_id: StreamId,
    event_id: usize,
    locale: Locale,
) -> Result<CsbEventDetail, AppError> {
    let links = CsbStreamLinks::find(import_stores, pre_submission_stores, stream_id)?;
    CsbEventDetail::find::<CsbStoreData>(
        links.data.events(),
        stream_id,
        event_id,
        links.stream_label(locale),
        locale,
        &links,
        |event, before| stream_context(&event.action, before, locale),
    )
}

pub async fn csb_audit_log_detail<S: AppRequestState>(
    CsbAuditLogDetailPath {
        stream_id,
        event_id,
    }: CsbAuditLogDetailPath,
    context: CsbContext,
    main_store: CsbMainStore,
    State(state): State<S>,
    Query(query): Query<QueryParamState>,
) -> Result<impl IntoResponse, AppError> {
    let locale = context.session.locale;
    let import_stores = state
        .csb_store_registry()
        .stores_for_election(context.election)
        .await?;
    let detail = if stream_id == CSB_MAIN_STREAM_ID {
        let links = CsbMainLinks {
            store: &main_store,
            streams: &import_stores,
            locale,
        };
        CsbEventDetail::find::<CsbMainStoreData>(
            main_store.snapshot().events(),
            stream_id,
            event_id,
            trans!("audit_log.filter.csb_main_stream", locale),
            locale,
            &links,
            |_, _| EventContext::default(),
        )?
    } else {
        let pre_submission_stores = state
            .pre_submission_store_registry()
            .stores_for_election(context.election)
            .await?;
        stream_event_detail(
            &import_stores,
            &pre_submission_stores,
            stream_id,
            event_id,
            locale,
        )?
    };

    Ok(HtmlTemplate(
        CsbAuditLogDetailTemplate {
            detail,
            overlay: Overlay::new_edit(&query),
        },
        context,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::{Query, State},
        http::StatusCode,
        response::IntoResponse,
    };

    use crate::{
        AppError, AppState, CsbAction, CsbContext, CsbMainAction, CsbMainStore, CsbStream, CsbUser,
        ElectionConfig, QueryParamState, StreamId, csb::audit_log::pages::CsbAuditLogDetailPath,
        test_utils::response_body_string,
    };

    #[tokio::test]
    async fn renders_detail_for_main_stream_event() -> Result<(), AppError> {
        let main_store = CsbMainStore::new_for_test();
        main_store
            .update(CsbMainAction::Login.by(CsbUser::Developer))
            .await?;

        let state = AppState::new_for_tests().await;

        let response = csb_audit_log_detail(
            CsbAuditLogDetailPath {
                stream_id: CSB_MAIN_STREAM_ID,
                event_id: 1,
            },
            CsbContext::new_test(),
            main_store,
            State(state),
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Signed in"));

        Ok(())
    }

    /// A correction shows the value it replaced, so the change reads as a
    /// change and not as an addition.
    #[tokio::test]
    async fn correction_detail_shows_the_previous_value() -> Result<(), AppError> {
        use crate::structs::csb::{Correction, PersonCorrection};

        let state = AppState::new_for_tests().await;
        let stream_id = StreamId::new();
        let (store, person) = store_with_imported_candidate(&state, stream_id).await?;
        store
            .update(
                CsbAction::UpdateCorrection(Correction::Person(
                    person.id,
                    PersonCorrection::LastName("Nieuwenhuis".parse().unwrap()),
                ))
                .by(CsbUser::new_test()),
            )
            .await?;

        let body = detail_body(state, stream_id, 2).await?;
        assert!(body.contains("Jansen"), "old value missing: {body}");
        assert!(body.contains("Nieuwenhuis"), "new value missing: {body}");

        Ok(())
    }

    /// A paper correction wraps a political group's event, but its subject
    /// and the candidates in its diff link to the CSB's pages.
    #[tokio::test]
    async fn paper_correction_links_to_the_csb_pages() -> Result<(), AppError> {
        use crate::{
            PgEvent,
            structs::{candidate_lists::CandidateListId, persons::PersonId},
            test_utils::{sample_candidate_list, sample_person},
        };

        let state = AppState::new_for_tests().await;
        let stream_id = StreamId::new();
        let submission = crate::PgStore::new_for_test();
        let person = sample_person(PersonId::new());
        person.create(&submission).await?;
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person.id];
        list.create(&submission).await?;
        let store = import_submission(&state, stream_id, &submission).await?;
        store
            .update(
                CsbAction::PaperCorrectedUpdate(Box::new(PgEvent::UpdateCandidateListOrder {
                    list_id,
                    candidates: Vec::new(),
                }))
                .by(CsbUser::new_test()),
            )
            .await?;

        let body = detail_body(state, stream_id, 2).await?;
        assert!(
            body.contains(&format!(
                "href=\"/csb/examination/{stream_id}/list/{list_id}\""
            )),
            "subject link to the CSB list page missing: {body}"
        );
        assert!(
            body.contains(&format!("<code>{list_id}</code>")),
            "subject id missing: {body}"
        );
        assert!(
            body.contains(&format!("?stream={stream_id}&#38;search={list_id}")),
            "link to the subject's events missing: {body}"
        );
        // The removed candidate's id links to their events on this stream.
        assert!(
            body.contains(&format!(
                "<a href=\"/csb/audit-log?stream={stream_id}&#38;search={}\"><abbr",
                person.id
            )),
            "link to the candidate's events missing: {body}"
        );
        assert!(
            !body.contains("href=\"/candidate-lists/") && !body.contains("href=\"/persons/"),
            "links into the political group's part of the app: {body}"
        );

        Ok(())
    }

    /// A stream holding one imported candidate, and the candidate.
    async fn store_with_imported_candidate(
        state: &AppState,
        stream_id: StreamId,
    ) -> Result<(CsbStream, Person), AppError> {
        use crate::test_utils::sample_person;

        let submission = crate::PgStore::new_for_test();
        let person = sample_person(PersonId::new());
        person.create(&submission).await?;
        let store = import_submission(state, stream_id, &submission).await?;
        Ok((store, person))
    }

    /// The stream `stream_id` with `submission` imported into it.
    async fn import_submission(
        state: &AppState,
        stream_id: StreamId,
        submission: &crate::PgStore,
    ) -> Result<CsbStream, AppError> {
        let store = state
            .csb_store_for_stream(stream_id, ElectionConfig::EK27)
            .await?;
        let snapshot = Box::new(crate::PgStoreData::clone(&submission.snapshot()));
        store
            .update(
                CsbAction::Import {
                    hash: [0; 32],
                    source_stream_id: submission.stream_id,
                    snapshot,
                }
                .by(CsbUser::new_test()),
            )
            .await?;
        Ok(store)
    }

    async fn detail_body(
        state: AppState,
        stream_id: StreamId,
        event_id: usize,
    ) -> Result<String, AppError> {
        let response = csb_audit_log_detail(
            CsbAuditLogDetailPath {
                stream_id,
                event_id,
            },
            CsbContext::new_test(),
            CsbMainStore::new_for_test(),
            State(state),
            Query(QueryParamState::default()),
        )
        .await?
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        Ok(response_body_string(response).await)
    }

    /// A lookup shows who it was for, what was sent, and what came back.
    #[tokio::test]
    async fn brp_lookup_detail_shows_the_request_and_the_answer() -> Result<(), AppError> {
        use crate::structs::brp::{BrpField, BrpLookup, BrpLookupOutcome, BrpQuery};

        let state = AppState::new_for_tests().await;
        let stream_id = StreamId::new();
        let (store, person) = store_with_imported_candidate(&state, stream_id).await?;
        store
            .update(
                CsbAction::BrpLookup(BrpLookup {
                    persons: vec![person.id],
                    query: BrpQuery::SearchByLastNameAndDateOfBirth {
                        last_name: "Jansen".to_string(),
                        date_of_birth: "1980-01-02".to_string(),
                        last_name_prefix: Some("de".to_string()),
                        gender: None,
                        include_deceased: true,
                        fields: vec![BrpField::Bsn, BrpField::LastName],
                    },
                    outcome: BrpLookupOutcome::Returned {
                        bsns: vec!["999992806".to_string(), "999993653".to_string()],
                    },
                })
                .by(CsbUser::new_test()),
            )
            .await?;

        let body = detail_body(state, stream_id, 2).await?;

        assert!(body.contains("Consulted the BRP"));
        assert!(body.contains(&person.name.display()));
        assert!(body.contains("Search by last name and date of birth"));
        assert!(body.contains("Jansen"));
        assert!(body.contains("1980-01-02"));
        assert!(body.contains("burgerservicenummer, naam.geslachtsnaam"));
        assert!(body.contains("999992806, 999993653"));
        assert!(body.contains("Including deceased persons"));

        Ok(())
    }

    #[tokio::test]
    async fn failed_brp_lookup_detail_shows_the_error() -> Result<(), AppError> {
        use crate::structs::brp::{BrpField, BrpLookup, BrpLookupOutcome, BrpQuery};

        let state = AppState::new_for_tests().await;
        let stream_id = StreamId::new();
        let (store, person) = store_with_imported_candidate(&state, stream_id).await?;
        store
            .update(
                CsbAction::BrpLookup(BrpLookup {
                    persons: vec![person.id],
                    query: BrpQuery::ConsultWithBsn {
                        bsn: vec!["999992806".parse().unwrap()],
                        fields: vec![BrpField::Bsn],
                    },
                    outcome: BrpLookupOutcome::Failed {
                        error: "connection refused".to_string(),
                    },
                })
                .by(CsbUser::new_test()),
            )
            .await?;

        let body = detail_body(state, stream_id, 2).await?;

        assert!(body.contains("Lookup by citizen service number"));
        assert!(body.contains("999992806"));
        assert!(body.contains("connection refused"));

        Ok(())
    }

    /// A check sets the candidate's values against the BRP's, by name.
    #[tokio::test]
    async fn brp_check_detail_compares_the_list_with_the_brp() -> Result<(), AppError> {
        use crate::structs::brp::{BrpFindingKind, BrpValue};

        let state = AppState::new_for_tests().await;
        let stream_id = StreamId::new();
        let (store, person) = store_with_imported_candidate(&state, stream_id).await?;
        store
            .update(
                CsbAction::BrpPersonChecked {
                    person: person.id,
                    findings: vec![
                        BrpFindingKind::Mismatch {
                            brp_value: BrpValue::PlaceOfResidence("Utrecht".parse().unwrap()),
                        }
                        .into(),
                        BrpFindingKind::NotDutch.into(),
                    ],
                }
                .by(CsbUser::new_test()),
            )
            .await?;

        let body = detail_body(state, stream_id, 2).await?;

        assert!(body.contains("Checked candidate against the BRP"));
        assert!(body.contains(&person.name.display()));
        assert!(body.contains("Value on the list"));
        assert!(body.contains("Value in the BRP"));
        assert!(body.contains("<td>Juinen</td>"));
        assert!(body.contains("<td>Utrecht</td>"));
        assert!(body.contains("The BRP records no Dutch nationality for this candidate."));
        // A comparison is not a diff, so no row is coloured as one.
        assert!(!body.contains(r#"class="changed""#));
        assert!(!body.contains(r#"class="added""#));

        Ok(())
    }

    #[tokio::test]
    async fn brp_check_detail_says_so_when_the_brp_agrees() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let stream_id = StreamId::new();
        let (store, person) = store_with_imported_candidate(&state, stream_id).await?;
        store
            .update(
                CsbAction::BrpPersonChecked {
                    person: person.id,
                    findings: vec![],
                }
                .by(CsbUser::new_test()),
            )
            .await?;

        let body = detail_body(state, stream_id, 2).await?;

        assert!(body.contains("The BRP matches every checked field."));

        Ok(())
    }

    #[tokio::test]
    async fn brp_finding_handled_detail_names_the_candidate_and_finding() -> Result<(), AppError> {
        use crate::structs::brp::BrpFindingKind;

        let state = AppState::new_for_tests().await;
        let stream_id = StreamId::new();
        let (store, person) = store_with_imported_candidate(&state, stream_id).await?;
        store
            .update(
                CsbAction::SetBrpFindingHandled {
                    person: person.id,
                    finding: BrpFindingKind::BsnUnknown,
                    handled: true,
                }
                .by(CsbUser::new_test()),
            )
            .await?;

        let body = detail_body(state, stream_id, 2).await?;

        assert!(body.contains(&person.name.display()));
        assert!(body.contains("No person in the BRP has this citizen service number."));
        assert!(body.contains("<dt>Handled</dt>"));

        Ok(())
    }

    #[tokio::test]
    async fn close_link_returns_to_redirect_target() -> Result<(), AppError> {
        let main_store = CsbMainStore::new_for_test();
        main_store
            .update(CsbMainAction::Login.by(CsbUser::Developer))
            .await?;

        let state = AppState::new_for_tests().await;
        let return_url = "/csb/audit-log?per_page=20&event_type=login";

        let response = csb_audit_log_detail(
            CsbAuditLogDetailPath {
                stream_id: CSB_MAIN_STREAM_ID,
                event_id: 1,
            },
            CsbContext::new_test(),
            main_store,
            State(state),
            Query(QueryParamState::redirect_to(return_url.to_string())),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // The overlay close link points back at the filtered list, not the bare
        // audit-log path. `&` is HTML-escaped in the rendered attribute.
        assert!(body.contains("href=\"/csb/audit-log?per_page=20&#38;event_type=login\""));

        Ok(())
    }

    #[tokio::test]
    async fn renders_detail_for_import_stream_event() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let stream_id = StreamId::new();
        let csb_store = state
            .csb_store_for_stream(stream_id, ElectionConfig::EK27)
            .await?;
        csb_store
            .update(CsbAction::SetFinished(true).by(CsbUser::new_test()))
            .await?;

        let response = csb_audit_log_detail(
            CsbAuditLogDetailPath {
                stream_id,
                event_id: 1,
            },
            CsbContext::new_test(),
            CsbMainStore::new_for_test(),
            State(state),
            Query(QueryParamState::default()),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Set finished state"));

        Ok(())
    }

    #[tokio::test]
    async fn returns_not_found_for_unknown_event() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;

        let result = csb_audit_log_detail(
            CsbAuditLogDetailPath {
                stream_id: CSB_MAIN_STREAM_ID,
                event_id: 999,
            },
            CsbContext::new_test(),
            CsbMainStore::new_for_test(),
            State(state),
            Query(QueryParamState::default()),
        )
        .await;

        assert!(result.is_err());

        Ok(())
    }
}
