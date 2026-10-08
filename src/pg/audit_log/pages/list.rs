use askama::Template;
use axum::{extract::Query, response::IntoResponse};

use crate::{
    AppError, Context, Event, HtmlTemplate, PgStore,
    audit_log::{AuditLogEntry, paths::AuditLogPath},
    filters,
    pagination::Pagination,
    structs::audit_log::EventTypeCategory,
    utils::filter_query_suffix,
};

const PER_PAGE: usize = 20;

/// Event type categories grouped with their specific event keys, used by the
/// filter dropdown to render `<optgroup>`s with fine-grained `<option>`s.
///
/// Category label translations (referenced dynamically in the template):
/// trans!("audit_log.filter.category.political_group", _)
/// trans!("audit_log.filter.category.person", _)
/// trans!("audit_log.filter.category.candidate_list", _)
/// trans!("audit_log.filter.category.name_authorisation", _)
/// trans!("audit_log.filter.category.list_submitter", _)
/// trans!("audit_log.filter.category.substitute_submitter", _)
/// trans!("audit_log.filter.category.system", _)
/// trans!("audit_log.filter.category.import", _)
pub const EVENT_TYPES_BY_CATEGORY: &[EventTypeCategory] = &[
    EventTypeCategory {
        key: "political_group",
        event_types: &["update_political_group"],
    },
    EventTypeCategory {
        key: "person",
        event_types: &[
            "create_person",
            "update_person",
            "update_person_address",
            "update_person_representative",
            "delete_person",
        ],
    },
    EventTypeCategory {
        key: "candidate_list",
        event_types: &[
            "create_candidate_list",
            "update_candidate_list_districts",
            "update_candidate_list_order",
            "add_candidate_to_list",
            "remove_candidate_from_list",
            "delete_candidate_list",
        ],
    },
    EventTypeCategory {
        key: "name_authorisation",
        event_types: &[
            "create_name_authorisation",
            "update_name_authorisation",
            "delete_name_authorisation",
        ],
    },
    EventTypeCategory {
        key: "list_submitter",
        event_types: &["update_list_submitter"],
    },
    EventTypeCategory {
        key: "substitute_submitter",
        event_types: &[
            "create_substitute_submitter",
            "update_substitute_submitter",
            "delete_substitute_submitter",
        ],
    },
    EventTypeCategory {
        key: "system",
        event_types: &[
            "login",
            "logout",
            "developer_login",
            "download_file",
            "hide_download_warning",
            "export_csv",
            "import_csv",
        ],
    },
    EventTypeCategory {
        key: "import",
        event_types: &["import"],
    },
];

#[derive(Debug, Default, Clone, serde::Deserialize, serde::Serialize)]
pub struct AuditLogFilter {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
}

impl AuditLogFilter {
    /// Build a query string fragment (with leading `&`) for the active filters.
    pub fn as_query_suffix(&self) -> String {
        filter_query_suffix(self)
    }

    pub fn is_active(&self) -> bool {
        self.event_type.as_deref().is_some_and(|s| !s.is_empty())
            || self.search.as_deref().is_some_and(|s| !s.is_empty())
    }
}

#[derive(Template)]
#[template(path = "pg/audit_log/pages/list.html")]
struct AuditLogTemplate {
    entries: Vec<AuditLogEntry>,
    pagination: crate::pagination::PaginationInfo,
    filter: AuditLogFilter,
    event_types_by_category: &'static [EventTypeCategory],
}

