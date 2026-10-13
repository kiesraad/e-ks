use axum::{extract::FromRequestParts, http::request::Parts};

use crate::{AppError, AppRequestState, store::StreamMeta};

/// One row of the monitoring overview.
pub struct StreamMonitorRow {
    pub meta: StreamMeta,
    pub political_group_name: Option<String>,
    /// Last event applied in the cache when warm; `None` when the stream is cold.
    pub cache_until_event: Option<usize>,
}

impl StreamMonitorRow {
    pub fn short_id(&self) -> String {
        self.meta.stream_id.to_string().chars().take(8).collect()
    }
}

pub struct StreamMonitor {
    pub rows: Vec<StreamMonitorRow>,
    pub database_enabled: bool,
}

impl<S: AppRequestState> FromRequestParts<S> for StreamMonitor {
    type Rejection = AppError;

    async fn from_request_parts(_parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let registry = state.store_registry();

        // Reading these figures never decrypts or warms a stream.
        let metadata = registry.stream_metadata_by_scope().await?;

        let mut rows = Vec::with_capacity(metadata.len());
        for mut meta in metadata {
            // The name and exact timestamps live in encrypted events, so they
            // are only available for streams already warm in the cache.
            let (political_group_name, cache_until_event) =
                match registry.get_cached(meta.stream_id, meta.election) {
                    Some(store) => {
                        let data = store.snapshot();
                        meta.created_at = data
                            .events
                            .first()
                            .map(|e| e.created_at)
                            .or(meta.created_at);
                        meta.last_event_at = data
                            .events
                            .last()
                            .map(|e| e.created_at)
                            .or(meta.last_event_at);
                        let name = data
                            .political_group
                            .appellation
                            .as_ref()
                            .map(ToString::to_string);
                        let until = data.events.last().map(|e| e.event_id).unwrap_or(0);
                        (name, Some(until))
                    }
                    None => (None, None),
                };

            rows.push(StreamMonitorRow {
                meta,
                political_group_name,
                cache_until_event,
            });
        }

        rows.sort_by_key(|row| row.meta.created_at);

        Ok(StreamMonitor {
            rows,
            database_enabled: cfg!(feature = "database"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PgStoreData, store::StoreRegistry};
    use axum::{body::Body, http::Request};

    use crate::{AppState, ElectionConfig, PgEvent, StreamId, test_utils::sample_political_group};

    fn empty_parts() -> Parts {
        Request::builder()
            .uri("/csb/monitoring")
            .body(Body::empty())
            .unwrap()
            .into_parts()
            .0
    }

    /// Persist a political-group stream with one name-setting event, warming it
    /// into the PG registry cache.
    async fn seed_political_group(state: &AppState, election: ElectionConfig) -> StreamId {
        let stream_id = StreamId::new();
        let store = state
            .store_for_stream(stream_id, election, false)
            .await
            .unwrap();
        store
            .update(PgEvent::UpdatePoliticalGroup(sample_political_group()))
            .await
            .unwrap();
        stream_id
    }

    /// Persist a stream through a separate registry sharing the PG registry's
    /// persistence, so it exists on disk but is never warmed into the registry cache.
    async fn seed_cold_political_group(state: &AppState, election: ElectionConfig) -> StreamId {
        use crate::crypto::MasterKey;

        let cold_registry = StoreRegistry::<PgStoreData>::with_persistence(
            state.store_registry.persistence().clone(),
            MasterKey::new(&state.config.master_encryption_key),
        );
        let stream_id = StreamId::new();
        let store = cold_registry
            .get_or_create(stream_id, election)
            .await
            .unwrap();
        store
            .update(PgEvent::UpdatePoliticalGroup(sample_political_group()))
            .await
            .unwrap();
        stream_id
    }

    #[tokio::test]
    async fn reports_a_row_per_political_group_stream() {
        let state = AppState::new_for_tests().await;
        let first = seed_political_group(&state, ElectionConfig::EK27).await;
        let second = seed_political_group(&state, ElectionConfig::EK27).await;

        let mut parts = empty_parts();
        let monitor = StreamMonitor::from_request_parts(&mut parts, &state)
            .await
            .unwrap();

        assert_eq!(monitor.rows.len(), 2);
        let ids: Vec<_> = monitor.rows.iter().map(|row| row.meta.stream_id).collect();
        assert!(ids.contains(&first));
        assert!(ids.contains(&second));
    }

    #[tokio::test]
    async fn row_carries_counts_timestamps_and_group_name() {
        let state = AppState::new_for_tests().await;
        let stream_id = seed_political_group(&state, ElectionConfig::EK27).await;

        let mut parts = empty_parts();
        let monitor = StreamMonitor::from_request_parts(&mut parts, &state)
            .await
            .unwrap();

        let row = monitor
            .rows
            .iter()
            .find(|row| row.meta.stream_id == stream_id)
            .expect("stream present");

        assert_eq!(row.meta.election, ElectionConfig::EK27);
        assert_eq!(row.meta.event_count, 1);
        assert!(row.meta.created_at.is_some());
        assert!(row.meta.last_event_at.is_some());
        assert_eq!(row.political_group_name.as_deref(), Some("Kiesraad Demo"));
        assert_eq!(row.short_id(), stream_id.to_string()[..8].to_string());
    }

    #[tokio::test]
    async fn warm_stream_reports_its_cache_position() {
        let state = AppState::new_for_tests().await;
        let stream_id = seed_political_group(&state, ElectionConfig::EK27).await;

        let mut parts = empty_parts();
        let monitor = StreamMonitor::from_request_parts(&mut parts, &state)
            .await
            .unwrap();
        let row = monitor
            .rows
            .iter()
            .find(|row| row.meta.stream_id == stream_id)
            .expect("stream present");

        assert_eq!(monitor.database_enabled, cfg!(feature = "database"));
        // Seeding warmed the stream, so it is applied up to its one event.
        assert_eq!(row.cache_until_event, Some(1));
    }

    #[tokio::test]
    async fn cold_streams_are_listed_from_metadata_without_a_name() {
        let state = AppState::new_for_tests().await;
        let stream_id = seed_cold_political_group(&state, ElectionConfig::EK27).await;

        let mut parts = empty_parts();
        let monitor = StreamMonitor::from_request_parts(&mut parts, &state)
            .await
            .unwrap();

        let row = monitor
            .rows
            .iter()
            .find(|row| row.meta.stream_id == stream_id)
            .expect("cold stream still listed");

        // The count comes from persistence; the name needs decryption, so a
        // never-warmed stream has neither a name nor a cache position.
        assert_eq!(row.meta.event_count, 1);
        assert!(row.political_group_name.is_none());
        assert!(row.cache_until_event.is_none());
    }
}
