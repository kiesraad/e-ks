//! In-memory persistence backend for the event store.
//!
//! Unlike the database backend, the in-memory backend keeps no
//! durable storage: event payloads live only in the registry's cached
//! projections, so an in-memory store's [`Store::load`](super::Store::load) is a
//! no-op. It does, however, keep a small shared index of each stream's scope and
//! its events' chain hashes, so cross-stream lookups (by scope, by hash prefix)
//! resolve uniformly with the other backends instead of being special-cased by
//! callers.
//!
//! The index is shared (cloning a [`MemoryStore`] shares the same state), so
//! registries built on the same in-memory backend, e.g. the app and CSB
//! registries, observe each other's writes, mirroring a shared database pool.

use std::{collections::HashMap, sync::Arc};

use parking_lot::RwLock;

use super::{EventHash, StreamMeta};
use crate::{AppError, ElectionConfig, Scope, StreamId};

/// Per-stream metadata recorded by the in-memory backend.
#[derive(Debug, Default)]
struct StreamEntry {
    /// Scope recorded when the stream was first ensured.
    scope: Option<Scope>,
    /// `(event_id, chain hash)` for each event, in append order.
    events: Vec<(usize, [u8; 32])>,
}

/// Shared in-memory index of streams and their event hashes.
#[derive(Clone, Debug, Default)]
pub struct MemoryStore {
    inner: Arc<RwLock<HashMap<(StreamId, ElectionConfig), StreamEntry>>>,
}

/// Record `scope` for `(stream_id, election)` when the stream is first created.
///
/// Mirrors `ensure_stream` on the persisting backends: a no-op when the stream
/// already exists, so a stream's scope is never overwritten.
pub(crate) fn ensure_stream(
    store: &MemoryStore,
    stream_id: StreamId,
    election: ElectionConfig,
    scope: Scope,
) {
    store
        .inner
        .write()
        .entry((stream_id, election))
        .or_default()
        .scope
        .get_or_insert(scope);
}

/// Record a freshly applied event's `(event_id, chain hash)` under its stream.
pub(crate) fn record_event(
    store: &MemoryStore,
    stream_id: StreamId,
    election: ElectionConfig,
    event_id: usize,
    hash: EventHash,
) {
    store
        .inner
        .write()
        .entry((stream_id, election))
        .or_default()
        .events
        .push((event_id, hash));
}

/// List every non-empty `(stream_id, election)` stream with the given scope.
pub(crate) fn streams_by_scope(
    store: &MemoryStore,
    scope: Scope,
) -> Vec<(StreamId, ElectionConfig)> {
    let index = store.inner.read();
    index
        .iter()
        .filter_map(|((id, election), entry)| {
            (entry.scope == Some(scope) && !entry.events.is_empty()).then_some((*id, *election))
        })
        .collect()
}

/// List [`StreamMeta`] for every non-empty stream with the given scope. The
/// in-memory index keeps no timestamps, so they are always `None`.
pub(crate) fn stream_metadata_by_scope(store: &MemoryStore, scope: Scope) -> Vec<StreamMeta> {
    let index = store.inner.read();
    index
        .iter()
        .filter(|(_, entry)| entry.scope == Some(scope) && !entry.events.is_empty())
        .map(|((id, election), entry)| StreamMeta {
            stream_id: *id,
            election: *election,
            event_count: entry
                .events
                .iter()
                .map(|(event_id, _)| *event_id)
                .max()
                .unwrap_or(0),
            created_at: None,
            last_event_at: None,
        })
        .collect()
}

/// List the elections under the given stream that have recorded events.
pub(crate) fn elections_for_stream(
    store: &MemoryStore,
    stream_id: StreamId,
) -> Vec<ElectionConfig> {
    let index = store.inner.read();
    index
        .iter()
        .filter_map(|((id, election), entry)| {
            (*id == stream_id && !entry.events.is_empty()).then_some(*election)
        })
        .collect()
}