pub async fn audit_log(
    _: AuditLogPath,
    context: Context,
    store: PgStore,
    pagination: Pagination,
    Query(filter): Query<AuditLogFilter>,
) -> Result<impl IntoResponse, AppError> {
    let locale = context.session.locale;

    let active_event_type = filter.event_type.as_deref().filter(|s| !s.is_empty());
    let active_search = filter.search.as_deref().filter(|s| !s.is_empty());

    // Convert all events to entries, applying event_type filter early
    let all_entries: Vec<AuditLogEntry> = store
        .get_events()
        .into_iter()
        .rev()
        .filter(|event| {
            active_event_type
                .map(|et| event.payload.category() == et || event.payload.key() == et)
                .unwrap_or(true)
        })
        .map(|event| AuditLogEntry::new(event, locale))
        .filter(|entry| {
            active_search
                .map(|q| entry.matches_search(q))
                .unwrap_or(true)
        })
        .collect();

    let total = all_entries.len();

    let pagination = Pagination {
        per_page: PER_PAGE,
        ..pagination
    }
    .set_total(total);

    let entries: Vec<AuditLogEntry> = all_entries
        .into_iter()
        .skip(pagination.offset())
        .take(pagination.limit())
        .collect();

    Ok(HtmlTemplate(
        AuditLogTemplate {
            entries,
            pagination,
            filter,
            event_types_by_category: EVENT_TYPES_BY_CATEGORY,
        },
        context,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppError, Context, PgStore,
        pagination::Pagination,
        structs::persons::PersonId,
        test_utils::{response_body_string, sample_person, sample_political_group},
    };
    use axum::{extract::Query, http::StatusCode, response::IntoResponse};

    fn no_filter() -> Query<AuditLogFilter> {
        Query(AuditLogFilter::default())
    }

    /// One event per `PgEvent` variant, so the table cannot silently miss one.
    #[test]
    #[allow(clippy::too_many_lines)] // one constructor per variant, no logic
    fn filter_table_covers_every_event() {
        use crate::{
            PgEvent, StreamId,
            structs::{
                candidate_lists::CandidateListId, list_submitters::ListSubmitterId,
                name_authorisations::NameAuthorisationId,
            },
            test_utils::{sample_candidate_list, sample_list_submitter, sample_name_authorisation},
        };

        let person = sample_person(PersonId::new());
        let person_id = person.id;
        let list_id = CandidateListId::new();
        let submitter = sample_list_submitter(ListSubmitterId::new());
        let authorisation = sample_name_authorisation(NameAuthorisationId::new());

        let events = vec![
            PgEvent::UpdatePoliticalGroup(sample_political_group()),
            PgEvent::CreatePerson(person.clone()),
            PgEvent::CreatePersonPersonalData {
                person_id,
                name: person.name.clone(),
                personal_data: person.personal_data.clone(),
            },
            PgEvent::UpdatePerson(person.clone()),
            PgEvent::UpdatePersonPersonalData {
                person_id,
                name: person.name.clone(),
                personal_data: person.personal_data.clone(),
            },
            PgEvent::UpdatePersonAddress {
                person_id,
                address: person.address.clone(),
            },
            PgEvent::UpdatePersonRepresentative {
                person_id,
                representative: None,
            },
            PgEvent::DeletePerson { person_id },
            PgEvent::CreateCandidateList(sample_candidate_list(list_id)),
            PgEvent::UpdateCandidateListDistricts {
                list_id,
                electoral_districts: Default::default(),
            },
            PgEvent::UpdateCandidateListOrder {
                list_id,
                candidates: vec![person_id],
            },
            PgEvent::AddCandidateToCandidateList { list_id, person_id },
            PgEvent::RemoveCandidateFromCandidateList { list_id, person_id },
            PgEvent::DeleteCandidateList(list_id),
            PgEvent::CreateNameAuthorisation(authorisation.clone()),
            PgEvent::UpdateNameAuthorisation(authorisation.clone()),
            PgEvent::DeleteNameAuthorisation(authorisation.id),
            PgEvent::UpdateListSubmitter(submitter.clone()),
            PgEvent::CreateSubstituteSubmitter(submitter.clone()),
            PgEvent::UpdateSubstituteSubmitter(submitter.clone()),
            PgEvent::DeleteSubstituteSubmitter {
                substitute_submitter_id: submitter.id,
            },
            PgEvent::DeveloperLogin {
                stream_id: StreamId::new(),
            },
            PgEvent::Login,
            PgEvent::Logout,
            PgEvent::DownloadFile {
                file_name: "h1.pdf".to_string(),
                download_path: "/download/h1".to_string(),
            },
            PgEvent::HideDownloadWarning,
            PgEvent::ExportCsv {
                file_name: "export.csv".to_string(),
                file_size: 1,
                list_id,
            },
            PgEvent::ImportCandidates {
                list_id,
                file_name: "import.csv".to_string(),
                file_size: 1,
                created_persons: vec![],
                updated_persons: vec![],
                candidates: vec![],
            },
            PgEvent::Import { hash: [0; 32] },
        ];

        crate::structs::audit_log::assert_covers(EVENT_TYPES_BY_CATEGORY, events);
    }

    #[tokio::test]
    async fn renders_empty_audit_log() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        let response = audit_log(
            AuditLogPath {},
            Context::new_test_without_db(),
            store,
            Pagination::default(),
            no_filter(),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Audit log"));
        // Should show empty message, not a table
        assert!(!body.contains("<table"));

        Ok(())
    }

    #[tokio::test]
    async fn renders_events_in_table() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let person = sample_person(PersonId::new());
        person.create(&store).await?;

        let response = audit_log(
            AuditLogPath {},
            Context::new_test_without_db(),
            store,
            Pagination::default(),
            no_filter(),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("<table"));
        assert!(body.contains("<td>Created person</td>"));
        assert!(body.contains(&person.name.display()));

        Ok(())
    }

    #[tokio::test]
    async fn shows_events_in_reverse_chronological_order() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let pg = sample_political_group();
        pg.update(&store).await?;
        let person = sample_person(PersonId::new());
        person.create(&store).await?;

        let response = audit_log(
            AuditLogPath {},
            Context::new_test_without_db(),
            store,
            Pagination::default(),
            no_filter(),
        )
        .await
        .unwrap()
        .into_response();

        let body = response_body_string(response).await;
        let person_pos = body.find("<td>Created person</td>").expect("person event");
        let pg_pos = body
            .find("<td>Updated political group</td>")
            .expect("pg event");
        assert!(
            person_pos < pg_pos,
            "newest event (create person) should appear before older event (update pg)"
        );

        Ok(())
    }

    #[tokio::test]
    async fn paginates_results() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        // Create more events than PER_PAGE
        for _ in 0..PER_PAGE + 5 {
            let person = sample_person(PersonId::new());
            person.create(&store).await?;
        }

        let response = audit_log(
            AuditLogPath {},
            Context::new_test_without_db(),
            store,
            Pagination::default(),
            no_filter(),
        )
        .await
        .unwrap()
        .into_response();

        let body = response_body_string(response).await;
        // Should show pagination controls
        assert!(body.contains("Pagination"));
        // Count table rows - should be PER_PAGE
        let row_count = body.matches("<td>Created person</td>").count();
        assert_eq!(row_count, PER_PAGE);

        Ok(())
    }

    #[tokio::test]
    async fn filters_by_event_type() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let pg = sample_political_group();
        pg.update(&store).await?;
        let person = sample_person(PersonId::new());
        person.create(&store).await?;

        let response = audit_log(
            AuditLogPath {},
            Context::new_test_without_db(),
            store,
            Pagination::default(),
            Query(AuditLogFilter {
                event_type: Some("person".to_string()),
                search: None,
            }),
        )
        .await
        .unwrap()
        .into_response();

        let body = response_body_string(response).await;
        assert!(body.contains("<td>Created person</td>"));
        assert!(!body.contains("<td>Updated political group</td>"));

        Ok(())
    }

    #[tokio::test]
    async fn filters_by_specific_event_key() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let person = sample_person(PersonId::new());
        person.create(&store).await?;
        let other = sample_person(PersonId::new());
        other.create(&store).await?;
        other.delete(&store).await?;

        let response = audit_log(
            AuditLogPath {},
            Context::new_test_without_db(),
            store,
            Pagination::default(),
            Query(AuditLogFilter {
                event_type: Some("delete_person".to_string()),
                search: None,
            }),
        )
        .await
        .unwrap()
        .into_response();

        let body = response_body_string(response).await;
        assert!(body.contains("<td>Deleted person</td>"));
        assert!(!body.contains("<td>Created person</td>"));

        Ok(())
    }

    #[tokio::test]
    async fn reset_button_only_shown_when_filter_active() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let person = sample_person(PersonId::new());
        person.create(&store).await?;

        let response = audit_log(
            AuditLogPath {},
            Context::new_test_without_db(),
            store.clone(),
            Pagination::default(),
            no_filter(),
        )
        .await
        .unwrap()
        .into_response();

        let body = response_body_string(response).await;
        assert!(!body.contains("/audit-log\" class=\"button secondary\">Reset"));

        let response = audit_log(
            AuditLogPath {},
            Context::new_test_without_db(),
            store,
            Pagination::default(),
            Query(AuditLogFilter {
                event_type: Some("person".to_string()),
                search: None,
            }),
        )
        .await
        .unwrap()
        .into_response();

        let body = response_body_string(response).await;
        assert!(body.contains("/audit-log\" class=\"button secondary\">Reset"));

        Ok(())
    }

    /// In paper-corrections mode the audit log lists only the CSB's
    /// corrections, never the source stream's own events.
    #[tokio::test]
    async fn paper_corrections_mode_lists_only_csb_corrections() -> Result<(), AppError> {
        use crate::{CsbAction, CsbStore, StreamId, test_utils::sample_political_group};

        // Source stream with an event of its own.
        let source = PgStore::new_for_test();
        let person = sample_person(PersonId::new());
        person.create(&source).await?;

        // Import the source stream the way `do_import` does: the snapshot
        // excludes the source event log.
        let events = source.data.read().events.clone();
        let snapshot = crate::PgStoreData::snapshot_until(&events, usize::MAX);
        let csb_store = CsbStore::new_for_test();
        csb_store
            .update(CsbAction::Import {
                hash: [1; 32],
                source_stream_id: StreamId::new(),
                snapshot: Box::new(snapshot),
            })
            .await?;

        let store = csb_store.paper_corrections();
        sample_political_group().update(&store).await?;

        let response = audit_log(
            AuditLogPath {},
            Context::new_test_without_db(),
            store,
            Pagination::default(),
            no_filter(),
        )
        .await
        .unwrap()
        .into_response();

        let body = response_body_string(response).await;
        assert!(body.contains("<td>Updated political group</td>"));
        assert!(!body.contains("<td>Created person</td>"));
        // The import that seeds the corrections shows up as event #1.
        assert!(body.contains("<td>Imported political group</td>"));

        Ok(())
    }

    /// In paper-corrections mode the import (CSB event #1) is listed as a
    /// synthetic entry, so the numbering starts at 1 rather than 2.
    #[tokio::test]
    async fn paper_corrections_mode_lists_the_import_as_event_one() -> Result<(), AppError> {
        use crate::{CsbAction, CsbStore, StreamId, test_utils::sample_political_group};

        let csb_store = CsbStore::new_for_test();
        csb_store
            .update(CsbAction::Import {
                hash: [1; 32],
                source_stream_id: StreamId::new(),
                snapshot: Box::new(crate::PgStoreData::default()),
            })
            .await?;

        let store = csb_store.paper_corrections();
        sample_political_group().update(&store).await?;

        let response = audit_log(
            AuditLogPath {},
            Context::new_test_without_db(),
            store,
            Pagination::default(),
            no_filter(),
        )
        .await
        .unwrap()
        .into_response();

        let body = response_body_string(response).await;
        // Event #1 is the import; the first correction is event #2.
        assert!(body.contains("<td>1</td>"));
        assert!(body.contains("<td>Imported political group</td>"));
        assert!(body.contains("<td>2</td>"));

        Ok(())
    }

    /// The synthetic import entry honours the event-type filter: it appears
    /// under the "import" type and is excluded from unrelated types.
    #[tokio::test]
    async fn paper_corrections_import_entry_respects_event_type_filter() -> Result<(), AppError> {
        use crate::{CsbAction, CsbStore, StreamId, test_utils::sample_political_group};

        let csb_store = CsbStore::new_for_test();
        csb_store
            .update(CsbAction::Import {
                hash: [1; 32],
                source_stream_id: StreamId::new(),
                snapshot: Box::new(crate::PgStoreData::default()),
            })
            .await?;
        let store = csb_store.paper_corrections();
        sample_political_group().update(&store).await?;

        let filtered = |event_type: &str| {
            Query(AuditLogFilter {
                event_type: Some(event_type.to_string()),
                search: None,
            })
        };

        // Filtering by "import" keeps the import, drops the correction.
        let response = audit_log(
            AuditLogPath {},
            Context::new_test_without_db(),
            store.clone(),
            Pagination::default(),
            filtered("import"),
        )
        .await
        .unwrap()
        .into_response();
        let body = response_body_string(response).await;
        assert!(body.contains("<td>Imported political group</td>"));
        assert!(!body.contains("<td>Updated political group</td>"));

        // Filtering by an unrelated type drops the import.
        let response = audit_log(
            AuditLogPath {},
            Context::new_test_without_db(),
            store,
            Pagination::default(),
            filtered("political_group"),
        )
        .await
        .unwrap()
        .into_response();
        let body = response_body_string(response).await;
        assert!(!body.contains("<td>Imported political group</td>"));
        assert!(body.contains("<td>Updated political group</td>"));

        Ok(())
    }

    #[tokio::test]
    async fn searches_by_details() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let person = sample_person(PersonId::new());
        let name = person.name.display();
        person.create(&store).await?;
        let pg = sample_political_group();
        pg.update(&store).await?;

        let response = audit_log(
            AuditLogPath {},
            Context::new_test_without_db(),
            store,
            Pagination::default(),
            Query(AuditLogFilter {
                event_type: None,
                search: Some(name.clone()),
            }),
        )
        .await
        .unwrap()
        .into_response();

        let body = response_body_string(response).await;
        assert!(body.contains("<td>Created person</td>"));
        assert!(!body.contains("<td>Updated political group</td>"));

        Ok(())
    }
}
