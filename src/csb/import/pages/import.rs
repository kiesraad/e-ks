use std::{
    collections::HashSet,
    sync::{LazyLock, Mutex},
    time::Duration,
};

use askama::Template;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use crate::{
    AppError, AppRequestState, Context, CsbAction, CsbContext, CsbStore, CsbUser, Form,
    HtmlTemplate, Locale, PgStoreData, StreamId,
    csb::examination::{CsbExaminationOverviewPath, CsbPoliticalGroupPath},
    filters,
    projection::WithCorrections,
    redirect_success,
    structs::{
        brp::{BRP_BSN_BATCH_SIZE, BrpClient, BrpStatus},
        persons::Person,
    },
    trans,
    utils::parse_hash_prefix,
};

use super::{CsbCreateEmptyPath, CsbImportPath};

const BRP_COURTESY_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Template)]
#[template(path = "csb/import/pages/import.html")]
struct CsbImportTemplate {
    hash: String,
    error: Option<String>,
    warning: Option<String>,
}

fn render_import(
    context: CsbContext,
    hash: String,
    error: Option<String>,
    warning: Option<String>,
) -> Response {
    HtmlTemplate(
        CsbImportTemplate {
            hash,
            error,
            warning,
        },
        context,
    )
    .into_response()
}

/// Render the placeholder import page.
pub async fn import(_: CsbImportPath, context: CsbContext) -> Result<Response, AppError> {
    Ok(render_import(context, String::new(), None, None))
}

/// Form payload for the import page: the chain hash of the package to import,
/// and the hash for which a duplicate-import warning was already confirmed.
#[derive(Debug, Deserialize)]
pub struct ImportForm {
    pub hash: String,
    pub confirmed_hash: Option<String>,
}

/// Outcome of an import attempt that did not fail outright.
enum ImportOutcome {
    Imported(Response),
    /// The source stream was already imported into a live CSB store carrying
    /// this appellation; the user must confirm before importing again.
    AlreadyImported {
        appellation: String,
    },
}

/// Import the package identified by the submitted chain hash.
pub async fn import_submit<S: AppRequestState>(
    _: CsbImportPath,
    State(state): State<S>,
    context: CsbContext,
    Form(form): Form<ImportForm>,
) -> Result<Response, AppError> {
    let hash = form.hash.clone();
    let locale = context.session.locale;
    let user = context.user()?;
    match do_import(&state, form, user, locale).await {
        Ok(ImportOutcome::Imported(response)) => Ok(response),
        Ok(ImportOutcome::AlreadyImported { appellation }) => Ok(render_import(
            context,
            hash,
            None,
            Some(trans!(
                "csb.import.warning.already_imported",
                locale,
                appellation
            )),
        )),
        Err(AppError::UserError(msg)) => Ok(render_import(context, hash, Some(msg), None)),
        Err(AppError::AmbiguousHash) => Ok(render_import(
            context,
            hash,
            Some(trans!("csb.import.error.ambiguous_hash", locale)),
            None,
        )),
        Err(e) => Err(e),
    }
}

