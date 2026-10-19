//! Property suite for replaying persisted events (`bin/fuzz props`): what a
//! database writer without the keys can make a replay accept, and whether
//! incremental replays with live snapshots match a replay from scratch.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use proptest::{collection::vec, prelude::*};

use super::{
    EncryptedEvent, EventHash, GENESIS_HASH, StoreData, StoreEvent, apply_encrypted_events,
    chain_hash, event_aad,
};
use crate::{
    Scope,
    crypto::{EventCipher, StreamKey},
    test_utils::proptest_config,
};

#[derive(Default, Clone, Debug)]
struct Events(Vec<StoreEvent<usize>>);

impl StoreData for Events {
    type Event = usize;

    fn apply(&mut self, event: StoreEvent<usize>) {
        self.0.push(event);
    }

    fn events(&self) -> &[StoreEvent<usize>] {
        &self.0
    }

    fn scope() -> Scope {
        Scope::PoliticalGroup
    }
}

/// `(event_id, payload, created_at, hash)`: an applied event, comparable.
type Applied = (usize, usize, DateTime<Utc>, EventHash);

fn applied(data: &Events) -> Vec<Applied> {
    data.0
        .iter()
        .map(|e| (e.event_id, e.payload, e.created_at, e.hash))
        .collect()
}

fn start() -> DateTime<Utc> {
    DateTime::from_timestamp(1_700_000_000, 0).expect("valid timestamp")
}

/// Encrypt and chain `payloads` as events `1..=n`, as a backend stores them.
fn seal_chain(cipher: &EventCipher, payloads: &[usize]) -> Vec<EncryptedEvent> {
    let mut prev_hash = GENESIS_HASH;
    (1..)
        .zip(payloads)
        .map(|(event_id, payload)| {
            let created_at =
                start() + Duration::seconds(i64::try_from(payload % 1000).expect("small"));
            let aad = event_aad(event_id, created_at, &prev_hash);
            let payload = cipher.encrypt(payload, &aad).expect("encrypt");
            let hash = chain_hash(&prev_hash, event_id, created_at, &payload);
            prev_hash = hash;
            EncryptedEvent {
                event_id,
                created_at,
                hash,
                payload,
            }
        })
        .collect()
}

fn copy(event: &EncryptedEvent) -> EncryptedEvent {
    EncryptedEvent {
        event_id: event.event_id,
        created_at: event.created_at,
        hash: event.hash,
        payload: event.payload.clone(),
    }
}

/// What someone with write access to the event table, but not the keys,
/// can do to the stored rows.
#[derive(Debug, Clone)]
enum Tamper {
    FlipPayload {
        event: usize,
        bit: usize,
    },
    TruncatePayload {
        event: usize,
        len: usize,
    },
    FlipHash {
        event: usize,
        bit: u8,
    },
    ShiftId {
        event: usize,
        by: i8,
    },
    ShiftTime {
        event: usize,
        micros: i32,
    },
    Swap(usize, usize),
    Remove(usize),
    Duplicate(usize),
    /// Another event's payload in this row.
    MovePayload {
        from: usize,
        to: usize,
    },
    /// A row sealed under another stream's key.
    Foreign {
        at: usize,
    },
}

fn tamper() -> impl Strategy<Value = Tamper> {
    let at = any::<usize>();
    prop_oneof![
        (at, any::<usize>()).prop_map(|(event, bit)| Tamper::FlipPayload { event, bit }),
        (at, any::<usize>()).prop_map(|(event, len)| Tamper::TruncatePayload { event, len }),
        (at, any::<u8>()).prop_map(|(event, bit)| Tamper::FlipHash { event, bit }),
        (at, any::<i8>()).prop_map(|(event, by)| Tamper::ShiftId { event, by }),
        (at, any::<i32>()).prop_map(|(event, micros)| Tamper::ShiftTime { event, micros }),
        (at, any::<usize>()).prop_map(|(a, b)| Tamper::Swap(a, b)),
        at.prop_map(Tamper::Remove),
        at.prop_map(Tamper::Duplicate),
        (at, any::<usize>()).prop_map(|(from, to)| Tamper::MovePayload { from, to }),
        at.prop_map(|at| Tamper::Foreign { at }),
    ]
}

impl Tamper {
    fn apply(&self, rows: &mut Vec<EncryptedEvent>) {
        if rows.is_empty() {
            return;
        }
        let len = rows.len();
        let index = |i: &usize| i % len;
        match self {
            Self::FlipPayload { event, bit } => {
                let payload = &mut rows[index(event)].payload;
                if !payload.is_empty() {
                    let bit = bit % (payload.len() * 8);
                    payload[bit / 8] ^= 1 << (bit % 8);
                }
            }
            Self::TruncatePayload { event, len } => {
                let payload = &mut rows[index(event)].payload;
                payload.truncate(len % (payload.len() + 1));
            }
            Self::FlipHash { event, bit } => {
                let bit = usize::from(*bit);
                rows[index(event)].hash[bit / 8] ^= 1 << (bit % 8);
            }
            Self::ShiftId { event, by } => {
                let row = &mut rows[index(event)];
                row.event_id = row.event_id.saturating_add_signed(isize::from(*by));
            }
            Self::ShiftTime { event, micros } => {
                rows[index(event)].created_at += Duration::microseconds(i64::from(*micros));
            }
            Self::Swap(a, b) => {
                let (a, b) = (index(a), index(b));
                rows.swap(a, b);
            }
            Self::Remove(at) => {
                rows.remove(index(at));
            }
            Self::Duplicate(at) => {
                let at = index(at);
                let row = copy(&rows[at]);
                rows.insert(at, row);
            }
            Self::MovePayload { from, to } => {
                let payload = rows[index(from)].payload.clone();
                let to = index(to);
                rows[to].payload = payload;
            }
            Self::Foreign { at } => {
                let foreign = seal_chain(&StreamKey::generate().cipher(), &[usize::MAX]);
                let at = index(at);
                let mut row = copy(&foreign[0]);
                row.event_id = rows[at].event_id;
                row.created_at = rows[at].created_at;
                row.hash = rows[at].hash;
                rows[at] = row;
            }
        }
    }
}

