use std::sync::Arc;

use hkdf::Hkdf;
use secrecy::{ExposeSecret, SecretString};
use sha2::Sha256;

use crate::StreamId;

const HKDF_SALT: &[u8] = b"e-KS BSN identifier derivation v1";
const STREAM_INFO_PREFIX: &[u8] = b"stream-id:";

/// Derives a deterministic per-user `StreamId` from an identification code using HKDF-SHA256.
///
/// The deriver holds a pre-extracted PRK (pseudo-random key) computed once at
/// startup from the master secret. Each call to `derive_stream_id`
/// runs only the cheaper HKDF-Expand step.
///
/// One `StreamId` per user covers all of the user's elections; the election
/// is tracked alongside the stream as a second key axis in the store.
#[derive(Clone)]
pub struct IdDeriver {
    hk: Arc<Hkdf<Sha256>>,
}

impl IdDeriver {
    pub fn new(secret: &SecretString) -> Self {
        let hk = Hkdf::<Sha256>::new(Some(HKDF_SALT), secret.expose_secret().as_bytes());
        Self { hk: Arc::new(hk) }
    }

    pub fn derive_stream_id(&self, code: &SecretString) -> StreamId {
        let info: Vec<u8> = STREAM_INFO_PREFIX
            .iter()
            .chain(code.expose_secret().as_bytes())
            .copied()
            .collect();

        let mut okm = [0u8; 16];
        self.hk
            .expand(&info, &mut okm)
            .expect("16 bytes is within HKDF-SHA256 output limit");

        uuid::Builder::from_custom_bytes(okm).into_uuid().into()
    }
}

impl std::fmt::Debug for IdDeriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("IdDeriver([REDACTED])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_secret(value: &str) -> SecretString {
        SecretString::from(value)
    }

    #[test]
    fn derive_is_deterministic() {
        let deriver = IdDeriver::new(&test_secret("test-secret"));
        let code = test_secret("999999990");

        let id1 = deriver.derive_stream_id(&code);
        let id2 = deriver.derive_stream_id(&code);

        assert_eq!(id1, id2);
    }

    #[test]
    fn different_codes_produce_different_ids() {
        let deriver = IdDeriver::new(&test_secret("test-secret"));
        let code_a = test_secret("999999990");
        let code_b = test_secret("123456782");

        let id_a = deriver.derive_stream_id(&code_a);
        let id_b = deriver.derive_stream_id(&code_b);

        assert_ne!(id_a, id_b);
    }

    #[test]
    fn different_secrets_produce_different_ids() {
        let deriver_a = IdDeriver::new(&test_secret("secret-one"));
        let deriver_b = IdDeriver::new(&test_secret("secret-two"));
        let code = test_secret("999999990");

        let id_a = deriver_a.derive_stream_id(&code);
        let id_b = deriver_b.derive_stream_id(&code);

        assert_ne!(id_a, id_b);
    }

    #[test]
    fn produces_uuid_v8() {
        let deriver = IdDeriver::new(&test_secret("test-secret"));
        let code = test_secret("999999990");

        let id = deriver.derive_stream_id(&code);
        let uuid = id.uuid();

        assert_eq!(uuid.get_version_num(), 8);
        assert_eq!(uuid.get_variant(), uuid::Variant::RFC4122);
    }
}
