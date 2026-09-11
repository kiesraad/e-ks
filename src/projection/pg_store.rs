//! [`PgStore`]: the store handle used by the app feature handlers, including
//! the CSB paper-corrections write target.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use chrono::Utc;
use tracing::warn;

use crate::{
    AppError, CsbAction, CsbStore, MAX_CANDIDATES, PgEvent, PgStoreData, RateLimit, RateLimits,
    store::{Store, StoreData},
};

/// Store handle used by the app feature handlers: reads come from the
/// [`PgStoreData`] projection, writes dispatch [`PgEvent`]s to the write
/// target.
///
/// A political group session appends events to its own stream. A CSB session
/// correcting the paper documents gets every event wrapped in
/// [`CsbAction::PaperCorrectedUpdate`] and appended to the CSB stream instead,
/// so corrections land in that stream's `paper_corrected_data` projection and
/// the political group's own stream is never touched.
#[derive(Clone)]
pub struct PgStore {
    /// Projection served to reads; [`std::ops::Deref`] keeps `store.election`,
    /// `store.data` and the getters working unchanged.
    projection: Store<PgStoreData>,
    target: WriteTarget,
    /// Limits enforced on writes; `None` disables them (paper corrections).
    limits: Option<RateLimits>,
    /// The last event this handle has seen on its own stream. A write is
    /// refused when the stream moved past it, so a request never persists a
    /// decision made on a snapshot another request has since changed.
    /// Shared by the clones handed to one request's extractors.
    seen_event_id: Arc<AtomicUsize>,
}

#[derive(Clone)]
enum WriteTarget {
    /// Persist events on the projection's own stream.
    Own,
    /// Wrap events in [`CsbAction::PaperCorrectedUpdate`] and persist them on
    /// this CSB stream, which records the correcting committee member. The
    /// projection is a request-local snapshot of the stream's
    /// `paper_corrected_data`.
    PaperCorrections { store: CsbStore },
}

impl std::ops::Deref for PgStore {
    type Target = Store<PgStoreData>;

    fn deref(&self) -> &Self::Target {
        &self.projection
    }
}

impl PgStore {
    /// Wrap a store that persists app events on its own stream.
    pub fn own(store: Store<PgStoreData>) -> Self {
        let seen_event_id = store.data.read().last_event_id();
        Self {
            projection: store,
            target: WriteTarget::Own,
            limits: Some(RateLimits::default()),
            seen_event_id: Arc::new(AtomicUsize::new(seen_event_id)),
        }
    }

    /// Enforce the configured (rather than the built-in default) limits on
    /// this handle's writes.
    pub fn with_limits(mut self, limits: RateLimits) -> Self {
        self.limits = Some(limits);
        self
    }