/// Locates the political-group event whose hash matches the entry, replays that
/// stream up to the event into an [`PgStoreData`] snapshot (its event log
/// excluded), and records the snapshot in a [`CsbAction::Import`] persisted under
/// a fresh CSB stream keyed on the source election. The source `stream_id` is
/// carried on the event for reference; it is never reused as the CSB partition,
/// which would collide with the PG stream's own events there.
async fn do_import<S: AppRequestState>(
    state: &S,
    form: ImportForm,
    user: CsbUser,
    locale: Locale,
) -> Result<ImportOutcome, AppError> {
    let hash_prefix = parse_hash_prefix(&form.hash)
        .ok_or_else(|| AppError::UserError(trans!("csb.import.error.invalid_hash", locale)))?;

    let (source_stream_id, source_election, event_id) = state
        .store_registry()
        .persistence()
        .find_event_by_hash_prefix(&hash_prefix)
        .await?
        .ok_or_else(|| AppError::UserError(trans!("csb.import.error.not_found", locale)))?;

    // Importing a source stream that was already imported is allowed, but only
    // after the user confirms the warning for this exact hash entry.
    let confirmed = form.confirmed_hash.as_deref() == Some(form.hash.as_str());
    if !confirmed {
        for store in state.csb_store_registry().stores_by_scope().await? {
            let already_imported_and_not_deleted =
                store.data.read().events.first().is_some_and(|e| {
                    matches!(&e.payload.action, CsbAction::Import { source_stream_id: sid, .. } if *sid == source_stream_id) &&
                    !store.is_deleted()
                });
            if already_imported_and_not_deleted {
                let appellation = store.get_appellation(WithCorrections::All);
                return Ok(ImportOutcome::AlreadyImported { appellation });
            }
        }
    }

    // Replay the source stream up to the matched event into a snapshot. Reload
    // first so a cached store that lags the persisted log (e.g. events appended
    // by another instance) still contains the matched event; otherwise
    // `snapshot_until` would silently produce an incomplete snapshot.
    let source_store = state
        .store_for_stream(source_stream_id, source_election, false)
        .await?;
    source_store.load().await?;

    let events = source_store.data.read().events.clone();
    let full_hash = events
        .iter()
        .find(|e| e.event_id == event_id)
        .map(|e| e.hash)
        .ok_or(AppError::GenericNotFound)?;
    let snapshot = PgStoreData::snapshot_until(&events, event_id);

    // Persist the import under a fresh CSB stream.
    let csb_store = CsbStore::acting_as(
        state
            .csb_store_for_stream(StreamId::new(), source_election)
            .await?,
        user,
    );
    csb_store
        .update(CsbAction::Import {
            hash: full_hash,
            source_stream_id,
            snapshot: Box::new(snapshot),
        })
        .await?;

    do_brp_verification(&csb_store, state.brp_client()).await?;

    Ok(ImportOutcome::Imported(redirect_success(
        CsbPoliticalGroupPath {
            stream_id: csb_store.stream_id,
        },
    )))
}

/// Create a new empty CSB store without importing from a political-group stream.
pub async fn create_empty<S: AppRequestState>(
    _: CsbCreateEmptyPath,
    State(state): State<S>,
    context: CsbContext,
) -> Result<Response, AppError> {
    let csb_store = CsbStore::acting_as(
        state
            .csb_store_for_stream(StreamId::new(), context.election)
            .await?,
        context.user()?,
    );
    csb_store.update(CsbAction::CreateEmpty).await?;
    Ok(redirect_success(CsbPoliticalGroupPath {
        stream_id: csb_store.stream_id,
    }))
}

/// The streams this process is sweeping right now.
///
/// [`BrpStatus::InProgress`] only records that a sweep was started, and a
/// process that stopped halfway through leaves that behind for good.
static RUNNING_SWEEPS: LazyLock<Mutex<HashSet<StreamId>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// Whether a BRP sweep over this stream is running right now.
pub fn brp_sweep_running(stream_id: StreamId) -> bool {
    RUNNING_SWEEPS
        .lock()
        .expect("running sweeps")
        .contains(&stream_id)
}

/// One stream claimed for one sweep, released on drop so that a sweep ending
/// without recording its outcome still frees the stream.
struct SweepClaim(StreamId);

impl SweepClaim {
    /// `None` when this stream is already being swept.
    fn claim(stream_id: StreamId) -> Option<Self> {
        // The guard has to be released before the claim exists: dropping one
        // takes the same lock.
        let claimed = RUNNING_SWEEPS
            .lock()
            .expect("running sweeps")
            .insert(stream_id);

        claimed.then(|| Self(stream_id))
    }
}

impl Drop for SweepClaim {
    fn drop(&mut self) {
        RUNNING_SWEEPS
            .lock()
            .expect("running sweeps")
            .remove(&self.0);
    }
}

