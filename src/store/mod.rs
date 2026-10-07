pub(crate) mod crypto;
#[cfg(feature = "database")]
pub(crate) mod database;

pub(crate) mod persistence;

mod encoding;
mod event;
mod event_hash_prefix;
mod health;
pub(crate) mod memory;
mod registry;
mod store_handle;
mod stream_id;

#[cfg(feature = "database")]
pub(crate) use event::EncryptedEvent;
pub use event::{Event, EventHash, GENESIS_HASH, StoreEvent};
pub use event_hash_prefix::EventHashPrefix;
pub use health::{DbHealth, run_db_prober};
pub use persistence::StorePersistence;
pub use registry::{StoreRegistry, run_store_cache_sweeper};
pub use store_handle::Store;
#[cfg(test)]
pub(crate) use store_handle::StoreBackend;
pub use stream_id::StreamId;

pub(crate) use event::chain_hash;
#[cfg(feature = "database")]
pub(crate) use event::event_aad;

use chrono::{DateTime, Utc};
#[cfg(feature = "database")]
use serde::de::DeserializeOwned;

#[cfg(feature = "database")]
use crate::crypto::{EventCipher, EventDecryptError};
use crate::{AppError, ElectionConfig, Scope};

/// Decryption-free metadata about a persisted stream, read from the backend's
/// index without replaying (or warming) it. The political group name is absent:
/// it lives in the encrypted payloads.
#[derive(Clone, Debug)]
pub struct StreamMeta {
    pub stream_id: StreamId,
    pub election: ElectionConfig,
    /// Number of events, i.e. the last event id (events are appended `1..=n`).
    pub event_count: usize,
    pub created_at: Option<DateTime<Utc>>,
    pub last_event_at: Option<DateTime<Utc>>,
}

pub trait StoreData: Default + Send + Sync + 'static {
    type Event: Event;

    /// Apply a fully wrapped store event to the data projection.
    fn apply(&mut self, event: StoreEvent<Self::Event>);

    /// All events applied to this projection, in order.
    fn events(&self) -> &[StoreEvent<Self::Event>];

    /// Return the last applied event ID for this data instance.
    fn last_event_id(&self) -> usize {
        self.events().last().map(|e| e.event_id).unwrap_or(0)
    }

    /// Return the chain hash of the last applied event, or [`GENESIS_HASH`] if
    /// no events have been applied yet.
    fn last_event_hash(&self) -> EventHash {
        self.events().last().map(|e| e.hash).unwrap_or(GENESIS_HASH)
    }

    fn scope() -> Scope;
}

/// What a replay pass did with the stored events it was handed.
///
/// Callers need `chain_tip` (not the projection's last applied hash) to append
/// a new event, because the two differ once a replay was truncated.
#[cfg(feature = "database")]
#[derive(Clone, Debug)]
pub(crate) struct Replay {
    /// Chain hash of the highest-numbered *stored* event seen, applied or not.
    /// The hash a new event must chain onto.
    pub chain_tip: EventHash,
    /// Set to the event id where replay stopped applying payloads, if any.
    pub truncated_at: Option<usize>,
}

#[cfg(feature = "database")]
impl Replay {
    /// Refuse to append to a stream whose replay was truncated.
    ///
    /// A new event would land after the gap, so it would be stored but never
    /// applied on the next load: the write would look like it succeeded and
    /// then vanish on restart. The stream is readable (as of the last event
    /// before the gap) but not writable until the build can decode its events
    /// again.
    pub(crate) fn reject_append(&self, stream_id: StreamId) -> Result<(), AppError> {
        match self.truncated_at {
            None => Ok(()),
            Some(event_id) => Err(AppError::EventDecodeError(format!(
                "stream {stream_id} stopped replaying at event {event_id}: \
                 refusing to append to a stream this build cannot fully read"
            ))),
        }
    }
}

/// Refuse an append when the stream moved past the id the caller validated
/// against.
pub(crate) fn check_expected_event_id(
    expected: Option<usize>,
    last_id: usize,
) -> Result<(), AppError> {
    match expected {
        Some(expected) if expected != last_id => Err(AppError::Conflict),
        _ => Ok(()),
    }
}

