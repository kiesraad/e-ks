use askama::Template;
use axum::{
    extract::{Query, State},
    response::IntoResponse,
};

use crate::{
    AppError, AppRequestState, Context, CsbContext, CsbMainStore, Event, HtmlTemplate, Locale,
    StreamId,
    csb::audit_log::{pages::CsbAuditLogPath, structs::CsbAuditLogEntry},
    filters,
    pagination::Pagination,
    store::StoreEvent,
    structs::audit_log::EventTypeCategory,
    trans,
    utils::filter_query_suffix,
};

const PER_PAGE: usize = 20;

/// Event type categories grouped with their specific event keys, used by the
/// filter dropdown to render `<optgroup>`s with fine-grained `<option>`s.
///
/// Category label translations (referenced dynamically in the template):
/// trans!("audit_log.filter.category.import", _)
/// trans!("audit_log.filter.category.delete", _)
/// trans!("audit_log.filter.category.paper_correction", _)
/// trans!("audit_log.filter.category.correction", _)
/// trans!("audit_log.filter.category.set_finished", _)
/// trans!("audit_log.filter.category.omission", _)
/// trans!("audit_log.filter.category.system", _)
/// trans!("audit_log.filter.category.registered_political_group", _)
///
/// Event type option labels (referenced dynamically in the template):
/// trans!("audit_log.event.paper_correction", _)
pub const EVENT_TYPES_BY_CATEGORY: &[EventTypeCategory] = &[
    EventTypeCategory {
        key: "import",
        event_types: &["import", "create_empty"],
    },
    EventTypeCategory {
        key: "delete",
        event_types: &["delete"],
    },
    EventTypeCategory {
        key: "paper_correction",
        // key() delegates to the wrapped PgEvent, so filter by category string
        event_types: &["paper_correction"],
    },
    EventTypeCategory {
        key: "correction",
        event_types: &["update_correction"],
    },
    EventTypeCategory {
        key: "set_finished",
        event_types: &["set_finished"],
    },
    EventTypeCategory {
        key: "omission",
        event_types: &["create_omission", "update_omission", "delete_omission"],
    },
    EventTypeCategory {
        key: "registered_political_group",
        event_types: &[
            "create_registered_political_group",
            "update_registered_political_group",
            "delete_registered_political_group",
        ],
    },
    EventTypeCategory {
        key: "system",
        event_types: &["login"],
    },
];

/// Filters for the CSB audit log list view.
#[derive(Debug, Default, Clone, serde::Deserialize, serde::Serialize)]
pub struct CsbAuditLogFilter {
    /// The stream can be:
    /// - `None` to show the CSB main stream
    /// - a UUID string to show that import stream
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
}

impl CsbAuditLogFilter {
    pub fn as_query_suffix(&self) -> String {
        filter_query_suffix(self)
    }

    pub fn is_active(&self) -> bool {
        self.stream.as_deref().is_some_and(|s| !s.is_empty())
            || self.event_type.as_deref().is_some_and(|s| !s.is_empty())
            || self.search.as_deref().is_some_and(|s| !s.is_empty())
    }
}

#[derive(Template)]
#[template(path = "csb/audit_log/pages/list.html")]
struct CsbAuditLogTemplate {
    entries: Vec<CsbAuditLogEntry>,
    pagination: crate::pagination::PaginationInfo,
    filter: CsbAuditLogFilter,
    /// Import streams available for filtering: (stream_id, label).
    import_streams: Vec<(StreamId, String)>,
    event_types_by_category: &'static [EventTypeCategory],
    /// Current list URL (page + filters), used so the detail overlay can
    /// return to the same view when closed.
    return_url: String,
}

fn filter_events<'a, E: Event + crate::HasCsbUser + 'a>(
    iter: impl DoubleEndedIterator<Item = &'a StoreEvent<E>> + 'a,
    stream_id: StreamId,
    label: String,
    locale: Locale,
    event_type: Option<&'a str>,
    search: Option<&'a str>,
) -> impl Iterator<Item = CsbAuditLogEntry> + 'a {
    iter.rev()
        .filter(move |event| {
            event_type.is_none_or(|et| event.payload.category() == et || event.payload.key() == et)
        })
        .map(move |event| CsbAuditLogEntry::from_event(event, stream_id, label.clone(), locale))
        .filter(move |e| search.is_none_or(|q| e.matches_search(q)))
}

