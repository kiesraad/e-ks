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
    structs::{audit_log::FieldChange, csb::Correction},
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
        })
    }
}

/// The field changes of a correction event. The value it replaced comes from
/// the stream replayed up to the event before it; other events have none.
fn correction_changes(
    events: &[StoreEvent<CsbEvent>],
    event_id: usize,
    election: ElectionConfig,
    locale: Locale,
) -> Vec<FieldChange> {
    let Some(index) = events.iter().position(|e| e.event_id == event_id) else {
        return vec![];
    };
    let CsbAction::UpdateCorrection(correction) = &events[index].payload.action else {
        return vec![];
    };

    let before = CsbStream::new_for_temp_stream(election);
    {
        let mut data = before.data.write();
        for event in &events[..index] {
            data.apply(event.clone());
        }
    }

    let change = match correction {
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
    };
    vec![change]
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
        detail.changes = correction_changes(&data.events, event_id, context.election, locale);
        detail
    };

    Ok(HtmlTemplate(
        CsbAuditLogDetailTemplate {
            detail,
            overlay: Overlay::new(&query),
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