/// Decrypt persisted events and apply the ones `data` has not seen yet.
///
/// `events` yields the stored events in ascending event order; events at or
/// below the projection's last ID are skipped.
///
/// A payload this build can no longer decode does not fail the load: replay
/// stops there and reports the id in [`Replay::truncated_at`], leaving the
/// projection deliberately incomplete, so callers must refuse to append on top
/// of it. Unreadable bytes and a broken hash chain stay hard errors.
#[cfg(feature = "database")]
pub(crate) fn apply_encrypted_events<D>(
    data: &mut D,
    cipher: &EventCipher,
    events: impl IntoIterator<Item = EncryptedEvent>,
) -> Result<Replay, AppError>
where
    D: StoreData,
    D::Event: DeserializeOwned,
{
    let mut prev_hash = data.last_event_hash();
    let mut chain_tip = prev_hash;
    let mut truncated_at = None;

    for EncryptedEvent {
        event_id,
        created_at,
        hash,
        payload: encrypted_payload,
    } in events
    {
        if truncated_at.is_none() && data.last_event_id() >= event_id {
            chain_tip = hash;
            continue;
        }

        // Verify the chain over the stored blob before touching the plaintext.
        // Gated behind a feature flag: it costs a SHA-256 over every loaded
        // event. (Reordering, removal, and in-place edits are still caught by
        // the AES-GCM tag, since `prev_hash` is part of the associated data.)
        #[cfg(feature = "verify-event-hash-chain")]
        if chain_hash(&prev_hash, event_id, created_at, &encrypted_payload) != hash {
            return Err(AppError::EventDecodeError(format!(
                "hash chain broken at event {event_id}"
            )));
        }

        let aad = event_aad(event_id, created_at, &prev_hash);
        // The chain is walked over every stored event, including the ones past
        // a truncation point: it needs only the encrypted blob, and callers
        // need the real tip to append.
        prev_hash = hash;
        chain_tip = hash;

        if truncated_at.is_some() {
            continue;
        }

        match cipher.decrypt::<D::Event>(encrypted_payload, &aad) {
            Ok(payload) => data.apply(StoreEvent {
                event_id,
                payload,
                created_at,
                hash,
            }),
            Err(err @ EventDecryptError::Unreadable(_)) => return Err(err.into()),
            Err(EventDecryptError::IncompatiblePayload(err)) => {
                tracing::error!(
                    event_id,
                    error = %err,
                    "event payload does not match this build's event type; \
                     replay stops here and the projection stays at event {}",
                    event_id.saturating_sub(1)
                );
                truncated_at = Some(event_id);
            }
        }
    }

    Ok(Replay {
        chain_tip,
        truncated_at,
    })
}

#[cfg(all(test, feature = "database"))]
mod tests {
    use serde::Serialize;

    use super::*;
    use crate::crypto::StreamKey;

    #[derive(Default)]
    struct TestData {
        events: Vec<StoreEvent<usize>>,
    }

    impl StoreData for TestData {
        type Event = usize;

        fn apply(&mut self, event: StoreEvent<usize>) {
            self.events.push(event);
        }

        fn events(&self) -> &[StoreEvent<usize>] {
            &self.events
        }

        fn scope() -> Scope {
            Scope::PoliticalGroup
        }
    }

    impl TestData {
        fn payloads(&self) -> Vec<usize> {
            self.events.iter().map(|e| e.payload).collect()
        }
    }