    /// Build a paper-corrections handle over a loaded CSB store: reads serve a
    /// snapshot of its `paper_corrected_data`, writes go to the CSB stream,
    /// recorded as triggered by the store's committee member.
    pub fn paper_corrections(csb_store: CsbStore) -> Self {
        let projection = Store::new_for_temp_stream(csb_store.election);
        *projection.data.write() = csb_store.data.read().paper_corrected_data.clone();

        Self {
            projection: Store {
                stream_id: csb_store.stream_id,
                ..projection
            },
            target: WriteTarget::PaperCorrections { store: csb_store },
            // Recording what was handed in on paper must not be cut off.
            limits: None,
            // Unused: writes go to the CSB stream.
            seen_event_id: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Create a temporary, in-memory handle with no persistence.
    pub fn new_for_temp_stream(election: crate::ElectionConfig) -> Self {
        Self::own(Store::new_for_temp_stream(election))
    }

    /// Persist an event on the write target and apply it to the projection.
    /// On the own stream the write is refused with [`AppError::Conflict`] when
    /// another request appended since this handle last saw the stream.
    pub async fn update(&self, event: PgEvent) -> Result<(), AppError> {
        match &self.target {
            WriteTarget::Own => {
                let expected = self.seen_event_id.load(Ordering::Acquire);
                self.check_rate_limits(&event)?;
                self.projection.update_if_unchanged(event, expected).await?;
                self.seen_event_id.store(
                    self.projection.data.read().last_event_id(),
                    Ordering::Release,
                );
                Ok(())
            }
            WriteTarget::PaperCorrections { store: csb_store } => {
                csb_store
                    .update(CsbAction::PaperCorrectedUpdate(Box::new(event)))
                    .await?;

                // Refresh the snapshot so reads later in this request observe
                // the correction.
                *self.projection.data.write() = csb_store.data.read().paper_corrected_data.clone();

                Ok(())
            }
        }
    }

    /// Refuse the write when one of the configured [`RateLimits`] is reached,
    /// counted from the stream's own event log.
    ///
    /// Every event counts, session events (login/logout) included.
    fn check_rate_limits(&self, event: &PgEvent) -> Result<(), AppError> {
        self.check_limits(matches!(event, PgEvent::DownloadFile { .. }))
    }

    /// The limits a `DownloadFile` event would be held to, for checking before
    /// the documents are generated.
    pub fn check_download_limit(&self) -> Result<(), AppError> {
        self.check_limits(true)
    }

    fn check_limits(&self, is_download: bool) -> Result<(), AppError> {
        let Some(RateLimits {
            downloads,
            events: event_limit,
            events_total,
        }) = self.limits
        else {
            return Ok(());
        };

        let data = self.projection.data.read();
        let events = data.events();
        let now = Utc::now();

        if events_total > 0 && events.len() >= events_total {
            return Err(self.limit_hit(
                "events_total",
                AppError::EventLimitReached { max: events_total },
            ));
        }

        // Events are appended in order, so the window is a tail of the log.
        let in_window = |limit: &RateLimit| {
            let start = limit.window_start(now);
            &events[events.partition_point(|event| event.created_at <= start)..]
        };

        if event_limit.is_reached(in_window(&event_limit).len()) {
            return Err(self.limit_hit(
                "events",
                AppError::TooManyEvents {
                    max: event_limit.max,
                    window: event_limit.window,
                },
            ));
        }

        if is_download {
            let count = in_window(&downloads)
                .iter()
                .filter(|event| matches!(event.payload, PgEvent::DownloadFile { .. }))
                .count();
            if downloads.is_reached(count) {
                return Err(self.limit_hit(
                    "downloads",
                    AppError::TooManyDownloads {
                        max: downloads.max,
                        window: downloads.window,
                    },
                ));
            }
        }

        Ok(())
    }

    /// Emit the monitoring marker for a refused write and pass the error on.
    /// Alerting matches on `event = "rate_limit.hit"`.
    fn limit_hit(&self, limit: &'static str, err: AppError) -> AppError {
        warn!(
            event = "rate_limit.hit",
            limit,
            stream_id = %self.projection.stream_id,
            "rate limit hit"
        );
        err
    }

    /// Hard cap on the number of candidates a single list may hold.
    ///
    /// Paper corrections record the list exactly as it was handed in on paper,
    /// so an over-long list has to be enterable there: the surplus candidates
    /// are scratched later, they are not kept out of the data. The cap only
    /// applies to a political group entering its own data.
    pub fn candidate_limit(&self) -> usize {
        match &self.target {
            WriteTarget::Own => MAX_CANDIDATES,
            WriteTarget::PaperCorrections { .. } => usize::MAX,
        }
    }

    /// The CSB stream receiving paper corrections, when this handle is in
    /// paper-corrections mode.
    pub fn paper_corrections_stream_id(&self) -> Option<crate::StreamId> {
        match &self.target {
            WriteTarget::Own => None,
            WriteTarget::PaperCorrections {
                store: csb_store, ..
            } => Some(csb_store.stream_id),
        }
    }

    /// The imported snapshot the paper corrections were applied on top of,
    /// when this handle is in paper-corrections mode. Serves as the base
    /// state for audit-log replays, since the correction events alone do not
    /// reconstruct the imported entities.
    pub fn imported_snapshot(&self) -> Option<PgStoreData> {
        match &self.target {
            WriteTarget::Own => None,
            WriteTarget::PaperCorrections {
                store: csb_store, ..
            } => Some(csb_store.data.read().imported_data.clone()),
        }
    }
}

#[cfg(all(test, feature = "database"))]
impl PgStore {
    pub async fn new_with_pool_for_stream(
        pool: sqlx::PgPool,
        stream_id: crate::StreamId,
        election: crate::ElectionConfig,
        master: &crate::crypto::MasterKey,
    ) -> Result<Self, AppError> {
        Ok(Self::own(
            Store::new_with_pool_for_stream(pool, stream_id, election, master).await?,
        ))
    }
}

#[cfg(test)]
impl PgStore {
    pub fn new_for_test() -> Self {
        Self::new_for_test_with_election(crate::ElectionConfig::EK27)
    }

    pub fn new_for_test_with_election(election: crate::ElectionConfig) -> Self {
        use crate::StreamId;

        let data = PgStoreData {
            political_group: crate::test_utils::sample_political_group(),
            ..PgStoreData::default()
        };

        Self::own(crate::store::Store {
            stream_id: StreamId::new(),
            election,
            backend: crate::store::StoreBackend::Memory {
                store: crate::store::memory::MemoryStore::default(),
            },
            data: std::sync::Arc::new(parking_lot::RwLock::new(data)),
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;
    use crate::{
        store::StoreEvent,
        structs::{candidate_lists::CandidateListId, persons::PersonId},
    };

    /// Two requests validate against the same snapshot; only the first may
    /// write, the second sees the stream has moved.
    #[tokio::test]
    async fn a_write_on_a_changed_stream_is_refused() -> Result<(), AppError> {
        use crate::test_utils::sample_person;

        let first = PgStore::new_for_test();
        let second = PgStore::own(first.projection.clone());

        sample_person(PersonId::new()).create(&first).await?;

        let err = sample_person(PersonId::new())
            .create(&second)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Conflict));
        assert_eq!(first.get_persons().len(), 1);

        // the handle that wrote keeps writing, a fresh handle sees the new state
        sample_person(PersonId::new()).create(&first).await?;
        sample_person(PersonId::new())
            .create(&PgStore::own(first.projection.clone()))
            .await?;
        assert_eq!(first.get_persons().len(), 3);

        Ok(())
    }

    /// A store holding `count` events that all happened `age` ago.
    fn store_with_events(limits: RateLimits, count: usize, age: TimeDelta) -> PgStore {
        let store = PgStore::new_for_test();
        let created_at = Utc::now() - age;

        for event_id in 1..=count {
            store.apply_event(StoreEvent::new_at(
                event_id,
                PgEvent::DeletePerson {
                    person_id: PersonId::new(),
                },
                created_at,
            ));
        }

        // a handle that has seen the seeded events
        PgStore::own(store.projection).with_limits(limits)
    }

    fn download_event() -> PgEvent {
        PgEvent::DownloadFile {
            file_name: "documents.zip".to_string(),
            download_path: "/generate/nl".to_string(),
        }
    }

    fn data_event() -> PgEvent {
        PgEvent::DeleteCandidateList(CandidateListId::new())
    }

    /// The absolute cap stops every write, session events included, but never
    /// a read (nothing in this module gates reads): a political group whose
    /// stream is capped can still look at its data.
    #[tokio::test]
    async fn absolute_event_cap_stops_every_write_but_not_reads() {
        let limits = RateLimits::new_for_test(0, 0, 3, TimeDelta::minutes(1));
        let store = store_with_events(limits, 3, TimeDelta::days(30));

        for event in [data_event(), PgEvent::Login, PgEvent::Logout] {
            let err = store.update(event).await.expect_err("cap must be enforced");
            assert!(
                matches!(err, AppError::EventLimitReached { max: 3 }),
                "got {err:?}"
            );
        }

        // Reads never pass through the limit check, so the stream stays
        // viewable while the cap is in force.
        assert_eq!(
            store.get_political_group().appellation,
            crate::test_utils::sample_political_group().appellation
        );
    }

    /// Below the cap, writes go through.
    #[tokio::test]
    async fn absolute_event_cap_allows_writes_below_the_maximum() {
        let limits = RateLimits::new_for_test(0, 0, 4, TimeDelta::minutes(1));
        let store = store_with_events(limits, 3, TimeDelta::days(30));

        store.update(data_event()).await.expect("below the cap");
    }

    /// The sliding window counts every event, and only the recent ones.
    #[tokio::test]
    async fn event_window_limit_blocks_a_burst_and_expires() {
        let limits = RateLimits::new_for_test(0, 2, 0, TimeDelta::minutes(1));

        let recent = store_with_events(limits, 2, TimeDelta::seconds(30));
        let err = recent
            .update(data_event())
            .await
            .expect_err("window limit must be enforced");
        assert!(
            matches!(
                err,
                AppError::TooManyEvents { max: 2, window } if window == TimeDelta::minutes(1)
            ),
            "got {err:?}"
        );

        let expired = store_with_events(limits, 2, TimeDelta::seconds(61));
        expired
            .update(data_event())
            .await
            .expect("events outside the window no longer count");
    }

    /// The download limit only counts (and only blocks) downloads: other
    /// changes keep working while it is in force.
    #[tokio::test]
    async fn download_limit_blocks_only_downloads() {
        let limits = RateLimits::new_for_test(2, 0, 0, TimeDelta::minutes(1));
        let store = store_with_events(limits, 0, TimeDelta::zero());

        store.update(download_event()).await.expect("first");
        store.update(download_event()).await.expect("second");

        let err = store
            .update(download_event())
            .await
            .expect_err("download limit must be enforced");
        assert!(
            matches!(err, AppError::TooManyDownloads { max: 2, .. }),
            "got {err:?}"
        );

        store
            .update(data_event())
            .await
            .expect("other changes are unaffected");
    }

    /// Paper corrections are recorded by the committee on the CSB stream and
    /// are not rate limited.
    #[tokio::test]
    async fn paper_corrections_are_not_rate_limited() -> Result<(), AppError> {
        let store = crate::test_utils::paper_corrections_store().await?;

        for _ in 0..5 {
            store.update(download_event()).await?;
        }

        Ok(())
    }

    /// A refused write emits the monitoring marker.
    #[tracing_test::traced_test]
    #[tokio::test]
    async fn a_limit_hit_logs_a_monitoring_marker() {
        let limits = RateLimits::new_for_test(0, 1, 0, TimeDelta::minutes(1));
        let store = store_with_events(limits, 1, TimeDelta::seconds(1));

        store.update(data_event()).await.expect_err("limit hit");

        assert!(logs_contain("rate_limit.hit"));
        assert!(logs_contain("limit=\"events\""));
    }

    /// A store built by the middleware carries the configured limits.
    #[tokio::test]
    async fn with_limits_overrides_the_defaults() {
        let limits = RateLimits::new_for_test(0, 1, 0, TimeDelta::minutes(1));
        let store = PgStore::new_for_test().with_limits(limits);

        store.update(data_event()).await.expect("first");
        assert!(store.update(data_event()).await.is_err());
    }

    /// The hard maximum guards a political group's own data; paper
    /// corrections must be able to record an over-long list as handed in.
    #[tokio::test]
    async fn candidate_limit_is_lifted_while_correcting() -> Result<(), AppError> {
        assert_eq!(PgStore::new_for_test().candidate_limit(), MAX_CANDIDATES);
        assert_eq!(
            crate::test_utils::paper_corrections_store()
                .await?
                .candidate_limit(),
            usize::MAX
        );

        Ok(())
    }
}
