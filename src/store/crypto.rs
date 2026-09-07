//! Envelope encryption for event payloads.
//!
//! Each stream gets a random 256-bit [`StreamKey`], generated when the stream
//! is first created. Event payloads are postcard-serialized and encrypted with
//! it using AES-256-GCM ([`EventCipher`]). The stream key is stored only in
//! wrapped form: encrypted by the [`MasterKey`] (derived from the
//! `MASTER_ENCRYPTION_KEY` secret) and persisted next to the stream. The wrap
//! binds `(stream_id, election)` as associated data, so a wrapped key cannot
//! be moved to another stream.
//!
//! This is at-rest, defence-in-depth encryption, not end-to-end: the server
//! holds the master secret. The threat model is read access to the database or
//! files without access to the server's memory.

use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{AeadInOut, Generate, Nonce},
};
use hkdf::Hkdf;
use secrecy::{ExposeSecret, SecretBox, SecretString};
use serde::{Serialize, de::DeserializeOwned};
use sha2::Sha256;
use zeroize::{Zeroize, Zeroizing};

use crate::{AppError, ElectionConfig, StreamId};

/// Domain-separation salt, distinct from the BSN id-derivation salt.
const KEK_HKDF_SALT: &[u8] = b"e-KS stream key wrapping v1";
const KEK_HKDF_INFO: &[u8] = b"key-encryption-key:v1";
const WRAP_AAD_PREFIX: &[u8] = b"stream-key:";

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

/// Key-wrapping key derived from the master secret. Wraps and unwraps
/// per-stream [`StreamKey`]s; never encrypts event payloads itself.
#[derive(Clone)]
pub struct MasterKey {
    cipher: Aes256Gcm,
}

impl MasterKey {
    /// Derive the key-wrapping key from the master secret with HKDF-SHA256.
    pub fn new(secret: &SecretString) -> Self {
        let hk = Hkdf::<Sha256>::new(Some(KEK_HKDF_SALT), secret.expose_secret().as_bytes());
        let mut kek = Zeroizing::new([0u8; KEY_LEN]);
        hk.expand(KEK_HKDF_INFO, kek.as_mut())
            .expect("32 bytes is within HKDF-SHA256 output limit");

        let cipher = Aes256Gcm::new_from_slice(kek.as_ref())
            .expect("32 bytes is a valid AES-256 key length");

        Self { cipher }
    }

    /// Encrypt `key` for storage next to its stream, binding
    /// `(stream_id, election)` into the tag.
    pub fn wrap_key(
        &self,
        key: &StreamKey,
        stream_id: StreamId,
        election: ElectionConfig,
    ) -> Result<WrappedKey, AppError> {
        let nonce = Nonce::<Aes256Gcm>::generate();

        // encrypt_in_place overwrites the plaintext copy
        let mut buf = key.0.expose_secret().to_vec();
        self.cipher
            .encrypt_in_place(&nonce, &wrap_aad(stream_id, election), &mut buf)
            .map_err(|e| AppError::ServerError(std::io::Error::other(e.to_string())))?;

        let mut out = Vec::with_capacity(NONCE_LEN + buf.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&buf);
        Ok(WrappedKey(out))
    }

    /// Decrypt a key wrapped by [`wrap_key`](Self::wrap_key) for the same
    /// `(stream_id, election)`.
    pub fn unwrap_key(
        &self,
        wrapped: &WrappedKey,
        stream_id: StreamId,
        election: ElectionConfig,
    ) -> Result<StreamKey, AppError> {
        let wrapped = wrapped.as_bytes();
        if wrapped.len() < NONCE_LEN {
            return Err(AppError::EventDecodeError(
                "wrapped stream key too short for nonce".to_string(),
            ));
        }

        let nonce = Nonce::<Aes256Gcm>::try_from(&wrapped[..NONCE_LEN])
            .expect("slice is exactly NONCE_LEN bytes");

        let mut buf = Zeroizing::new(wrapped[NONCE_LEN..].to_vec());
        self.cipher
            .decrypt_in_place(&nonce, &wrap_aad(stream_id, election), &mut *buf)
            .map_err(|e| AppError::EventDecodeError(format!("stream key unwrap failed: {e}")))?;

        if buf.len() != KEY_LEN {
            return Err(AppError::EventDecodeError(format!(
                "unwrapped stream key is {} bytes, expected {KEY_LEN}",
                buf.len()
            )));
        }

        Ok(StreamKey(SecretBox::init_with_mut(
            |slot: &mut [u8; KEY_LEN]| {
                slot.copy_from_slice(&buf);
            },
        )))
    }
}