/// Collects the filtered audit log entries of the selected stream: the import
/// stream the `stream` filter points at, or the committee main stream.
fn collect_entries(
    import_stores: &[crate::projection::CsbStream],
    main_store: &CsbMainStore,
    filter: &CsbAuditLogFilter,
    locale: Locale,
) -> Result<Vec<CsbAuditLogEntry>, AppError> {
    let active_stream = filter.stream.as_deref().filter(|s| !s.is_empty());
    let active_event_type = filter.event_type.as_deref().filter(|s| !s.is_empty());
    let active_search = filter.search.as_deref().filter(|s| !s.is_empty());

    let entries = if let Some(stream_id) = active_stream {
        let store = import_stores
            .iter()
            .find(|s| s.stream_id.to_string() == stream_id)
            .ok_or(AppError::GenericNotFound)?;

        filter_events(
            store.data.read().events.iter(),
            store.stream_id,
            store.get_appellation_with_deleted_label(
                crate::projection::WithCorrections::All,
                locale,
            ),
            locale,
            active_event_type,
            active_search,
        )
        .collect()
    } else {
        filter_events(
            main_store.data.read().events.iter(),
            main_store.stream_id,
            trans!("audit_log.filter.csb_main_stream", locale),
            locale,
            active_event_type,
            active_search,
        )
        .collect()
    };

    Ok(entries)
}

