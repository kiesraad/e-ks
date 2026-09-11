use crate::{
    core::ModelLocale, finalise::AllProblems, models::documents::DocumentData,
    structs::audit_log::FieldChange, utils::format_hash,
};
use askama::Template;
use axum::response::IntoResponse;

use crate::{
    AppError, Context, HtmlTemplate, Overlay, PgStore,
    audit_log::{
        AuditLogDetail, AuditLogPath,
        paths::{AuditLogDetailPath, AuditLogDownloadDocumentsPath},
    },
    filters,
};

#[derive(Template)]
#[template(path = "pg/audit_log/pages/detail.html")]
struct AuditLogDetailTemplate {
    detail: AuditLogDetail,
    download_path_nl: String,
    download_path_fry: String,
    is_downloadable_state: bool,
    frisian_export_allowed: bool,
    overlay: Overlay,
    hash: String,
}

pub async fn audit_log_detail(
    AuditLogDetailPath { event_id }: AuditLogDetailPath,
    context: Context,
    store: PgStore,
) -> Result<impl IntoResponse, AppError> {
    let events = store.get_events();
    let locale = context.session.locale;

    let base = store.imported_snapshot().unwrap_or_default();
    let detail = AuditLogDetail::compute(&base, &events, event_id, locale)
        .ok_or(AppError::GenericNotFound)?;

    let temp_store = create_temp_store(&store, event_id);
    let is_downloadable_state = AllProblems::find_all(&temp_store)?.models_downloadable();

    let hash = store
        .get_events()
        .iter()
        .find(|e| e.event_id == event_id)
        .ok_or(AppError::GenericNotFound)?
        .hash;

    Ok(HtmlTemplate(
        AuditLogDetailTemplate {
            detail,
            download_path_nl: AuditLogDownloadDocumentsPath {
                event_id,
                locale: ModelLocale::Nl,
            }
            .to_string(),
            download_path_fry: AuditLogDownloadDocumentsPath {
                event_id,
                locale: ModelLocale::Fry,
            }
            .to_string(),
            is_downloadable_state,
            frisian_export_allowed: context.election.frisian_export_allowed(),
            overlay: Overlay::default(),
            hash: format_hash(&hash, true),
        },
        context,
    ))
}

pub async fn audit_log_gen_documents(
    path @ AuditLogDownloadDocumentsPath { event_id, locale }: AuditLogDownloadDocumentsPath,
    context: Context,
    store: PgStore,
) -> Result<impl IntoResponse, AppError> {
    // Downloading documents is not part of paper corrections.
    if store.paper_corrections_stream_id().is_some() {
        return Err(AppError::Unauthorised);
    }

    let temp_store = create_temp_store(&store, event_id);

    if !AllProblems::find_all(&temp_store)?.models_downloadable() {
        return Err(AppError::IncompleteData(
            "Documents cannot be downloaded for this version",
        ));
    }
    store.check_download_limit()?;

    let (bundles, filename) = DocumentData::from_store_and_context(&temp_store, &context, locale)?;

    DocumentData::serve_download(bundles, filename, path.to_string(), &store, &temp_store).await
}