impl std::fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MasterKey([REDACTED])")
    }
}

/// Associated data binding a wrapped key to its `(stream_id, election)`.
fn wrap_aad(stream_id: StreamId, election: ElectionConfig) -> Vec<u8> {
    let election_id = election.stable_id();
    let mut aad = Vec::with_capacity(WRAP_AAD_PREFIX.len() + 16 + 1 + election_id.len());
    aad.extend_from_slice(WRAP_AAD_PREFIX);
    aad.extend_from_slice(stream_id.uuid().as_bytes());
    aad.push(b':');
    aad.extend_from_slice(election_id.as_bytes());
    aad
}

/// A [`StreamKey`] encrypted by the [`MasterKey`] as `nonce || ciphertext || tag`:
/// the only form in which stream keys are persisted. Ciphertext, not a secret,
/// but a distinct type so wrapped-key blobs cannot be confused with other byte
/// buffers (unwrapped keys are [`StreamKey`]s and never leave this module).
#[derive(Clone, PartialEq, Eq)]
pub struct WrappedKey(Vec<u8>);

impl WrappedKey {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Only for reading back a persisted wrapped key; wrap fresh keys with
/// [`MasterKey::wrap_key`].
impl From<Vec<u8>> for WrappedKey {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl std::fmt::Debug for WrappedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WrappedKey({} bytes)", self.0.len())
    }
}

/// Random 256-bit data-encryption key for a single stream. Zeroed on drop;
/// persisted only in wrapped form.
pub struct StreamKey(SecretBox<[u8; KEY_LEN]>);

impl StreamKey {
    /// Generate a fresh key from the system CSPRNG.
    pub fn generate() -> Self {
        let mut bytes = <[u8; KEY_LEN]>::generate();
        let key = Self(SecretBox::init_with_mut(|slot: &mut [u8; KEY_LEN]| {
            slot.copy_from_slice(&bytes);
        }));
        bytes.zeroize();
        key
    }

    /// Build the [`EventCipher`] for this stream's payloads.
    pub fn cipher(&self) -> EventCipher {
        let cipher = Aes256Gcm::new_from_slice(self.0.expose_secret())
            .expect("32 bytes is a valid AES-256 key length");
        EventCipher { cipher }
    }
}

impl std::fmt::Debug for StreamKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StreamKey([REDACTED])")
    }
}

/// AES-256-GCM cipher for a single stream's event payloads.
///
/// Encrypts and decrypts event payloads serialized with postcard.
/// Each ciphertext is prefixed with a random 12-byte nonce.
///
/// Only persisting backends hold one (see `StoreBackend`); an in-memory store
/// has no cipher because it never writes events out.
#[derive(Clone)]
pub struct EventCipher {
    cipher: Aes256Gcm,
}

