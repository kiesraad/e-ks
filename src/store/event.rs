//! Event wrapper used by the event-sourced store.
//!
//! Persisted events form a hash chain: each event carries a [`StoreEvent::hash`]
//! computed from the previous event's hash plus this event's metadata and stored
//! body. See [`chain_hash`] for the exact construction and the project README
//! ("Event hash chain") for the rationale.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Locale, structs::audit_log::FieldChange};

/// SHA-256 digest linking one persisted event to the previous one.
pub type EventHash = [u8; 32];

/// Hash linked to the (virtual) event preceding the first event of a stream.
pub const GENESIS_HASH: EventHash = [0u8; 32];

/// One event as it sits in a backend's storage: payload still encrypted.
pub(crate) struct EncryptedEvent {
    pub event_id: usize,
    pub created_at: DateTime<Utc>,
    pub hash: EventHash,
    pub payload: Vec<u8>,
}

pub trait Event {
    /// Return a stable category key for filtering in the audit log
    fn category(&self) -> &'static str;

    /// Return a stable snake_case key identifying the event variant
    ///
    /// Variants that share a user-facing description share a key (e.g. both
    /// `CreatePerson` and `CreatePersonPersonalData` map to `create_person`).
    fn key(&self) -> &'static str;

    /// Translated label describing what the event did
    fn description(&self, locale: Locale) -> String;

    /// Short human-readable details for a listing row (name, file, districts, ...)
    fn details(&self) -> String;

    /// Field-level changes to show in the audit log detail view. Returns an
    /// empty vec for events that have no structured change data.
    fn changes(&self, _locale: Locale) -> Vec<FieldChange> {
        vec![]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreEvent<E> {
    /// Monotonic event identifier within a stream.
    pub event_id: usize,
    /// Domain-specific event payload.
    pub payload: E,
    /// Timestamp recorded when the event was created.
    pub created_at: DateTime<Utc>,
    /// Chain hash of this event: `SHA256(prev_hash ‖ event_id ‖ created_at ‖ body)`,
    /// where `body` is the persisted representation of the payload (the AES-GCM
    /// blob for the file/database backends, the plain encoding for the in-memory
    /// backend). Stored unencrypted; see [`chain_hash`] and the README.
    pub hash: EventHash,
}

impl<E: Event> StoreEvent<E> {
    /// Construct a store event with a placeholder ([`GENESIS_HASH`]) chain hash
    /// and `created_at` set to now.
    ///
    /// Persistence backends never use this — they build [`StoreEvent`] with a
    /// real chain hash via [`chain_hash`]. It exists for tests and ad-hoc
    /// projections where the hash is irrelevant.
    pub fn new(event_id: usize, payload: E) -> Self {
        Self::new_at(event_id, payload, Utc::now())
    }

    /// Like [`StoreEvent::new`] but with an explicit timestamp.
    pub fn new_at(event_id: usize, payload: E, created_at: DateTime<Utc>) -> Self {
        Self {
            event_id,
            payload,
            created_at,
            hash: GENESIS_HASH,
        }
    }
}

/// Associated data mixed into AES-256-GCM when (de)crypting an event payload.
///
/// It authenticates the cleartext metadata stored next to the ciphertext
/// (`event_id`, `created_at`) and pins the ciphertext to its position in the
/// chain (`prev_hash`), so a ciphertext cannot be replayed at a different
/// offset without the tag check failing.
pub(crate) fn event_aad(
    event_id: usize,
    created_at: DateTime<Utc>,
    prev_hash: &EventHash,
) -> Vec<u8> {
    let mut aad = Vec::with_capacity(8 + 8 + 32);
    aad.extend_from_slice(&(event_id as u64).to_le_bytes());
    aad.extend_from_slice(&created_at.timestamp_micros().to_le_bytes());
    aad.extend_from_slice(prev_hash);
    aad
}

/// Compute the chain hash for an event.
///
/// `body` is the bytes that get persisted for this event: the `nonce ‖ ciphertext
/// ‖ tag` blob for the file and database backends, or the CBOR encoding of the
/// plaintext payload for the in-memory backend. Hashing the *encrypted* blob (which
/// is indistinguishable from random and carries a fresh nonce) is what makes it
/// safe to store the hash unencrypted: it commits to the stored event without
/// leaking anything about the plaintext.
pub(crate) fn chain_hash(
    prev_hash: &EventHash,
    event_id: usize,
    created_at: DateTime<Utc>,
    body: &[u8],
) -> EventHash {
    let mut hasher = Sha256::new();
    hasher.update(prev_hash);
    hasher.update((event_id as u64).to_le_bytes());
    hasher.update(created_at.timestamp_micros().to_le_bytes());
    hasher.update(body);

    let digest = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&digest);
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_sets_timestamp_and_fields() {
        let before = Utc::now();
        let event = StoreEvent::new(3, 37);
        let after = Utc::now();

        assert_eq!(event.event_id, 3);
        assert_eq!(event.payload, 37);
        assert!(event.created_at >= before);
        assert!(event.created_at <= after);
        assert_eq!(event.hash, GENESIS_HASH);
    }

    #[test]
    fn new_at_uses_provided_timestamp() {
        let timestamp = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let event = StoreEvent::new_at(7, 37, timestamp);

        assert_eq!(event.event_id, 7);
        assert_eq!(event.payload, 37);
        assert_eq!(event.created_at, timestamp);
    }

    #[test]
    fn chain_hash_depends_on_every_input() {
        let ts = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let base = chain_hash(&GENESIS_HASH, 1, ts, b"body");

        assert_ne!(base, chain_hash(&[1u8; 32], 1, ts, b"body"));
        assert_ne!(base, chain_hash(&GENESIS_HASH, 2, ts, b"body"));
        assert_ne!(
            base,
            chain_hash(&GENESIS_HASH, 1, ts + chrono::Duration::seconds(1), b"body")
        );
        assert_ne!(base, chain_hash(&GENESIS_HASH, 1, ts, b"body!"));
        // Deterministic.
        assert_eq!(base, chain_hash(&GENESIS_HASH, 1, ts, b"body"));
    }

    #[test]
    fn chain_hash_only_uses_microsecond_precision() {
        // created_at sub-microsecond digits don't survive a round-trip through
        // the file frame / Postgres timestamptz, so they must not affect the hash.
        let micros = DateTime::from_timestamp_micros(1_700_000_000_123_456).unwrap();
        let with_nanos = micros + chrono::Duration::nanoseconds(789);
        assert_eq!(
            chain_hash(&GENESIS_HASH, 1, micros, b"body"),
            chain_hash(&GENESIS_HASH, 1, with_nanos, b"body"),
        );
    }

    #[test]
    fn event_aad_is_stable_and_distinct() {
        let ts = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        assert_eq!(
            event_aad(1, ts, &GENESIS_HASH),
            event_aad(1, ts, &GENESIS_HASH)
        );
        assert_ne!(
            event_aad(1, ts, &GENESIS_HASH),
            event_aad(2, ts, &GENESIS_HASH)
        );
        assert_ne!(
            event_aad(1, ts, &GENESIS_HASH),
            event_aad(1, ts, &[7u8; 32])
        );
    }
}
