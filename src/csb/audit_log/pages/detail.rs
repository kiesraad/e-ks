use askama::Template;
use axum::{
    extract::{Query, State},
    response::IntoResponse,
};
use chrono::{DateTime, Utc};

use crate::{
    AppError, AppRequestState, Context, CsbAction, CsbContext, CsbEvent, CsbMainStore, CsbStream,
    ElectionConfig, Event, HasCsbUser, HtmlTemplate, Locale, Overlay, QueryParamState,
    csb::audit_log::pages::CsbAuditLogDetailPath,
    filters,
    projection::{CSB_MAIN_STREAM_ID, WithCorrections},
    store::{StoreData, StoreEvent},
    structs::{
        audit_log::FieldChange,
        brp::{BrpFinding, BrpLookup, BrpLookupOutcome, BrpQuery},
        common::Bsn,
        csb::Correction,
        persons::{Person, PersonId},
    },
    trans,
};

struct CsbEventDetail {
    event_id: usize,
    stream_label: String,
    description: String,
    /// Human-readable label for the committee member that triggered the event
    user: String,
    details: String,
    created_at: DateTime<Utc>,
    changes: Vec<FieldChange>,
    /// Labelled values the event payload alone cannot give: the candidate a
    /// BRP event is about, or what a BRP request sent and got back.
    facts: Vec<Fact>,
    /// Whether `changes` sets the list against the BRP rather than an old
    /// value against a new one, which changes the column headers.
    brp_comparison: bool,
}

/// One labelled value in the detail view.
struct Fact {
    label: String,
    value: String,
}

impl CsbEventDetail {
    /// Look up `event_id` in a stream's events and build its detail view.
    fn find<E: Event + HasCsbUser>(
        events: &[StoreEvent<E>],
        event_id: usize,
        stream_label: String,
        locale: Locale,
    ) -> Result<Self, AppError> {
        let event = events
            .iter()
            .find(|e| e.event_id == event_id)
            .ok_or(AppError::GenericNotFound)?;
        Ok(Self {
            event_id: event.event_id,
            stream_label,
            description: event.payload.description(locale),
            user: event.payload.csb_user().describe(locale),
            details: event.payload.details(),
            changes: event.payload.changes(locale),
            created_at: event.created_at,
            facts: Vec::new(),
            brp_comparison: false,
        })
    }

    /// The diff colouring of a row; a comparison with the BRP has none.
    fn row_class(&self, change: &FieldChange) -> &'static str {
        if self.brp_comparison {
            ""
        } else {
            change.change_kind()
        }
    }

    /// Fill in what the event payload alone cannot say: the value a correction
    /// replaced, or the candidate and data a BRP event is about. Both come
    /// from the stream replayed up to the event before this one.
    fn add_stream_context(
        &mut self,
        events: &[StoreEvent<CsbEvent>],
        election: ElectionConfig,
        locale: Locale,
    ) {
        let Some(index) = events.iter().position(|e| e.event_id == self.event_id) else {
            return;
        };
        let action = &events[index].payload.action;
        if !matches!(
            action,
            CsbAction::UpdateCorrection(_)
                | CsbAction::BrpLookup(_)
                | CsbAction::BrpPersonChecked { .. }
                | CsbAction::SetBrpFindingHandled { .. }
        ) {
            return;
        }

        let before = CsbStream::new_for_temp_stream(election);
        {
            let mut data = before.data.write();
            for event in &events[..index] {
                data.apply(event.clone());
            }
        }

        match action {
            CsbAction::UpdateCorrection(correction) => {
                self.changes = vec![correction_change(&before, correction, locale)];
            }
            CsbAction::BrpLookup(lookup) => self.facts = lookup_facts(&before, lookup, locale),
            CsbAction::BrpPersonChecked { person, findings } => {
                let candidate = before.get_person(*person, WithCorrections::All);
                self.facts = vec![candidate_fact(candidate.as_ref(), *person, locale)];
                if findings.is_empty() {
                    self.facts.push(Fact {
                        label: trans!("audit_log.detail.brp.result", locale),
                        value: trans!("audit_log.detail.brp.agrees", locale),
                    });
                }
                self.brp_comparison = true;
                self.changes = findings
                    .iter()
                    .map(|finding| finding_row(candidate.as_ref(), finding, locale))
                    .collect();
            }
            CsbAction::SetBrpFindingHandled {
                person,
                finding,
                handled,
            } => {
                let candidate = before.get_person(*person, WithCorrections::All);
                self.facts = vec![
                    candidate_fact(candidate.as_ref(), *person, locale),
                    Fact {
                        label: trans!("audit_log.detail.brp.finding", locale),
                        value: finding.message(locale),
                    },
                    Fact {
                        label: trans!("audit_log.detail.brp.handled", locale),
                        value: yes_or_no(*handled, locale),
                    },
                ];
            }
            _ => {}
        }
    }
}

