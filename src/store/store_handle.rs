//! Store handle and constructors for event-sourced data.

use std::sync::Arc;

use chrono::{DateTime, Utc};

use std::path::PathBuf;

use crate::{ElectionConfig, StreamId, crypto::EventCipher};

use super::{EventHash, EventHashPrefix, StoreData, StoreEvent, memory::MemoryStore};

/// Event-sourced store handle for a single (stream, election) pair.
pub struct Store<D> {
    /// Stream identifier. One stream per user; events are partitioned by
    /// `(stream_id, election)`.
    pub stream_id: StreamId,
    /// Election this store instance is scoped to.
    pub election: ElectionConfig,
    /// Persistence target paired with its cipher. Persisting backends are
    /// always encrypted; see [`StoreBackend`].
    pub(crate) backend: StoreBackend,
    /// In-memory projection for the stream.
    pub(crate) data: Arc<parking_lot::RwLock<D>>,
}

impl<D> Clone for Store<D> {
    /// Clone the store handle, sharing the same underlying data and persistence.
    fn clone(&self) -> Self {
        Self {
            stream_id: self.stream_id,
            election: self.election,
            backend: self.backend.clone(),
            data: self.data.clone(),
        }
    }
}

/// A store's resolved backend: a persistence target paired with the
/// per-stream [`EventCipher`].
///
/// The persisting variants (`Database`, `Local`) cannot be constructed
/// without a cipher (see `StorePersistence::into_backend_for_stream`), so
/// events written to disk or database are *always* encrypted. `Memory` carries
/// no cipher because it never writes events out; it keeps only the shared
/// index used to answer cross-stream lookups.
#[derive(Clone, Debug)]
pub(crate) enum StoreBackend {
    /// PostgreSQL-backed, encrypted persistence.
    #[cfg(feature = "database")]
    Database {
        pool: sqlx::PgPool,
        cipher: Box<EventCipher>,
    },
    /// Local filesystem-backed, encrypted persistence.
    Local {
        dir: PathBuf,
        cipher: Box<EventCipher>,
    },
    /// In-memory only: no durable persistence and no encryption, just the shared
    /// index that records event hashes and stream scopes.
    Memory { store: MemoryStore },
}

impl<D> Store<D>
where
    D: StoreData,
{
    /// Create a temporary, in-memory store with no persistence.
    ///
    /// An in-memory store never writes events out, so it has no cipher
    /// (see [`StoreBackend::Memory`]).
    pub fn new_for_temp_stream(election: ElectionConfig) -> Self {
        Store {
            stream_id: StreamId::new(),
            election,
            backend: StoreBackend::Memory {
                store: MemoryStore::default(),
            },
            data: Arc::new(parking_lot::RwLock::new(D::default())),
        }
    }

    /// Apply a single event to the in-memory projection.
    ///
    /// No-op if the projection is already at or past this event: another
    /// instance of the application may have processed and applied it before
    /// this caller acquired the write lock.
    pub fn apply_event(&self, store_event: StoreEvent<D::Event>) {
        let mut data = self.data.write();

        if data.last_event_id() >= store_event.event_id {
            return;
        }

        data.apply(store_event);
    }

    /// Build a [`StoreEvent`] for a freshly persisted event and apply it to the
    /// in-memory projection via [`Store::apply_event`].
    pub(crate) fn apply_persisted_event(
        &self,
        event_id: usize,
        payload: D::Event,
        created_at: DateTime<Utc>,
        hash: EventHash,
    ) {
        self.apply_event(StoreEvent {
            event_id,
            payload,
            created_at,
            hash,
        });
    }

    /// Last event ID applied to the in-memory projection, or 0 if none.
    pub fn current_event_id(&self) -> usize {
        self.data.read().last_event_id()
    }

    /// Chain hash of the last applied event, or
    /// [`GENESIS_HASH`](crate::store::GENESIS_HASH) if none.
    pub fn current_event_hash(&self) -> EventHash {
        self.data.read().last_event_hash()
    }

    /// Whether `prefix` names any event in this chain: the check download and
    /// export links are held to. Any event, not just the newest, so a link
    /// still on screen survives the audit event its own download appends.
    pub fn has_event_hash(&self, prefix: EventHashPrefix) -> bool {
        if prefix.is_genesis() {
            return false;
        }
        self.data
            .read()
            .events()
            .iter()
            .any(|event| prefix.matches(&event.hash))
    }

    /// Whether `prefix` names `event_id` specifically, for links that already
    /// identify one event.
    pub fn event_hash_matches(&self, event_id: usize, prefix: EventHashPrefix) -> bool {
        if prefix.is_genesis() {
            return false;
        }
        self.data
            .read()
            .events()
            .iter()
            .any(|event| event.event_id == event_id && prefix.matches(&event.hash))
    }
}