/// Locate the political-group event whose chain hash begins with `hash_prefix`,
/// returning its `(stream_id, election, event_id)`.
///
/// The lookup is restricted to [`Scope::PoliticalGroup`] streams, mirroring the
/// database backend, so a prefix can only ever resolve to an
/// app-store event (never a CSB event). An ambiguous prefix matching more than
/// one event is reported as [`AppError::AmbiguousHash`].
pub(crate) fn find_event_by_hash_prefix(
    store: &MemoryStore,
    hash_prefix: &[u8],
) -> Result<Option<(StreamId, ElectionConfig, usize)>, AppError> {
    let index = store.inner.read();
    let mut matches = Vec::new();
    for ((stream_id, election), entry) in index.iter() {
        if entry.scope != Some(Scope::PoliticalGroup) {
            continue;
        }
        for (event_id, hash) in &entry.events {
            if hash.starts_with(hash_prefix) {
                matches.push((*stream_id, *election, *event_id));
                if matches.len() > 1 {
                    return Err(AppError::AmbiguousHash);
                }
            }
        }
    }

    Ok(matches.into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ELECTION: ElectionConfig = ElectionConfig::EK27;

    #[test]
    fn streams_by_scope_filters_on_scope_and_data() {
        let store = MemoryStore::default();
        let pg = StreamId::new();
        let csb = StreamId::new();
        let empty = StreamId::new();

        ensure_stream(&store, pg, ELECTION, Scope::PoliticalGroup);
        ensure_stream(&store, csb, ELECTION, Scope::CentralElectoralCommittee);
        ensure_stream(&store, empty, ELECTION, Scope::PoliticalGroup);
        record_event(&store, pg, ELECTION, 1, [1u8; 32]);
        record_event(&store, csb, ELECTION, 1, [2u8; 32]);

        assert_eq!(
            streams_by_scope(&store, Scope::PoliticalGroup),
            vec![(pg, ELECTION)]
        );
        assert_eq!(
            streams_by_scope(&store, Scope::CentralElectoralCommittee),
            vec![(csb, ELECTION)]
        );
    }

    #[test]
    fn find_event_by_hash_prefix_matches_political_group_only() {
        let store = MemoryStore::default();
        let pg = StreamId::new();
        let csb = StreamId::new();
        ensure_stream(&store, pg, ELECTION, Scope::PoliticalGroup);
        ensure_stream(&store, csb, ELECTION, Scope::CentralElectoralCommittee);

        let pg_hash = [0xABu8; 32];
        let csb_hash = [0xCDu8; 32];
        record_event(&store, pg, ELECTION, 1, pg_hash);
        record_event(&store, csb, ELECTION, 1, csb_hash);

        assert_eq!(
            find_event_by_hash_prefix(&store, &pg_hash[..4]).unwrap(),
            Some((pg, ELECTION, 1))
        );
        // A CSB event is never resolved, even on an exact hash.
        assert_eq!(find_event_by_hash_prefix(&store, &csb_hash).unwrap(), None);
        // Nothing matches an unknown prefix.
        assert_eq!(
            find_event_by_hash_prefix(&store, &[0xFFu8; 4]).unwrap(),
            None
        );
    }

    #[test]
    fn find_event_by_hash_prefix_detects_ambiguity() {
        let store = MemoryStore::default();
        let stream = StreamId::new();
        ensure_stream(&store, stream, ELECTION, Scope::PoliticalGroup);
        let mut first = [0x01u8; 32];
        let mut second = [0x02u8; 32];
        first[0] = 0xAB;
        second[0] = 0xAB;
        record_event(&store, stream, ELECTION, 1, first);
        record_event(&store, stream, ELECTION, 2, second);

        // The shared `0xAB` first byte matches both events.
        let err = find_event_by_hash_prefix(&store, &[0xABu8]).unwrap_err();
        assert!(matches!(err, AppError::AmbiguousHash));
    }

    #[test]
    fn ensure_stream_does_not_overwrite_scope() {
        let store = MemoryStore::default();
        let stream = StreamId::new();
        ensure_stream(&store, stream, ELECTION, Scope::PoliticalGroup);
        ensure_stream(&store, stream, ELECTION, Scope::CentralElectoralCommittee);
        record_event(&store, stream, ELECTION, 1, [1u8; 32]);

        assert_eq!(
            streams_by_scope(&store, Scope::PoliticalGroup),
            vec![(stream, ELECTION)]
        );
    }
}
