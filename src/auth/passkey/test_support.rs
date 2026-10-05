//! A software authenticator for the passkey tests: answers registration and
//! authentication challenges the way a browser with a platform authenticator
//! would, producing the exact JSON `credential.toJSON()` emits. Lets the
//! handler tests run whole ceremonies without a browser.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use p256::ecdsa::{Signature, SigningKey, signature::Signer};
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use webauthn_rs::prelude::{
    CreationChallengeResponse, Passkey, PublicKeyCredential, RegisterPublicKeyCredential,
    RequestChallengeResponse, Url, Uuid, Webauthn, WebauthnBuilder,
};

use super::{CsbPasskeyStore, PasskeyAccount, PasskeyAccountId, PasskeyLogin};
use crate::{CsbPasskeyConfig, CsbUser};

pub(crate) const TEST_ORIGIN: &str = "http://localhost:3000";
pub(crate) const TEST_RP_ID: &str = "localhost";

pub(crate) fn test_passkey_config() -> CsbPasskeyConfig {
    CsbPasskeyConfig {
        rp_id: TEST_RP_ID.to_string(),
        origin: Url::parse(TEST_ORIGIN).expect("valid origin"),
    }
}

pub(crate) fn test_webauthn() -> Webauthn {
    WebauthnBuilder::new(TEST_RP_ID, &Url::parse(TEST_ORIGIN).unwrap())
        .unwrap()
        .rp_name("test")
        .build()
        .unwrap()
}

pub(crate) fn test_login() -> PasskeyLogin {
    PasskeyLogin::new(
        &test_passkey_config(),
        CsbPasskeyStore::default(),
        &SecretString::from("test-encryption-secret-123"),
    )
    .expect("test passkey login")
}

pub(crate) fn test_account(name: &str) -> PasskeyAccount {
    PasskeyAccount::new(
        PasskeyAccountId::new(),
        name.parse().expect("valid name"),
        CsbUser::new_test(),
    )
}

/// A registered passkey, obtained by running a registration ceremony with a
/// fresh software authenticator seeded from `seed`.
pub(crate) fn test_passkey(seed: u8) -> Passkey {
    let webauthn = test_webauthn();
    let mut authenticator = SoftAuthenticator::new(seed);
    let (ccr, state) = webauthn
        .start_passkey_registration(Uuid::new_v4(), "test", "test", None)
        .unwrap();
    let credential = authenticator.register(&ccr);
    webauthn
        .finish_passkey_registration(&credential, &state)
        .expect("registration verifies")
}

/// A single-credential software authenticator: one P-256 key, one
/// credential id, a counter. User verification is always reported.
pub(crate) struct SoftAuthenticator {
    key: SigningKey,
    credential_id: Vec<u8>,
    counter: u32,
}

impl SoftAuthenticator {
    pub(crate) fn new(seed: u8) -> Self {
        let mut secret = [seed; 32];
        secret[0] = secret[0].wrapping_add(1); // never the all-zero scalar
        Self {
            key: SigningKey::from_bytes(&secret.into()).expect("valid scalar"),
            credential_id: (0..32).map(|i| i ^ seed).collect(),
            counter: 0,
        }
    }

    pub(crate) fn credential_id(&self) -> &[u8] {
        &self.credential_id
    }

    fn rp_id_hash(rp_id: &str) -> Vec<u8> {
        Sha256::digest(rp_id.as_bytes()).to_vec()
    }