/// Claim a stream, so a test can render what a running sweep looks like.
/// Released when the returned value is dropped.
#[cfg(test)]
pub fn claim_sweep_for_test(stream_id: StreamId) -> impl Drop {
    SweepClaim::claim(stream_id).expect("stream is already being swept")
}

/// Start the BRP check for every candidate on `store` in a background task,
/// returning as soon as it is spawned. A sweep that is already running is left
/// alone rather than joined by a second one.
pub async fn do_brp_verification(store: &CsbStore, brp_client: &BrpClient) -> Result<(), AppError> {
    // Claimed first, so two concurrent requests cannot both get past here.
    let Some(claim) = SweepClaim::claim(store.stream_id) else {
        tracing::info!(
            "BRP check for stream {} is already running; not starting another",
            store.stream_id
        );
        return Ok(());
    };

    store
        .update(CsbAction::SetBrpStatus(BrpStatus::in_progress()))
        .await?;

    tokio::task::spawn(monitor_verification(
        store.clone(),
        brp_client.clone(),
        claim,
    ));

    Ok(())
}

/// Run the sweep and record how it ended. Anything that stops it early leaves
/// the stream [`BrpStatus::Aborted`]: "finished" has to mean every candidate
/// was actually checked. `_claim` is released once the outcome is recorded.
async fn monitor_verification(store: CsbStore, brp_client: BrpClient, _claim: SweepClaim) {
    let outcome = tokio::task::spawn(verify_candidates(store.clone(), brp_client)).await;

    let error = match outcome {
        Ok(Ok(())) => return,
        Ok(Err(err)) => err.to_string(),
        Err(join_err) => join_err.to_string(),
    };

    tracing::error!("BRP check for stream {} aborted: {error}", store.stream_id);

    if let Err(err) = store
        .update(CsbAction::SetBrpStatus(BrpStatus::Aborted(error)))
        .await
    {
        tracing::error!("failed to record aborted BRP status: {err}");
    }
}