#[cfg(test)]
mod tests {
    use crate::{Event, Scope};

    use super::*;

    const TEST_ELECTION: ElectionConfig = ElectionConfig::EK27;

    #[derive(Default)]
    struct TestData {
        events: Vec<StoreEvent<usize>>,
        applied: Vec<usize>,
    }

    impl Event for usize {
        fn category(&self) -> &'static str {
            "number"
        }

        fn key(&self) -> &'static str {
            ""
        }

        fn description(&self, _locale: crate::Locale) -> String {
            "a number".to_string()
        }

        fn details(&self) -> String {
            self.to_string()
        }
    }

    impl StoreData for TestData {
        type Event = usize;

        fn apply(&mut self, event: StoreEvent<Self::Event>) {
            self.applied.push(event.payload);
            self.events.push(event);
        }

        fn events(&self) -> &[StoreEvent<Self::Event>] {
            &self.events
        }

        fn scope() -> Scope {
            Scope::PoliticalGroup
        }
    }

    fn test_store() -> Store<TestData> {
        Store {
            stream_id: StreamId::new(),
            election: TEST_ELECTION,
            backend: StoreBackend::Memory {
                store: MemoryStore::default(),
            },
            data: Arc::new(parking_lot::RwLock::new(TestData::default())),
        }
    }

    #[test]
    fn usize_event_trait_impl() {
        let v: usize = 37;
        assert_eq!(v.category(), "number");
        assert_eq!(v.key(), "");
        assert_eq!(v.description(crate::Locale::En), "a number");
        assert_eq!(v.details(), "37");
    }

    #[test]
    fn apply_event_updates_projection_and_last_event_id() {
        let store = test_store();

        store.apply_event(StoreEvent::new(1, 42));

        let data = store.data.read();
        assert_eq!(data.last_event_id(), 1);
        assert_eq!(data.applied, vec![42]);
    }

    /// Applies an event carrying `hash` as its chain hash.
    fn apply_hashed(store: &Store<TestData>, event_id: usize, hash: EventHash) {
        let mut event = StoreEvent::new(event_id, event_id);
        event.hash = hash;
        store.apply_event(event);
    }

    #[test]
    fn event_hash_lookups_accept_only_this_stream_s_events() {
        let store = test_store();
        apply_hashed(&store, 1, [0x11; 32]);
        apply_hashed(&store, 2, [0x22; 32]);

        assert!(store.has_event_hash(EventHashPrefix::of(&[0x11; 32])));
        assert!(store.has_event_hash(EventHashPrefix::of(&[0x22; 32])));
        assert!(!store.has_event_hash(EventHashPrefix::of(&[0xEE; 32])));

        // bound to one event, so a sibling's hash does not open it
        assert!(store.event_hash_matches(2, EventHashPrefix::of(&[0x22; 32])));
        assert!(!store.event_hash_matches(2, EventHashPrefix::of(&[0x11; 32])));
        assert!(!store.event_hash_matches(3, EventHashPrefix::of(&[0x22; 32])));
    }

    /// The genesis placeholder is shared by every stream, so it must never
    /// name an event, even once one carries it.
    #[test]
    fn the_genesis_hash_never_matches() {
        let store = test_store();
        apply_hashed(&store, 1, crate::store::GENESIS_HASH);

        let genesis = EventHashPrefix::of(&crate::store::GENESIS_HASH);
        assert!(!store.has_event_hash(genesis));
        assert!(!store.event_hash_matches(1, genesis));
    }

    #[test]
    fn apply_event_skips_when_already_up_to_date() {
        let store = test_store();

        {
            let mut data = store.data.write();
            data.events.push(StoreEvent::new(2, 0));
        }

        store.apply_event(StoreEvent::new(1, 7));

        let data = store.data.read();
        assert_eq!(data.last_event_id(), 2);
        assert!(data.applied.is_empty());
    }
}
