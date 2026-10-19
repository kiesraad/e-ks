//! Property suite for the at-rest encryption (`bin/fuzz props`).

use proptest::{collection::vec, prelude::*, sample::select};
use secrecy::SecretString;

use crate::{
    ElectionConfig, StreamId,
    crypto::{EventDecryptError, MasterKey, StreamKey, WrappedKey},
    test_utils::proptest_config,
};

fn stream_id() -> impl Strategy<Value = StreamId> {
    any::<[u8; 16]>().prop_map(|bytes| StreamId::from(uuid::Uuid::from_bytes(bytes)))
}

fn election() -> impl Strategy<Value = ElectionConfig> {
    select(ElectionConfig::all())
}

fn master() -> MasterKey {
    MasterKey::new(&SecretString::from("proptest master secret"))
}

proptest! {
    #![proptest_config(proptest_config(256))]

    /// A wrapped key unwraps only for the stream and election it was wrapped for.
    #[test]
    fn wrapped_key_is_bound_to_its_stream(
        (stream, election) in (stream_id(), election()),
        (other_stream, other_election) in (stream_id(), election()),
        same_stream in any::<bool>(),
        same_election in any::<bool>(),
    ) {
        let master = master();
        let wrapped = master.wrap_key(&StreamKey::generate(), stream, election).expect("wrap");
        let stream_b = if same_stream { stream } else { other_stream };
        let election_b = if same_election { election } else { other_election };
        let bound = stream_b == stream && election_b.stable_id() == election.stable_id();
        prop_assert_eq!(master.unwrap_key(&wrapped, stream_b, election_b).is_ok(), bound);
    }

    /// Any change to a wrapped key fails the unwrap, and never panics.
    #[test]
    fn tampered_wrapped_key_is_rejected(
        (stream, election) in (stream_id(), election()),
        bit in any::<usize>(),
        cut in proptest::option::of(any::<usize>()),
    ) {
        let master = master();
        let wrapped = master.wrap_key(&StreamKey::generate(), stream, election).expect("wrap");
        let mut bytes = wrapped.as_bytes().to_vec();
        match cut {
            Some(cut) => bytes.truncate(cut % bytes.len()),
            None => {
                let bit = bit % (bytes.len() * 8);
                bytes[bit / 8] ^= 1 << (bit % 8);
            }
        }
        prop_assert!(master.unwrap_key(&WrappedKey::from(bytes), stream, election).is_err());
    }

    /// A payload decrypts only unchanged, under its own key and associated
    /// data; anything else is `Unreadable`, never a panic or another value.
    #[test]
    fn event_payload_is_authenticated(
        payload in vec(any::<u8>(), 0..256),
        aad in vec(any::<u8>(), 0..64),
        change in 0u8..4,
        position in any::<usize>(),
    ) {
        let key = StreamKey::generate();
        let cipher = key.cipher();
        let mut blob = cipher.encrypt(&payload, &aad).expect("encrypt");
        let mut aad_b = aad.clone();
        let cipher_b = match change {
            0 => cipher.clone(),
            1 => {
                let bit = position % (blob.len() * 8);
                blob[bit / 8] ^= 1 << (bit % 8);
                cipher.clone()
            }
            2 => {
                if aad_b.is_empty() {
                    aad_b.push(0);
                } else {
                    let bit = position % (aad_b.len() * 8);
                    aad_b[bit / 8] ^= 1 << (bit % 8);
                }
                cipher.clone()
            }
            _ => StreamKey::generate().cipher(),
        };
        let decrypted = cipher_b.decrypt::<Vec<u8>>(blob, &aad_b);
        if change == 0 {
            prop_assert_eq!(decrypted.expect("unchanged payload decrypts"), payload);
        } else {
            prop_assert!(matches!(decrypted, Err(EventDecryptError::Unreadable(_))));
        }
    }

    /// Truncated ciphertext is `Unreadable` at every length.
    #[test]
    fn truncated_payload_is_rejected(payload in vec(any::<u8>(), 0..64), cut in any::<usize>()) {
        let cipher = StreamKey::generate().cipher();
        let mut blob = cipher.encrypt(&payload, b"aad").expect("encrypt");
        blob.truncate(cut % blob.len());
        prop_assert!(matches!(
            cipher.decrypt::<Vec<u8>>(blob, b"aad"),
            Err(EventDecryptError::Unreadable(_))
        ));
    }
}
