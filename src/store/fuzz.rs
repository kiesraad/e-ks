//! Fuzz harness for decoding stored event payloads (see `crate::fuzzing`).
//!
//! A replay decodes every payload a backend hands back; a panic there takes
//! the stream down, and a lossy decode would silently change history.

use ciborium::Value;
use serde::{Serialize, de::DeserializeOwned};

use super::encoding::{decode, encode};
use crate::{CsbEvent, CsbMainEvent, PgEvent};

/// The first byte picks the event type, the rest is the decrypted payload.
pub fn store_event_decode(data: &[u8]) {
    let Some((kind, payload)) = data.split_first() else {
        return;
    };
    match kind % 3 {
        0 => round_trip::<PgEvent>(payload),
        1 => round_trip::<CsbEvent>(payload),
        _ => round_trip::<CsbMainEvent>(payload),
    }
}

/// Whatever decodes encodes again to a payload that decodes to the same value.
fn round_trip<E: Serialize + DeserializeOwned>(payload: &[u8]) {
    let Ok(event) = decode::<E>(payload) else {
        return;
    };
    let encoded = encode(&event).expect("a decoded event encodes");
    let again: E = decode(&encoded)
        .unwrap_or_else(|error| panic!("an encoded event does not decode: {error}"));
    let reencoded = encode(&again).expect("a decoded event encodes");
    assert_eq!(
        canonical(&reencoded),
        canonical(&encoded),
        "decoding changed the event"
    );
}

/// The CBOR value with map entries sorted: `HashMap`s encode in any order.
fn canonical(bytes: &[u8]) -> Value {
    sort_maps(ciborium::from_reader(bytes).expect("own encoding is CBOR"))
}

fn sort_maps(value: Value) -> Value {
    match value {
        Value::Map(entries) => {
            let mut entries: Vec<(Value, Value)> = entries
                .into_iter()
                .map(|(key, value)| (sort_maps(key), sort_maps(value)))
                .collect();
            entries.sort_by_cached_key(|(key, _)| encode(key).expect("CBOR value encodes"));
            Value::Map(entries)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sort_maps).collect()),
        Value::Tag(tag, inner) => Value::Tag(tag, Box::new(sort_maps(*inner))),
        other => other,
    }
}

/// A harness input for `event`; `kind` 0, 1, 2 picks the event type.
#[cfg(test)]
pub(crate) fn encode_seed(kind: u8, event: &impl Serialize) -> Vec<u8> {
    let mut seed = vec![kind];
    seed.extend(encode(event).expect("event encodes"));
    seed
}