impl EventCipher {
    /// Serialize `event` with postcard and encrypt, binding `aad` into the
    /// authentication tag (see [`crate::store::event_aad`]).
    ///
    /// Returns `nonce || ciphertext || tag`.
    pub fn encrypt<E: Serialize>(&self, event: &E, aad: &[u8]) -> Result<Vec<u8>, AppError> {
        let nonce = Nonce::<Aes256Gcm>::generate();

        let mut ciphertext = postcard::to_allocvec(event).map_err(|e| {
            AppError::ServerError(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;

        self.cipher
            .encrypt_in_place(&nonce, aad, &mut ciphertext)
            .map_err(|e| AppError::ServerError(std::io::Error::other(e.to_string())))?;

        let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    /// Decrypt and deserialize from an already-owned buffer, avoiding a copy.
    ///
    /// Expects `nonce (12 bytes) || ciphertext || tag` and the same `aad` used
    /// when encrypting.
    pub fn decrypt<E: DeserializeOwned>(
        &self,
        mut data: Vec<u8>,
        aad: &[u8],
    ) -> Result<E, EventDecryptError> {
        if data.len() < NONCE_LEN {
            return Err(EventDecryptError::Unreadable(
                "ciphertext too short for nonce".to_string(),
            ));
        }

        let nonce = Nonce::<Aes256Gcm>::try_from(&data[..NONCE_LEN])
            .expect("slice is exactly NONCE_LEN bytes");

        // Remove the nonce prefix so the Vec contains only ciphertext + tag,
        // then decrypt in place — this truncates the tag, leaving plaintext.
        let _ = data.drain(..NONCE_LEN);
        self.cipher
            .decrypt_in_place(&nonce, aad, &mut data)
            .map_err(|e| EventDecryptError::Unreadable(format!("AES-GCM decrypt failed: {e}")))?;

        let event = postcard::from_bytes(&data)
            .map_err(|e| EventDecryptError::IncompatiblePayload(e.to_string()));
        // wipe the plaintext copy
        data.zeroize();
        event
    }
}

/// Why a stored event payload could not be turned back into an event.
///
/// Kept apart because [`crate::store::apply_encrypted_events`] treats them
/// differently: untrustworthy bytes are an integrity failure, an authentic
/// payload this build cannot decode is schema drift.
#[derive(Debug)]
pub enum EventDecryptError {
    /// Authentication or framing failed; the plaintext was never recovered.
    Unreadable(String),
    /// Plaintext recovered, but postcard could not decode it into the event type.
    IncompatiblePayload(String),
}

impl std::fmt::Display for EventDecryptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable(err) => write!(f, "{err}"),
            Self::IncompatiblePayload(err) => write!(f, "payload deserialize failed: {err}"),
        }
    }
}

impl From<EventDecryptError> for AppError {
    fn from(err: EventDecryptError) -> Self {
        AppError::EventDecodeError(err.to_string())
    }
}

impl std::fmt::Debug for EventCipher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EventCipher([REDACTED])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_master() -> MasterKey {
        MasterKey::new(&SecretString::from("test-encryption-secret"))
    }

    const TEST_ELECTION: ElectionConfig = ElectionConfig::EK27;

    const NO_AAD: &[u8] = b"";

    #[test]
    fn wrap_unwrap_round_trip() {
        let master = test_master();
        let stream_id = StreamId::new();
        let key = StreamKey::generate();

        let wrapped = master.wrap_key(&key, stream_id, TEST_ELECTION).unwrap();
        let unwrapped = master
            .unwrap_key(&wrapped, stream_id, TEST_ELECTION)
            .unwrap();

        assert_eq!(key.0.expose_secret(), unwrapped.0.expose_secret());
    }

    #[test]
    fn generated_keys_are_distinct() {
        let a = StreamKey::generate();
        let b = StreamKey::generate();

        assert_ne!(a.0.expose_secret(), b.0.expose_secret());
    }

    #[test]
    fn wrong_master_key_cannot_unwrap() {
        let stream_id = StreamId::new();
        let key = StreamKey::generate();
        let wrapped = test_master()
            .wrap_key(&key, stream_id, TEST_ELECTION)
            .unwrap();

        let other = MasterKey::new(&SecretString::from("different-secret"));
        let result = other.unwrap_key(&wrapped, stream_id, TEST_ELECTION);

        assert!(matches!(result, Err(AppError::EventDecodeError(_))));
    }

    #[test]
    fn wrapped_key_is_bound_to_stream_and_election() {
        let master = test_master();
        let stream_id = StreamId::new();
        let key = StreamKey::generate();
        let wrapped = master.wrap_key(&key, stream_id, TEST_ELECTION).unwrap();

        // A different stream cannot unwrap it.
        assert!(
            master
                .unwrap_key(&wrapped, StreamId::new(), TEST_ELECTION)
                .is_err()
        );
        // A different election cannot unwrap it.
        assert!(
            master
                .unwrap_key(
                    &wrapped,
                    stream_id,
                    ElectionConfig::PS27(crate::Province::Groningen)
                )
                .is_err()
        );
    }