pub async fn csb_audit_log<S: AppRequestState>(
    _: CsbAuditLogPath,
    context: CsbContext,
    main_store: CsbMainStore,
    State(state): State<S>,
    pagination: Pagination,
    Query(filter): Query<CsbAuditLogFilter>,
) -> Result<impl IntoResponse, AppError> {
    let locale = context.session.locale;
    let import_stores = state
        .csb_store_registry()
        .stores_for_election(context.election)
        .await?;

    // Build a short label for each import stream from its import event
    let import_stream_labels: Vec<(StreamId, String)> = import_stores
        .iter()
        .map(|store| {
            (
                store.stream_id,
                store.get_appellation_with_deleted_label(
                    crate::projection::WithCorrections::All,
                    locale,
                ),
            )
        })
        .collect();

    let all_entries = collect_entries(&import_stores, &main_store, &filter, locale)?;

    let total = all_entries.len();
    let pagination = Pagination {
        per_page: PER_PAGE,
        ..pagination
    }
    .set_total(total);

    let entries = all_entries
        .into_iter()
        .skip(pagination.offset())
        .take(pagination.limit())
        .collect();

    let return_url = format!(
        "{}{}{}",
        CsbAuditLogPath,
        pagination.url(pagination.page, pagination.per_page),
        filter.as_query_suffix()
    );

    Ok(HtmlTemplate(
        CsbAuditLogTemplate {
            entries,
            pagination,
            filter,
            import_streams: import_stream_labels,
            event_types_by_category: EVENT_TYPES_BY_CATEGORY,
            return_url,
        },
        context,
    ))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::{projection::WithCorrections, structs::common::Appellation};
    use axum::{
        extract::{Query, State},
        http::StatusCode,
        response::IntoResponse,
    };

    use crate::{
        AppError, AppState, CsbAction, CsbContext, CsbMainAction, CsbMainStore, CsbUser,
        ElectionConfig, StreamId,
        csb::audit_log::pages::CsbAuditLogPath,
        pagination::Pagination,
        structs::csb::{Omission, OmissionCategory},
        test_utils::response_body_string,
    };

    fn no_filter() -> Query<CsbAuditLogFilter> {
        Query(CsbAuditLogFilter::default())
    }

    async fn call(
        main_store: CsbMainStore,
        state: AppState,
        filter: Query<CsbAuditLogFilter>,
    ) -> Result<axum::response::Response, AppError> {
        Ok(csb_audit_log(
            CsbAuditLogPath,
            CsbContext::new_test(),
            main_store,
            State(state),
            Pagination::default(),
            filter,
        )
        .await?
        .into_response())
    }

    #[tokio::test]
    async fn renders_empty_audit_log() -> Result<(), AppError> {
        let response = call(
            CsbMainStore::new_for_test(),
            AppState::new_for_tests().await,
            no_filter(),
        )
        .await?;

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Audit log"));
        // Should show empty message, not a table
        assert!(!body.contains("<table"));

        Ok(())
    }

    #[tokio::test]
    async fn renders_main_stream_events() -> Result<(), AppError> {
        let main_store = CsbMainStore::new_for_test();
        main_store
            .update(CsbMainAction::Login.by(CsbUser::Developer))
            .await?;

        let response = call(main_store, AppState::new_for_tests().await, no_filter()).await?;

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("<table"));
        assert!(body.contains("<td>Signed in</td>"));
        assert!(body.contains("<td>Main CSB stream</td>"));

        Ok(())
    }

    #[tokio::test]
    async fn renders_import_stream_events() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let import_stream_id = StreamId::new();
        let csb_store = state
            .csb_store_for_stream(import_stream_id, ElectionConfig::EK27)
            .await?;
        csb_store
            .update(CsbAction::SetFinished(true).by(CsbUser::new_test()))
            .await?;

        let response = call(
            CsbMainStore::new_for_test(),
            state,
            Query(CsbAuditLogFilter {
                stream: Some(import_stream_id.to_string()),
                ..Default::default()
            }),
        )
        .await?;

        let body = response_body_string(response).await;
        assert!(body.contains("<td>Set finished state</td>"));

        Ok(())
    }

    #[tokio::test]
    async fn renders_import_stream_events_from_deleted_stream() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let import_stream_id = StreamId::new();
        let csb_store = state
            .csb_store_for_stream(import_stream_id, ElectionConfig::EK27)
            .await?;
        let mut pg = csb_store.get_political_group(WithCorrections::All);
        pg.appellation = Appellation::from_str("Test Partij").ok();
        csb_store.set_political_group(pg);
        csb_store
            .update(CsbAction::Delete.by(CsbUser::new_test()))
            .await?;

        let response = call(
            CsbMainStore::new_for_test(),
            state,
            Query(CsbAuditLogFilter {
                stream: Some(import_stream_id.to_string()),
                ..Default::default()
            }),
        )
        .await?;

        let body = response_body_string(response).await;
        assert!(
            body.chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>()
                .contains(">TestPartij(deleted)</option>")
        );

        Ok(())
    }

    #[tokio::test]
    async fn shows_events_newest_first() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let import_stream_id = StreamId::new();
        let csb_store = state
            .csb_store_for_stream(import_stream_id, ElectionConfig::EK27)
            .await?;
        csb_store
            .update(CsbAction::SetFinished(true).by(CsbUser::new_test()))
            .await?;
        csb_store
            .update(
                CsbAction::CreateOmission(Omission::new(
                    OmissionCategory::PoliticalGroup,
                    "test".parse().unwrap(),
                    "test".parse().unwrap(),
                    Some("test".parse().unwrap()),
                ))
                .by(CsbUser::new_test()),
            )
            .await?;

        let response = call(
            CsbMainStore::new_for_test(),
            state,
            Query(CsbAuditLogFilter {
                stream: Some(import_stream_id.to_string()),
                ..Default::default()
            }),
        )
        .await?;

        let body = response_body_string(response).await;
        let finished_pos = body
            .find("<td>Set finished state</td>")
            .expect("set finished event");
        let omission_pos = body
            .find("<td>Created omission</td>")
            .expect("create omission event");
        assert!(
            omission_pos < finished_pos,
            "newer event (create omission) should appear before older event (set finished)"
        );

        Ok(())
    }

    #[tokio::test]
    async fn paginates_results() -> Result<(), AppError> {
        let main_store = CsbMainStore::new_for_test();
        for _ in 0..PER_PAGE + 5 {
            main_store
                .update(CsbMainAction::Login.by(CsbUser::Developer))
                .await?;
        }

        let response = call(main_store, AppState::new_for_tests().await, no_filter()).await?;

        let body = response_body_string(response).await;
        assert!(body.contains("Pagination"));
        let row_count = body.matches("<td>Signed in</td>").count();
        assert_eq!(row_count, PER_PAGE);

        Ok(())
    }

    #[tokio::test]
    async fn filters_by_main_stream() -> Result<(), AppError> {
        let main_store = CsbMainStore::new_for_test();
        main_store
            .update(CsbMainAction::Login.by(CsbUser::Developer))
            .await?;

        let state = AppState::new_for_tests().await;
        let csb_store = state
            .csb_store_for_stream(StreamId::new(), ElectionConfig::EK27)
            .await?;
        csb_store
            .update(CsbAction::SetFinished(true).by(CsbUser::new_test()))
            .await?;

        let response = call(main_store, state, no_filter()).await?;

        let body = response_body_string(response).await;
        assert!(body.contains("<td>Signed in</td>"));
        assert!(!body.contains("<td>Set finished state</td>"));

        Ok(())
    }

    #[tokio::test]
    async fn filters_by_import_stream() -> Result<(), AppError> {
        let main_store = CsbMainStore::new_for_test();
        main_store
            .update(CsbMainAction::Login.by(CsbUser::Developer))
            .await?;

        let state = AppState::new_for_tests().await;
        let import_stream_id = StreamId::new();
        let csb_store = state
            .csb_store_for_stream(import_stream_id, ElectionConfig::EK27)
            .await?;
        csb_store
            .update(CsbAction::SetFinished(true).by(CsbUser::new_test()))
            .await?;

        let response = call(
            main_store,
            state,
            Query(CsbAuditLogFilter {
                stream: Some(import_stream_id.to_string()),
                ..Default::default()
            }),
        )
        .await?;

        let body = response_body_string(response).await;
        assert!(!body.contains("<td>Signed in</td>"));
        assert!(body.contains("<td>Set finished state</td>"));

        Ok(())
    }

    #[tokio::test]
    async fn filters_by_event_type() -> Result<(), AppError> {
        let main_store = CsbMainStore::new_for_test();
        main_store
            .update(CsbMainAction::Login.by(CsbUser::Developer))
            .await?;

        let state = AppState::new_for_tests().await;
        let import_stream_id = StreamId::new();
        let csb_store = state
            .csb_store_for_stream(import_stream_id, ElectionConfig::EK27)
            .await?;
        csb_store
            .update(CsbAction::SetFinished(true).by(CsbUser::new_test()))
            .await?;

        let response = call(
            main_store,
            state,
            Query(CsbAuditLogFilter {
                stream: Some(import_stream_id.to_string()),
                event_type: Some("set_finished".to_string()),
                ..Default::default()
            }),
        )
        .await?;

        let body = response_body_string(response).await;
        assert!(body.contains("<td>Set finished state</td>"));
        assert!(!body.contains("<td>Signed in</td>"));

        Ok(())
    }

    #[tokio::test]
    async fn searches_by_description() -> Result<(), AppError> {
        let main_store = CsbMainStore::new_for_test();
        main_store
            .update(CsbMainAction::Login.by(CsbUser::Developer))
            .await?;

        let state = AppState::new_for_tests().await;
        let csb_store = state
            .csb_store_for_stream(StreamId::new(), ElectionConfig::EK27)
            .await?;
        csb_store
            .update(CsbAction::SetFinished(true).by(CsbUser::new_test()))
            .await?;

        let response = call(
            main_store,
            state,
            Query(CsbAuditLogFilter {
                search: Some("Developer".to_string()),
                ..Default::default()
            }),
        )
        .await?;

        let body = response_body_string(response).await;
        assert!(body.contains("<td>Signed in</td>"));
        assert!(!body.contains("<td>Set finished state</td>"));

        Ok(())
    }

    #[tokio::test]
    async fn reset_button_only_shown_when_filter_active() -> Result<(), AppError> {
        let main_store = CsbMainStore::new_for_test();
        main_store
            .update(CsbMainAction::Login.by(CsbUser::Developer))
            .await?;

        let state = AppState::new_for_tests().await;

        let response = call(main_store.clone(), state.clone(), no_filter()).await?;
        let body = response_body_string(response).await;
        assert!(!body.contains("/csb/audit-log\" class=\"button secondary\">"));

        let response = call(
            main_store,
            state,
            Query(CsbAuditLogFilter {
                event_type: Some("system".to_string()),
                ..Default::default()
            }),
        )
        .await?;
        let body = response_body_string(response).await;
        assert!(body.contains("/csb/audit-log\" class=\"button secondary\">"));

        Ok(())
    }

    #[tokio::test]
    async fn detail_links_preserve_filter_as_redirect() -> Result<(), AppError> {
        let main_store = CsbMainStore::new_for_test();
        main_store
            .update(CsbMainAction::Login.by(CsbUser::Developer))
            .await?;

        let response = call(
            main_store,
            AppState::new_for_tests().await,
            Query(CsbAuditLogFilter {
                event_type: Some("login".to_string()),
                ..Default::default()
            }),
        )
        .await?;

        let body = response_body_string(response).await;
        // The detail link carries a redirect_to pointing back at the current,
        // filtered list view so closing the overlay restores it.
        assert!(body.contains("redirect_to="));
        let encoded = urlencoding::encode("/csb/audit-log?per_page=20&event_type=login");
        assert!(
            body.contains(encoded.as_ref()),
            "expected detail link to encode the filtered return URL"
        );

        Ok(())
    }
}
