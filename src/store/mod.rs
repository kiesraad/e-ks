pub(crate) mod crypto;
#[cfg(feature = "database")]
pub(crate) mod database;

pub(crate) mod persistence;

mod event;
mod filesystem;
mod health;
pub(crate) mod memory;
mod registry;
mod store_handle;
mod stream_id;

pub(crate) use event::EncryptedEvent;
pub use event::{Event, EventHash, GENESIS_HASH, StoreEvent};
pub use health::{DbHealth, run_db_prober};
pub use persistence::StorePersistence;
pub use registry::StoreRegistry;
pub use store_handle::Store;
#[cfg(test)]
pub(crate) use store_handle::StoreBackend;
pub use stream_id::StreamId;

pub(crate) use event::{chain_hash, event_aad};

use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;

use crate::{
    AppError, ElectionConfig, Scope,
    crypto::{EventCipher, EventDecryptError},
};

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
#[derive(Clone, Debug)]
pub(crate) struct Replay {
    /// Chain hash of the highest-numbered *stored* event seen, applied or not.
    /// The hash a new event must chain onto.
    pub chain_tip: EventHash,
    /// Set to the event id where replay stopped applying payloads, if any.
    pub truncated_at: Option<usize>,
}

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

/// Decrypt persisted events and apply the ones `data` has not seen yet.
///
/// `events` yields the stored events in ascending event order; events at or
/// below the projection's last ID are skipped. Shared by the filesystem and
/// database backends.
///
/// A payload this build can no longer decode does not fail the load: replay
/// stops there and reports the id in [`Replay::truncated_at`], leaving the
/// projection deliberately incomplete, so callers must refuse to append on top
/// of it. Unreadable bytes and a broken hash chain stay hard errors.
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