/// Check every candidate not covered by `CsbStoreData::brp_findings` yet,
/// [`BRP_BSN_BATCH_SIZE`] per request with `BRP_COURTESY_TIMEOUT` in between.
///
/// The corrected data is checked, since that is what the committee examines.
/// Errors propagate: the BRP being unreachable must not record the remaining
/// candidates as clean.
async fn verify_candidates(store: CsbStore, brp_client: BrpClient) -> Result<(), AppError> {
    let already_checked = store.get_brp_findings();
    let unchecked: Vec<Person> = store
        .get_persons(WithCorrections::All)
        .into_iter()
        .filter(|person| !already_checked.contains_key(&person.id))
        .collect();

    let mut ticker = tokio::time::interval(BRP_COURTESY_TIMEOUT);
    for batch in unchecked.chunks(BRP_BSN_BATCH_SIZE) {
        ticker.tick().await;

        for (person, findings) in brp_client.verify_batch(batch).await? {
            store
                .update(CsbAction::BrpPersonChecked { person, findings })
                .await?;
        }
    }

    tracing::info!(
        "Finished checking {} candidates on list {}",
        unchecked.len(),
        store
            .get_political_group(WithCorrections::All)
            .appellation
            .unwrap_or_default()
    );

    store
        .update(CsbAction::SetBrpStatus(BrpStatus::Finished))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    use crate::{
        AppState,
        CsbAction::Delete,
        CsbContext, ElectionConfig, PgEvent,
        brp_stub::{BrpStub, matching_record},
        structs::brp::{BrpFinding, BrpValue},
        test_utils::{response_body_string, sample_person_from_brp},
        utils::format_hash,
    };

    /// Populate a political-group stream with a single event in the (in-memory)
    /// test store and return its `(stream_id, formatted chain hash)`.
    async fn seed_source_event(state: &AppState) -> Result<(StreamId, String), AppError> {
        let source_stream = StreamId::new();
        let source_store = state
            .store_for_stream(source_stream, ElectionConfig::EK27, false)
            .await?;
        source_store.update(PgEvent::HideDownloadWarning).await?;

        let hash = source_store.data.read().events[0].hash;
        Ok((source_stream, format_hash(&hash, false)))
    }

    /// Submit the import form against a fresh test context.
    async fn submit(
        state: &AppState,
        hash: &str,
        confirmed_hash: Option<&str>,
    ) -> Result<Response, AppError> {
        Ok(import_submit(
            CsbImportPath {},
            State(state.clone()),
            CsbContext::new_test(),
            Form(ImportForm {
                hash: hash.to_string(),
                confirmed_hash: confirmed_hash.map(str::to_string),
            }),
        )
        .await?
        .into_response())
    }

    #[tokio::test]
    async fn import_renders_placeholder_page() -> Result<(), AppError> {
        let response = import(CsbImportPath {}, CsbContext::new_test())
            .await
            .unwrap()
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Import"));

        Ok(())
    }

    #[tokio::test]
    async fn import_submit_rejects_unparseable_hash() {
        let state = AppState::new_for_tests().await;
        let context = CsbContext::new_test();

        let response = import_submit(
            CsbImportPath {},
            State(state),
            context,
            Form(ImportForm {
                hash: "not-a-hash".to_string(),
                confirmed_hash: None,
            }),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn import_submit_rejects_unknown_hash() {
        // A fresh in-memory backend has no cached political-group streams, so a
        // well-formed hash resolves to no package.
        let state = AppState::new_for_tests().await;
        let context = CsbContext::new_test();

        let response = import_submit(
            CsbImportPath {},
            State(state),
            context,
            Form(ImportForm {
                hash: "F381 3DE7 96D3 8033".to_string(),
                confirmed_hash: None,
            }),
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn import_submit_imports_event_from_in_memory_store() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let (source_stream, hash) = seed_source_event(&state).await?;

        let context = CsbContext::new_test();

        let response = import_submit(
            CsbImportPath {},
            State(state.clone()),
            context,
            Form(ImportForm {
                hash,
                confirmed_hash: None,
            }),
        )
        .await?
        .into_response();

        // A successful import redirects to the examination overview.
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        // The import is recorded under a fresh CSB stream, carrying the source.
        let csb_stores = state.csb_store_registry().stores_by_scope().await?;
        assert_eq!(csb_stores.len(), 1);
        let imported = csb_stores[0].data.read().events.first().is_some_and(|e| {
            matches!(&e.payload.action, CsbAction::Import { source_stream_id, .. } if *source_stream_id == source_stream)
        });
        assert!(imported);

        Ok(())
    }

    #[tokio::test]
    async fn import_submit_warns_on_already_imported() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let (_, hash) = seed_source_event(&state).await?;

        // First import succeeds.
        let response = submit(&state, &hash, None).await?;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        // Re-importing the same source stream without confirmation re-renders
        // the form with a warning; nothing is imported yet.
        let response = submit(&state, &hash, None).await?;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("alert-warning"));
        assert!(body.contains(&hash));

        let csb_stores = state.csb_store_registry().stores_by_scope().await?;
        assert_eq!(csb_stores.len(), 1);

        // A confirmation for a different hash entry does not count.
        let response = submit(&state, &hash, Some("F381 3DE7 96D3 8033")).await?;
        assert_eq!(response.status(), StatusCode::OK);
        let csb_stores = state.csb_store_registry().stores_by_scope().await?;
        assert_eq!(csb_stores.len(), 1);

        Ok(())
    }

    #[tokio::test]
    async fn import_submit_imports_already_imported_after_confirm() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let (_, hash) = seed_source_event(&state).await?;

        let response = submit(&state, &hash, None).await?;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        // Confirming the warned hash imports the same source stream again.
        let response = submit(&state, &hash, Some(&hash)).await?;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let csb_stores = state.csb_store_registry().stores_by_scope().await?;
        assert_eq!(csb_stores.len(), 2);

        Ok(())
    }

    #[tokio::test]
    async fn create_empty_creates_csb_store_with_create_empty_event() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;

        let response = create_empty(
            CsbCreateEmptyPath {},
            State(state.clone()),
            CsbContext::new_test(),
        )
        .await?
        .into_response();

        // A successful creation redirects to the political group examination page.
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        // A single CSB store is recorded carrying the CreateEmpty event.
        let csb_stores = state.csb_store_registry().stores_by_scope().await?;
        assert_eq!(csb_stores.len(), 1);
        let data = csb_stores[0].data.read();
        assert!(matches!(
            data.events.first().unwrap().payload.action,
            CsbAction::CreateEmpty
        ));

        Ok(())
    }

    /// Poll for the background check to reach `status`, rather than sleeping
    /// out the full courtesy timeout.
    async fn wait_for_brp_status(store: &CsbStore, expected: fn(&BrpStatus) -> bool) -> BrpStatus {
        for _ in 0..200 {
            let status = store.get_brp_status();
            if expected(&status) {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!(
            "BRP verification did not reach the expected status in time, stuck at {:?}",
            store.get_brp_status()
        );
    }

    /// The candidate's burgerservicenummer, to serve a record under.
    fn exposed_bsn(person: &Person) -> String {
        person
            .personal_data
            .bsn
            .as_ref()
            .expect("the fixture has a BSN")
            .to_exposed_string()
    }

    /// A store holding `person`, ready for a sweep.
    async fn store_with_candidate(state: &AppState, person: Person) -> Result<CsbStore, AppError> {
        let csb_store = state
            .csb_store_for_stream(StreamId::new(), ElectionConfig::EK27)
            .await?
            .acting_as_test_user();
        csb_store.add_person(person);
        Ok(csb_store)
    }

    #[tokio::test]
    async fn brp_verification_records_a_finding_for_a_mismatched_candidate() -> Result<(), AppError>
    {
        let state = AppState::new_for_tests().await;
        let person = sample_person_from_brp();
        let person_id = person.id;
        let bsn = person
            .personal_data
            .bsn
            .as_ref()
            .expect("the fixture has a BSN")
            .to_exposed_string();
        let csb_store = store_with_candidate(&state, person).await?;

        // The BRP agrees on everything except the place of residence.
        let mut record = matching_record(&bsn);
        record["verblijfplaats"]["verblijfadres"]["woonplaats"] = serde_json::json!("Amsterdam");
        let stub = BrpStub::serving(vec![record]).await;

        do_brp_verification(&csb_store, &stub.client).await?;
        wait_for_brp_status(&csb_store, |status| matches!(status, BrpStatus::Finished)).await;

        assert_eq!(
            csb_store.get_brp_findings_for_person(person_id),
            vec![BrpFinding::Mismatch {
                brp_value: BrpValue::PlaceOfResidence("Amsterdam".parse().unwrap()),
            }]
        );
        // A BRP difference is for the committee to weigh, not a verzuim.
        assert!(csb_store.data.read().omissions.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn a_matching_candidate_is_recorded_as_checked_with_nothing_found() -> Result<(), AppError>
    {
        let state = AppState::new_for_tests().await;
        let person = sample_person_from_brp();
        let person_id = person.id;
        let bsn = person
            .personal_data
            .bsn
            .as_ref()
            .expect("the fixture has a BSN")
            .to_exposed_string();
        let csb_store = store_with_candidate(&state, person).await?;

        let stub = BrpStub::serving(vec![matching_record(&bsn)]).await;

        do_brp_verification(&csb_store, &stub.client).await?;
        wait_for_brp_status(&csb_store, |status| matches!(status, BrpStatus::Finished)).await;

        // Present in the map with no findings: checked, and nothing found.
        let findings = csb_store.get_brp_findings();
        assert_eq!(findings.get(&person_id), Some(&Vec::new()));

        Ok(())
    }

    #[tokio::test]
    async fn an_unreachable_brp_aborts_the_sweep_instead_of_finishing_it() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let csb_store = store_with_candidate(&state, sample_person_from_brp()).await?;

        // Port 1 on loopback refuses connections.
        let brp_client = BrpClient::new_for_test("http://127.0.0.1:1");

        do_brp_verification(&csb_store, &brp_client).await?;
        let status =
            wait_for_brp_status(&csb_store, |status| matches!(status, BrpStatus::Aborted(_))).await;

        // A BRP outage must not leave an empty findings list that reads as
        // "the BRP agreed on everything".
        assert!(matches!(status, BrpStatus::Aborted(_)), "{status:?}");
        assert!(csb_store.get_brp_findings().is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn do_brp_verification_returns_without_waiting_for_the_brp_check() -> Result<(), AppError>
    {
        let state = AppState::new_for_tests().await;
        let csb_store = state
            .csb_store_for_stream(StreamId::new(), ElectionConfig::EK27)
            .await?
            .acting_as_test_user();

        // Two batches means one BRP_COURTESY_TIMEOUT tick; the call should
        // return well before that.
        for _ in 0..=BRP_BSN_BATCH_SIZE {
            csb_store.add_person(sample_person_from_brp());
        }

        let stub = BrpStub::serving(Vec::new()).await;

        let start = tokio::time::Instant::now();
        do_brp_verification(&csb_store, &stub.client).await?;
        let elapsed = start.elapsed();
        assert!(
            elapsed < BRP_COURTESY_TIMEOUT,
            "do_brp_verification should return immediately instead of waiting for the background check, took {elapsed:?}"
        );

        wait_for_brp_status(&csb_store, |status| matches!(status, BrpStatus::Finished)).await;

        Ok(())
    }

    #[tokio::test]
    async fn candidates_are_looked_up_in_batches() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let csb_store = state
            .csb_store_for_stream(StreamId::new(), ElectionConfig::EK27)
            .await?
            .acting_as_test_user();

        for _ in 0..BRP_BSN_BATCH_SIZE {
            csb_store.add_person(sample_person_from_brp());
        }

        let stub = BrpStub::serving(Vec::new()).await;
        do_brp_verification(&csb_store, &stub.client).await?;
        wait_for_brp_status(&csb_store, |status| matches!(status, BrpStatus::Finished)).await;

        // They share one burgerservicenummer, so one lookup covers them.
        let lookups = stub.queries_of_type("RaadpleegMetBurgerservicenummer");
        assert_eq!(lookups.len(), 1);
        assert_eq!(
            lookups[0]["burgerservicenummer"].as_array().map(Vec::len),
            Some(BRP_BSN_BATCH_SIZE)
        );

        Ok(())
    }

    #[tokio::test]
    async fn do_brp_verification_skips_already_checked_candidates_on_a_later_call()
    -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let person = sample_person_from_brp();
        let bsn = person
            .personal_data
            .bsn
            .as_ref()
            .expect("the fixture has a BSN")
            .to_exposed_string();
        let csb_store = store_with_candidate(&state, person).await?;

        let stub = BrpStub::serving(vec![matching_record(&bsn)]).await;

        do_brp_verification(&csb_store, &stub.client).await?;
        wait_for_brp_status(&csb_store, |status| matches!(status, BrpStatus::Finished)).await;
        assert_eq!(stub.query_count(), 1);

        // Nothing is left to check, so there is no status to wait for.
        do_brp_verification(&csb_store, &stub.client).await?;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(stub.query_count(), 1);

        Ok(())
    }

    /// The case that used to be unrecoverable: nothing records how the sweep
    /// ended, leaving an `InProgress` status with no sweep behind it.
    #[tokio::test]
    async fn a_sweep_whose_process_went_away_does_not_block_the_next_one() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let person = sample_person_from_brp();
        let bsn = exposed_bsn(&person);
        let csb_store = store_with_candidate(&state, person).await?;

        // What a restart leaves behind.
        csb_store
            .update(CsbAction::SetBrpStatus(BrpStatus::in_progress()))
            .await?;
        assert!(!brp_sweep_running(csb_store.stream_id));

        let stub = BrpStub::serving(vec![matching_record(&bsn)]).await;
        do_brp_verification(&csb_store, &stub.client).await?;

        wait_for_brp_status(&csb_store, |status| matches!(status, BrpStatus::Finished)).await;
        assert_eq!(
            stub.query_count(),
            1,
            "the candidate should have been checked"
        );

        Ok(())
    }

    /// The claim has to be released too, or one sweep per stream would be all
    /// this process ever ran.
    #[tokio::test]
    async fn a_finished_sweep_leaves_the_stream_free_for_the_next_one() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let person = sample_person_from_brp();
        let bsn = exposed_bsn(&person);
        let csb_store = store_with_candidate(&state, person).await?;

        let stub = BrpStub::serving(vec![matching_record(&bsn)]).await;
        do_brp_verification(&csb_store, &stub.client).await?;
        wait_for_brp_status(&csb_store, |status| matches!(status, BrpStatus::Finished)).await;

        assert!(
            !brp_sweep_running(csb_store.stream_id),
            "a sweep that ended has to release the stream"
        );

        Ok(())
    }

    #[tokio::test]
    async fn a_running_sweep_is_not_joined_by_a_second_one() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let csb_store = state
            .csb_store_for_stream(StreamId::new(), ElectionConfig::EK27)
            .await?
            .acting_as_test_user();

        // Two batches, so the first sweep is still waiting out the courtesy
        // timeout when the second call arrives.
        for _ in 0..=BRP_BSN_BATCH_SIZE {
            csb_store.add_person(sample_person_from_brp());
        }

        let stub = BrpStub::serving(Vec::new()).await;
        do_brp_verification(&csb_store, &stub.client).await?;
        assert!(brp_sweep_running(csb_store.stream_id));

        // A second sweep would take the same snapshot and ask twice.
        do_brp_verification(&csb_store, &stub.client).await?;
        assert!(brp_sweep_running(csb_store.stream_id));

        wait_for_brp_status(&csb_store, |status| matches!(status, BrpStatus::Finished)).await;
        assert_eq!(
            stub.queries_of_type("RaadpleegMetBurgerservicenummer")
                .len(),
            2,
            "one lookup per batch, and no batch asked about twice"
        );

        Ok(())
    }

    #[tokio::test]
    async fn reimport_deleted_allowed() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let (_, hash) = seed_source_event(&state).await?;

        let context = CsbContext::new_test();
        let response = import_submit(
            CsbImportPath {},
            State(state.clone()),
            context,
            Form(ImportForm {
                hash: hash.clone(),
                confirmed_hash: None,
            }),
        )
        .await?
        .into_response();
        // First import succeeds (redirects away from import page)
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let csb_stores = state.csb_store_registry().stores_by_scope().await?;
        assert_eq!(csb_stores.len(), 1);

        // mark imported store as deleted
        csb_stores[0].update(Delete.by(CsbUser::new_test())).await?;

        // Re-importing the same source stream needs no confirmation when the
        // earlier import was deleted (the duplicate check consults the live
        // registry, as the in-memory backend persists nothing).
        let context = CsbContext::new_test();
        let response = import_submit(
            CsbImportPath {},
            State(state.clone()),
            context,
            Form(ImportForm {
                hash: hash.clone(),
                confirmed_hash: None,
            }),
        )
        .await?
        .into_response();

        // second import succeeds too as first import got deleted
        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let csb_stores = state.csb_store_registry().stores_by_scope().await?;
        assert_eq!(csb_stores.iter().filter(|s| s.is_deleted()).count(), 1);
        assert_eq!(csb_stores.iter().filter(|s| !s.is_deleted()).count(), 1);
        for store in csb_stores {
            if let CsbAction::Import {
                hash: import_hash, ..
            } = store.data.read().events.first().unwrap().payload.action
            {
                assert_eq!(hash.clone(), format_hash(&import_hash, false));
            } else {
                panic!("No import")
            }
        }

        Ok(())
    }
}