    fn created_at() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("valid timestamp")
    }

    /// Encrypt and chain one event as a backend stores it.
    fn seal(
        cipher: &EventCipher,
        event_id: usize,
        prev_hash: &EventHash,
        payload: &impl Serialize,
    ) -> EncryptedEvent {
        let aad = event_aad(event_id, created_at(), prev_hash);
        let payload = cipher.encrypt(payload, &aad).expect("encrypt");
        EncryptedEvent {
            event_id,
            created_at: created_at(),
            hash: chain_hash(prev_hash, event_id, created_at(), &payload),
            payload,
        }
    }

    /// Seal `payloads` as events `1..=n` of one stream.
    fn seal_chain(cipher: &EventCipher, payloads: &[usize]) -> Vec<EncryptedEvent> {
        let mut prev_hash = GENESIS_HASH;
        (1..)
            .zip(payloads)
            .map(|(event_id, payload)| {
                let event = seal(cipher, event_id, &prev_hash, payload);
                prev_hash = event.hash;
                event
            })
            .collect()
    }

    #[test]
    fn replay_applies_events_in_order() -> Result<(), AppError> {
        let cipher = StreamKey::generate().cipher();
        let events = seal_chain(&cipher, &[10, 20]);
        let tip = events[1].hash;

        let mut data = TestData::default();
        let replay = apply_encrypted_events(&mut data, &cipher, events)?;

        assert_eq!(data.payloads(), vec![10, 20]);
        assert_eq!(data.last_event_id(), 2);
        assert_eq!(data.last_event_hash(), tip);
        assert_eq!(replay.chain_tip, tip);
        assert_eq!(replay.truncated_at, None);

        Ok(())
    }

    #[test]
    fn replay_skips_events_already_applied() -> Result<(), AppError> {
        let cipher = StreamKey::generate().cipher();
        let events = seal_chain(&cipher, &[10, 20]);

        // Another instance already applied event 1.
        let mut data = TestData::default();
        data.apply(StoreEvent {
            event_id: 1,
            payload: 10,
            created_at: created_at(),
            hash: events[0].hash,
        });
        apply_encrypted_events(&mut data, &cipher, events)?;

        assert_eq!(data.payloads(), vec![10, 20]);

        Ok(())
    }

    #[test]
    fn a_tampered_payload_fails_the_replay() {
        let cipher = StreamKey::generate().cipher();
        let mut events = seal_chain(&cipher, &[10, 20]);
        // Flip the last byte of event 1 (its GCM tag).
        if let Some(byte) = events[0].payload.last_mut() {
            *byte ^= 0x01;
        }

        let mut data = TestData::default();
        let err = apply_encrypted_events(&mut data, &cipher, events)
            .expect_err("tampering must be detected");

        assert!(matches!(err, AppError::EventDecodeError(_)));
        assert!(data.events.is_empty());
    }

    #[test]
    fn another_streams_key_cannot_replay_events() {
        let events = seal_chain(&StreamKey::generate().cipher(), &[10]);

        let mut data = TestData::default();
        let err = apply_encrypted_events(&mut data, &StreamKey::generate().cipher(), events)
            .expect_err("replay must fail with the wrong key");

        assert!(matches!(err, AppError::EventDecodeError(_)));
        assert!(data.events.is_empty());
    }

    #[test]
    fn an_undecodable_payload_truncates_the_replay() -> Result<(), AppError> {
        let cipher = StreamKey::generate().cipher();
        // Event 2 decrypts but is not a `usize`, as after an event type change.
        let first = seal(&cipher, 1, &GENESIS_HASH, &10usize);
        let second = seal(&cipher, 2, &first.hash, &"unreadable");
        let third = seal(&cipher, 3, &second.hash, &30usize);
        let (hash1, hash3) = (first.hash, third.hash);

        let mut data = TestData::default();
        let replay = apply_encrypted_events(&mut data, &cipher, [first, second, third])?;

        assert_eq!(replay.truncated_at, Some(2));
        // The chain is walked past the gap: the tip is the last stored event.
        assert_eq!(replay.chain_tip, hash3);
        assert_eq!(data.payloads(), vec![10]);
        assert_eq!(data.last_event_hash(), hash1);

        // Reading degrades gracefully, appending must not.
        assert!(matches!(
            replay.reject_append(StreamId::new()),
            Err(AppError::EventDecodeError(_))
        ));

        Ok(())
    }

    #[cfg(feature = "verify-event-hash-chain")]
    #[test]
    fn a_rewritten_hash_fails_the_replay() {
        let cipher = StreamKey::generate().cipher();
        let mut events = seal_chain(&cipher, &[10]);
        // AES-GCM does not cover the stored hash; only the chain check does.
        events[0].hash[0] ^= 0x01;

        let mut data = TestData::default();
        let err = apply_encrypted_events(&mut data, &cipher, events)
            .expect_err("a rewritten hash must be detected");

        assert!(matches!(err, AppError::EventDecodeError(_)));
        assert!(data.events.is_empty());
    }
}