/// Replay the event stream up to and including `event_id` into a throwaway
/// in-memory store, so document state can be inspected as it was back then.
/// In paper-corrections mode the replay starts from the imported snapshot the
/// corrections were applied on top of.
fn create_temp_store(store: &PgStore, event_id: usize) -> PgStore {
    let temp_store = PgStore::new_for_temp_stream(store.election);
    if let Some(imported) = store.imported_snapshot() {
        *temp_store.data.write() = imported;
    }

    store
        .get_events()
        .iter()
        .take_while(|e| e.event_id <= event_id)
        .for_each(|e| temp_store.apply_event(e.clone()));

    temp_store
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppError, Context, PgStore,
        structs::persons::PersonId,
        test_utils::{response_body_string, sample_person},
    };
    use axum::{http::StatusCode, response::IntoResponse};

    #[tokio::test]
    async fn renders_detail_for_create_event() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let person = sample_person(PersonId::new());
        person.create(&store).await?;

        let response = audit_log_detail(
            AuditLogDetailPath { event_id: 1 },
            Context::new_test_without_db(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Created person"));
        assert!(body.contains("diff-table"));

        Ok(())
    }

    #[tokio::test]
    async fn renders_entity_ref_link_and_description_for_id_diff() -> Result<(), AppError> {
        use crate::test_utils::{sample_candidate_list, sample_person_with_last_name};

        let store = PgStore::new_for_test();

        let person = sample_person_with_last_name(PersonId::new(), "Janssen");
        let person_id = person.id;
        let person_name = person.name.display();
        person.create(&store).await?;

        let list_id = crate::structs::candidate_lists::CandidateListId::new();
        let list = sample_candidate_list(list_id);
        list.create(&store).await?;

        let mut list = store.get_candidate_list(list_id)?;
        list.append_candidate(&store, person_id).await?;

        let events = store.get_events();
        let target_event_id = events.last().unwrap().event_id;

        let response = audit_log_detail(
            AuditLogDetailPath {
                event_id: target_event_id,
            },
            Context::new_test_without_db(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        let full_id = person_id.to_string();
        assert!(
            body.contains(&format!("<abbr title=\"{full_id}\">")),
            "expected abbreviated link with full id in title; body: {body}"
        );
        assert!(
            body.contains(&format!("/audit-log?search={full_id}")),
            "expected link href to filter audit log by person id"
        );
        assert!(
            body.contains(&person_name),
            "expected person appellation to appear next to the abbreviated id"
        );

        Ok(())
    }

    /// Build a paper-corrections store carrying one CSB correction.
    async fn paper_corrections_store() -> Result<PgStore, AppError> {
        let store = crate::test_utils::paper_corrections_store().await?;
        sample_person(PersonId::new()).create(&store).await?;
        Ok(store)
    }

    #[tokio::test]
    async fn paper_corrections_mode_hides_document_download() -> Result<(), AppError> {
        // Normal mode renders the download button.
        let store = PgStore::new_for_test();
        sample_person(PersonId::new()).create(&store).await?;
        let response = audit_log_detail(
            AuditLogDetailPath { event_id: 1 },
            Context::new_test_from_store(&store),
            store,
        )
        .await
        .unwrap()
        .into_response();
        let body = response_body_string(response).await;
        assert!(body.contains("icon-download"));

        // Paper-corrections mode hides it.
        let store = paper_corrections_store().await?;
        let event_id = store.get_events().last().unwrap().event_id;
        let response = audit_log_detail(
            AuditLogDetailPath { event_id },
            Context::new_test_from_store(&store),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(!body.contains("icon-download"));

        Ok(())
    }

    #[tokio::test]
    async fn paper_corrections_mode_rejects_document_download() -> Result<(), AppError> {
        let store = paper_corrections_store().await?;
        let event_id = store.get_events().last().unwrap().event_id;

        let result = audit_log_gen_documents(
            AuditLogDownloadDocumentsPath {
                event_id,
                locale: ModelLocale::Nl,
            },
            Context::new_test_from_store(&store),
            store,
        )
        .await;

        assert!(matches!(result, Err(AppError::Unauthorised)));

        Ok(())
    }

    /// A correction to an imported entity diffs against the imported
    /// snapshot: the old values are the imported paper values, not blanks.
    #[tokio::test]
    async fn paper_corrections_detail_diffs_against_the_imported_snapshot() -> Result<(), AppError>
    {
        use crate::{CsbAction, CsbStore, PgEvent, StreamId};

        // Source stream with an imported person.
        let source = PgStore::new_for_test();
        let person = sample_person(PersonId::new());
        let person_id = person.id;
        person.create(&source).await?;

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

        // Correct the imported person's first name.
        let store = csb_store.paper_corrections();
        let mut corrected = store.get_person(person_id)?;
        corrected.name.first_name = Some("Gecorrigeerd".parse().unwrap());
        store.update(PgEvent::UpdatePerson(corrected)).await?;

        let event_id = store.get_events().last().unwrap().event_id;
        let response = audit_log_detail(
            AuditLogDetailPath { event_id },
            Context::new_test_from_store(&store),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("diff-table"));
        // Old value from the imported snapshot, new value from the correction.
        assert!(body.contains("Henk"));
        assert!(body.contains("Gecorrigeerd"));

        Ok(())
    }

    #[tokio::test]
    async fn returns_not_found_for_unknown_event() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        let result = audit_log_detail(
            AuditLogDetailPath { event_id: 999 },
            Context::new_test_without_db(),
            store,
        )
        .await;

        assert!(result.is_err());

        Ok(())
    }

    /// Number of consent declarations (h9) in a zip documents response.
    async fn h9_entry_count(response: axum::response::Response) -> usize {
        crate::test_utils::zip_entry_names(response)
            .await
            .iter()
            .filter(|filename| filename.starts_with("h9-instemmingsverklaringen/"))
            .count()
    }

    #[tokio::test]
    async fn audit_log_gen_documents_returns_zip_response() -> Result<(), AppError> {
        use axum::{http::StatusCode, response::IntoResponse};

        use crate::test_utils::{self, assert_zip_response_headers, setup_documents_test_state};

        let (store, _, context) =
            setup_documents_test_state(1, 5, true, true, crate::ElectionConfig::EK27).await?;
        // the setup does not include political group as an event but directly injects it in the data
        // hence we insert it as an event here
        test_utils::sample_political_group().create(&store).await?;

        // create two remove candidate events
        let mut lists = store.get_candidate_lists();
        let (c1, c2) = (lists[0].candidates[0], lists[0].candidates[1]);
        lists[0].remove_candidate(&store, c1).await?;
        lists[0].remove_candidate(&store, c2).await?;

        let current_event_id = store.current_event_id();

        let response_4_candidates = audit_log_gen_documents(
            AuditLogDownloadDocumentsPath {
                locale: ModelLocale::Nl,
                event_id: current_event_id - 1,
            },
            context.clone(),
            store.clone(),
        )
        .await?
        .into_response();

        let response_5_candidates = audit_log_gen_documents(
            AuditLogDownloadDocumentsPath {
                locale: ModelLocale::Nl,
                event_id: current_event_id - 2,
            },
            context,
            store.clone(),
        )
        .await?
        .into_response();

        assert_eq!(response_4_candidates.status(), StatusCode::OK);
        assert_zip_response_headers(response_4_candidates.headers());

        // check if two download events are present
        let download_event_count = store
            .get_events()
            .iter()
            .filter(|e| matches!(e.payload, crate::PgEvent::DownloadFile { .. }))
            .count();
        assert_eq!(2, download_event_count);

        assert_eq!(4, h9_entry_count(response_4_candidates).await);
        assert_eq!(5, h9_entry_count(response_5_candidates).await);

        Ok(())
    }
}