    fn client_data_json(kind: &str, challenge: &[u8]) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "type": kind,
            "challenge": URL_SAFE_NO_PAD.encode(challenge),
            "origin": TEST_ORIGIN,
            "crossOrigin": false,
        }))
        .unwrap()
    }

    /// The COSE EC2 key of the signing key as CBOR.
    fn cose_public_key(&self) -> Vec<u8> {
        let point = self.key.verifying_key().to_encoded_point(false);
        let map = ciborium::Value::Map(vec![
            (
                ciborium::Value::Integer(1.into()),
                ciborium::Value::Integer(2.into()),
            ),
            (
                ciborium::Value::Integer(3.into()),
                ciborium::Value::Integer((-7).into()),
            ),
            (
                ciborium::Value::Integer((-1).into()),
                ciborium::Value::Integer(1.into()),
            ),
            (
                ciborium::Value::Integer((-2).into()),
                ciborium::Value::Bytes(point.x().unwrap().to_vec()),
            ),
            (
                ciborium::Value::Integer((-3).into()),
                ciborium::Value::Bytes(point.y().unwrap().to_vec()),
            ),
        ]);
        let mut out = Vec::new();
        ciborium::into_writer(&map, &mut out).unwrap();
        out
    }

    /// Answers a registration challenge with a `none` attestation.
    pub(crate) fn register(
        &mut self,
        ccr: &CreationChallengeResponse,
    ) -> RegisterPublicKeyCredential {
        let options = &ccr.public_key;
        self.counter += 1;

        let mut auth_data = Self::rp_id_hash(&options.rp.id);
        auth_data.push(0x01 | 0x04 | 0x40); // user present, user verified, attested data
        auth_data.extend_from_slice(&self.counter.to_be_bytes());
        auth_data.extend_from_slice(&[0u8; 16]); // AAGUID
        auth_data.extend_from_slice(&(self.credential_id.len() as u16).to_be_bytes());
        auth_data.extend_from_slice(&self.credential_id);
        auth_data.extend_from_slice(&self.cose_public_key());

        let attestation = ciborium::Value::Map(vec![
            (
                ciborium::Value::Text("fmt".to_string()),
                ciborium::Value::Text("none".to_string()),
            ),
            (
                ciborium::Value::Text("attStmt".to_string()),
                ciborium::Value::Map(vec![]),
            ),
            (
                ciborium::Value::Text("authData".to_string()),
                ciborium::Value::Bytes(auth_data),
            ),
        ]);
        let mut attestation_object = Vec::new();
        ciborium::into_writer(&attestation, &mut attestation_object).unwrap();

        let json = serde_json::json!({
            "id": URL_SAFE_NO_PAD.encode(&self.credential_id),
            "rawId": URL_SAFE_NO_PAD.encode(&self.credential_id),
            "type": "public-key",
            "authenticatorAttachment": "platform",
            "response": {
                "attestationObject": URL_SAFE_NO_PAD.encode(attestation_object),
                "clientDataJSON": URL_SAFE_NO_PAD.encode(Self::client_data_json(
                    "webauthn.create",
                    &options.challenge,
                )),
                "transports": ["internal"],
            },
            "clientExtensionResults": {},
        });
        serde_json::from_value(json).expect("browser JSON deserializes")
    }

    /// Answers an authentication challenge; `None` when none of the allowed
    /// credentials is this authenticator's, as a browser would refuse.
    pub(crate) fn authenticate(
        &mut self,
        rcr: &RequestChallengeResponse,
    ) -> Option<PublicKeyCredential> {
        let options = &rcr.public_key;
        if !options
            .allow_credentials
            .iter()
            .any(|allowed| allowed.id == self.credential_id)
        {
            return None;
        }
        self.counter += 1;

        let mut auth_data = Self::rp_id_hash(&options.rp_id);
        auth_data.push(0x01 | 0x04); // user present, user verified
        auth_data.extend_from_slice(&self.counter.to_be_bytes());

        let client_data = Self::client_data_json("webauthn.get", &options.challenge);
        let mut signed = auth_data.clone();
        signed.extend_from_slice(&Sha256::digest(&client_data));
        let signature: Signature = self.key.sign(&signed);

        let json = serde_json::json!({
            "id": URL_SAFE_NO_PAD.encode(&self.credential_id),
            "rawId": URL_SAFE_NO_PAD.encode(&self.credential_id),
            "type": "public-key",
            "authenticatorAttachment": "platform",
            "response": {
                "authenticatorData": URL_SAFE_NO_PAD.encode(auth_data),
                "clientDataJSON": URL_SAFE_NO_PAD.encode(client_data),
                "signature": URL_SAFE_NO_PAD.encode(signature.to_der().as_bytes()),
                "userHandle": null,
            },
            "clientExtensionResults": {},
        });
        Some(serde_json::from_value(json).expect("browser JSON deserializes"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The software authenticator must pass webauthn-rs itself end to end,
    /// or every test built on it proves nothing.
    #[test]
    fn soft_authenticator_registers_and_authenticates() {
        let webauthn = test_webauthn();
        let mut authenticator = SoftAuthenticator::new(7);

        let (ccr, state) = webauthn
            .start_passkey_registration(Uuid::new_v4(), "jan", "Jan", None)
            .unwrap();
        let credential = authenticator.register(&ccr);
        let mut passkey = webauthn
            .finish_passkey_registration(&credential, &state)
            .expect("registration verifies");
        assert_eq!(passkey.cred_id().as_slice(), authenticator.credential_id());

        let (rcr, state) = webauthn
            .start_passkey_authentication(std::slice::from_ref(&passkey))
            .unwrap();
        let assertion = authenticator
            .authenticate(&rcr)
            .expect("credential allowed");
        let result = webauthn
            .finish_passkey_authentication(&assertion, &state)
            .expect("assertion verifies");
        assert_eq!(result.cred_id(), passkey.cred_id());
        assert!(result.needs_update(), "counter moved");
        assert_eq!(passkey.update_credential(&result), Some(true));

        // Another authenticator cannot answer for this credential.
        let mut other = SoftAuthenticator::new(8);
        assert!(other.authenticate(&rcr).is_none());
    }
}