    #[test]
    fn tampered_wrapped_key_fails() {
        let master = test_master();
        let stream_id = StreamId::new();
        let key = StreamKey::generate();

        let mut bytes = master
            .wrap_key(&key, stream_id, TEST_ELECTION)
            .unwrap()
            .as_bytes()
            .to_vec();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        let wrapped = WrappedKey::from(bytes);

        assert!(
            master
                .unwrap_key(&wrapped, stream_id, TEST_ELECTION)
                .is_err()
        );
    }

    #[test]
    fn too_short_wrapped_key_fails() {
        let master = test_master();
        let result = master.unwrap_key(
            &WrappedKey::from(vec![0u8; 5]),
            StreamId::new(),
            TEST_ELECTION,
        );
        assert!(matches!(result, Err(AppError::EventDecodeError(_))));
    }

    #[test]
    fn debug_output_is_redacted() {
        assert_eq!(format!("{:?}", test_master()), "MasterKey([REDACTED])");
        assert_eq!(
            format!("{:?}", StreamKey::generate()),
            "StreamKey([REDACTED])"
        );
        assert_eq!(
            format!("{:?}", StreamKey::generate().cipher()),
            "EventCipher([REDACTED])"
        );
    }

    #[test]
    fn round_trip_encrypt_decrypt() {
        let cipher = StreamKey::generate().cipher();

        let original: Vec<u8> = vec![1, 2, 3, 4, 5];
        let encrypted = cipher.encrypt(&original, NO_AAD).unwrap();
        let decrypted: Vec<u8> = cipher.decrypt(encrypted, NO_AAD).unwrap();

        assert_eq!(original, decrypted);
    }

    #[test]
    fn different_keys_produce_different_ciphertexts() {
        let cipher_a = StreamKey::generate().cipher();
        let cipher_b = StreamKey::generate().cipher();

        let data = "same payload";
        let enc_a = cipher_a.encrypt(&data, NO_AAD).unwrap();
        let enc_b = cipher_b.encrypt(&data, NO_AAD).unwrap();

        // Different keys → different ciphertext (with overwhelming probability)
        assert_ne!(enc_a, enc_b);
    }

    #[test]
    fn wrong_key_fails_decryption() {
        let cipher_a = StreamKey::generate().cipher();
        let cipher_b = StreamKey::generate().cipher();

        let encrypted = cipher_a.encrypt(&42u32, NO_AAD).unwrap();
        let result = cipher_b.decrypt::<u32>(encrypted, NO_AAD);

        assert!(result.is_err());
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let cipher = StreamKey::generate().cipher();

        let mut encrypted = cipher.encrypt(&"hello", NO_AAD).unwrap();
        // Flip a bit in the ciphertext (after the nonce)
        let last = encrypted.len() - 1;
        encrypted[last] ^= 0x01;

        let result = cipher.decrypt::<String>(encrypted, NO_AAD);
        assert!(result.is_err());
    }

    #[test]
    fn wrong_aad_fails_decryption() {
        let cipher = StreamKey::generate().cipher();

        let encrypted = cipher.encrypt(&"hello", b"aad-a").unwrap();

        assert!(
            cipher
                .decrypt::<String>(encrypted.clone(), b"aad-b")
                .is_err()
        );
        assert!(cipher.decrypt::<String>(encrypted.clone(), NO_AAD).is_err());
        assert_eq!(
            cipher.decrypt::<String>(encrypted, b"aad-a").unwrap(),
            "hello"
        );
    }

    #[test]
    fn decrypt_too_short_data_fails() {
        let cipher = StreamKey::generate().cipher();

        let result = cipher.decrypt::<u32>(vec![0u8; 5], NO_AAD);
        assert!(result.is_err());
    }

    #[test]
    fn same_plaintext_produces_different_ciphertexts() {
        let cipher = StreamKey::generate().cipher();

        let data = "repeated";
        let enc1 = cipher.encrypt(&data, NO_AAD).unwrap();
        let enc2 = cipher.encrypt(&data, NO_AAD).unwrap();

        // Random nonces → different ciphertexts
        assert_ne!(enc1, enc2);

        // Both decrypt to the same value
        let dec1: String = cipher.decrypt(enc1, NO_AAD).unwrap();
        let dec2: String = cipher.decrypt(enc2, NO_AAD).unwrap();
        assert_eq!(dec1, dec2);
        assert_eq!(dec1, "repeated");
    }
}