/// The projection as replay leaves it when the first `warm` events were
/// applied before (a cached store).
fn warm_projection(original: &[Applied], warm: usize) -> Arc<Events> {
    let mut data = Events::default();
    for &(event_id, payload, created_at, hash) in &original[..warm] {
        data.apply(StoreEvent {
            event_id,
            payload,
            created_at,
            hash,
        });
    }
    Arc::new(data)
}

fn original(cipher: &EventCipher, payloads: &[usize]) -> (Vec<EncryptedEvent>, Vec<Applied>) {
    let rows = seal_chain(cipher, payloads);
    let applied = rows
        .iter()
        .zip(payloads)
        .map(|(row, &payload)| (row.event_id, payload, row.created_at, row.hash))
        .collect();
    (rows, applied)
}

proptest! {
    #![proptest_config(proptest_config(512))]

    /// Untampered rows replay to exactly the stored history.
    #[test]
    fn untampered_rows_replay_completely(
        payloads in vec(any::<usize>(), 0..12),
        warm in any::<usize>(),
    ) {
        let cipher = StreamKey::generate().cipher();
        let (rows, expected) = original(&cipher, &payloads);
        let mut data = warm_projection(&expected, warm % (payloads.len() + 1));

        let replay = apply_encrypted_events(&mut data, &cipher, rows).expect("replay");

        prop_assert_eq!(applied(&data), expected.clone());
        prop_assert_eq!(replay.chain_tip, expected.last().map_or(GENESIS_HASH, |e| e.3));
        prop_assert_eq!(replay.truncated_at, None);
    }

    /// Tampered rows fail the replay, or yield a prefix of the real history:
    /// never an event that was not stored, or one out of place.
    #[test]
    fn tampered_rows_never_alter_history(
        payloads in vec(any::<usize>(), 1..10),
        warm in any::<usize>(),
        tampers in vec(tamper(), 1..4),
    ) {
        let cipher = StreamKey::generate().cipher();
        let (mut rows, expected) = original(&cipher, &payloads);
        let warm = warm % (payloads.len() + 1);
        for tamper in &tampers {
            tamper.apply(&mut rows);
        }
        let mut data = warm_projection(&expected, warm);

        let Ok(replay) = apply_encrypted_events(&mut data, &cipher, rows) else {
            return Ok(());
        };
        let got = applied(&data);
        prop_assert!(got.len() >= warm, "replay dropped applied events");
        prop_assert!(got.len() <= expected.len());
        // Without the hash chain check, nothing verifies the last applied
        // row's own hash: only a successor's associated data would.
        let strict = cfg!(feature = "verify-event-hash-chain");
        for (index, (event, want)) in got.iter().zip(&expected).enumerate() {
            prop_assert_eq!((event.0, event.1, event.2), (want.0, want.1, want.2));
            if strict || index + 1 < got.len() {
                prop_assert_eq!(event.3, want.3, "event {} has a forged hash", event.0);
            }
        }
        prop_assert_eq!(replay.truncated_at, None);
    }

    /// Replaying in overlapping batches, with a snapshot held after each, ends
    /// where one replay ends, and leaves every snapshot as it was taken.
    #[test]
    fn batched_replay_matches_full_replay(
        payloads in vec(any::<usize>(), 0..16),
        cuts in vec((any::<usize>(), any::<usize>()), 0..6),
    ) {
        let cipher = StreamKey::generate().cipher();
        let (rows, expected) = original(&cipher, &payloads);

        let mut data = Arc::new(Events::default());
        let mut snapshots: Vec<(Arc<Events>, Vec<Applied>)> = Vec::new();
        for (end, overlap) in cuts {
            let end = end % (rows.len() + 1);
            let begin = data.0.len().saturating_sub(overlap % 4).min(end);
            let batch = rows[begin..end].iter().map(copy);
            apply_encrypted_events(&mut data, &cipher, batch).expect("batch replay");
            snapshots.push((Arc::clone(&data), applied(&data)));
        }
        apply_encrypted_events(&mut data, &cipher, rows.iter().map(copy)).expect("final replay");

        prop_assert_eq!(applied(&data), expected);
        for (snapshot, taken) in snapshots {
            prop_assert_eq!(applied(&snapshot), taken, "a later replay changed a snapshot");
        }
    }
}