/// The field change of a correction: the value it replaced, as the stream
/// stood before it, against the corrected one.
fn correction_change(before: &CsbStream, correction: &Correction, locale: Locale) -> FieldChange {
    match correction {
        Correction::Appellation(appellation) => FieldChange::Regular {
            field: trans!("audit_log.detail.fields.appellation", locale),
            old_value: before
                .get_political_group(WithCorrections::All)
                .appellation
                .map(|a| a.to_string())
                .unwrap_or_default(),
            new_value: appellation.to_string(),
        },
        Correction::Person(person_id, person_correction) => person_correction.change(
            before.get_person(*person_id, WithCorrections::All).as_ref(),
            locale,
        ),
    }
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

fn yes_or_no(value: bool, locale: Locale) -> String {
    if value {
        trans!("audit_log.detail.values.bool_true", locale)
    } else {
        trans!("audit_log.detail.values.bool_false", locale)
    }
}

/// One finding as a row setting the list against the BRP: the candidate's
/// value of the field it is about, and the value or message from the BRP.
fn finding_row(candidate: Option<&Person>, finding: &BrpFinding, locale: Locale) -> FieldChange {
    let field = finding.field();
    FieldChange::Regular {
        field: field
            .map(|field| field.label(locale))
            .unwrap_or_else(|| trans!("audit_log.detail.brp.finding", locale)),
        old_value: match (field, candidate) {
            (Some(field), Some(candidate)) => field.value_of(candidate, locale),
            _ => String::new(),
        },
        new_value: finding
            .brp_value()
            .map(|value| value.display(locale))
            .unwrap_or_else(|| finding.message(locale)),
    }
}

/// What one request to the BRP was for, sent, and got back.
fn lookup_facts(before: &CsbStream, lookup: &BrpLookup, locale: Locale) -> Vec<Fact> {
    let candidates = lookup
        .persons
        .iter()
        .map(|person_id| {
            candidate_name(
                before.get_person(*person_id, WithCorrections::All).as_ref(),
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
            search_facts(
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
fn search_facts(
    last_name: &str,
    date_of_birth: &str,
    last_name_prefix: Option<&str>,
    gender: Option<&str>,
    include_deceased: bool,
    locale: Locale,
) -> Vec<Fact> {
    let mut facts = vec![Fact {
        label: trans!("audit_log.detail.fields.last_name", locale),
        value: last_name.to_string(),
    }];
    if let Some(prefix) = last_name_prefix {
        facts.push(Fact {
            label: trans!("audit_log.detail.fields.last_name_prefix", locale),
            value: prefix.to_string(),
        });
    }
    facts.push(Fact {
        label: trans!("audit_log.detail.fields.date_of_birth", locale),
        value: date_of_birth.to_string(),
    });
    if let Some(gender) = gender {
        facts.push(Fact {
            label: trans!("audit_log.detail.fields.gender", locale),
            value: gender.to_string(),
        });
    }
    facts.push(Fact {
        label: trans!("audit_log.detail.brp.include_deceased", locale),
        value: yes_or_no(include_deceased, locale),
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
    let detail = if stream_id == CSB_MAIN_STREAM_ID {
        let data = main_store.data.read();
        CsbEventDetail::find(
            &data.events,
            event_id,
            trans!("audit_log.filter.csb_main_stream", locale),
            locale,
        )?
    } else {
        let mut stores = state
            .csb_store_registry()
            .stores_for_election(context.election)
            .await?;
        let import_count = stores.len();
        stores.extend(
            state
                .pre_submission_store_registry()
                .stores_for_election(context.election)
                .await?,
        );
        let position = stores
            .iter()
            .position(|s| s.stream_id == stream_id)
            .ok_or(AppError::GenericNotFound)?;
        let store = &stores[position];
        let label = store.get_appellation_with_deleted_label(WithCorrections::All, locale);
        let label = if position < import_count {
            label
        } else {
            trans!("audit_log.filter.pre_submission_stream", locale, label)
        };
        let data = store.data.read();
        let mut detail = CsbEventDetail::find(&data.events, event_id, label, locale)?;
        detail.add_stream_context(&data.events, context.election, locale);
        detail
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
        AppError, AppState, CsbAction, CsbContext, CsbMainAction, CsbMainStore, CsbUser,
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
        use crate::{
            structs::{
                csb::{Correction, PersonCorrection},
                persons::PersonId,
            },
            test_utils::sample_person,
        };

        let state = AppState::new_for_tests().await;
        let stream_id = StreamId::new();
        let store = state
            .csb_store_for_stream(stream_id, ElectionConfig::EK27)
            .await?;

        // the stream starts with the imported submission, like a real one
        let submission = crate::PgStore::new_for_test();
        let person = sample_person(PersonId::new());
        person.create(&submission).await?;
        let snapshot = Box::new(submission.data.read().clone());
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
        store
            .update(
                CsbAction::UpdateCorrection(Correction::Person(
                    person.id,
                    PersonCorrection::LastName("Nieuwenhuis".parse().unwrap()),
                ))
                .by(CsbUser::new_test()),
            )
            .await?;

        let response = csb_audit_log_detail(
            CsbAuditLogDetailPath {
                stream_id,
                event_id: 2,
            },
            CsbContext::new_test(),
            CsbMainStore::new_for_test(),
            State(state),
            Query(QueryParamState::default()),
        )
        .await?
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Jansen"), "old value missing: {body}");
        assert!(body.contains("Nieuwenhuis"), "new value missing: {body}");

        Ok(())
    }

    /// A stream holding one imported candidate, and the candidate.
    async fn store_with_imported_candidate(
        state: &AppState,
        stream_id: StreamId,
    ) -> Result<(CsbStream, Person), AppError> {
        use crate::test_utils::sample_person;

        let store = state
            .csb_store_for_stream(stream_id, ElectionConfig::EK27)
            .await?;
        let submission = crate::PgStore::new_for_test();
        let person = sample_person(PersonId::new());
        person.create(&submission).await?;
        let snapshot = Box::new(submission.data.read().clone());
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
        Ok((store, person))
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

        assert!(body.contains("Consulted the BRP"), "{body}");
        assert!(body.contains(&person.name.display()), "{body}");
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

        assert!(body.contains("Lookup by social security number"), "{body}");
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

        assert!(body.contains("Checked candidate against the BRP"), "{body}");
        assert!(body.contains(&person.name.display()));
        assert!(body.contains("Value on the list"));
        assert!(body.contains("Value in the BRP"));
        assert!(body.contains("<td>Juinen</td>"), "{body}");
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

        assert!(
            body.contains("The BRP agrees on every checked field."),
            "{body}"
        );

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

        assert!(body.contains(&person.name.display()), "{body}");
        assert!(body.contains("No person in the BRP has this social security number."));
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
