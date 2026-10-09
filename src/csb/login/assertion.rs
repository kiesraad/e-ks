//! Verification of a WebAuthn assertion for the CSB login, following the
//! relying-party steps of the specification
//! (<https://www.w3.org/TR/webauthn-3/#sctn-verifying-assertion>) for the
//! one shape in use: ES256 keys, non-resident credentials, no extensions.
//! That keeps the whole check short enough to read in one sitting. Only the
//! data the authenticator signed is trusted; the rest of what the browser
//! sends is ignored.

use std::fmt::Display;

use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use p256::ecdsa::{Signature, VerifyingKey, signature::Verifier};
use serde::{Deserialize, Deserializer};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use url::{Origin, Url};

/// `authenticatorData` is the relying-party id hash (32), flags (1) and the
/// signature counter (4); anything after that is not used here.
const AUTHENTICATOR_DATA_MIN_LEN: usize = 37;
const FLAG_USER_PRESENT: u8 = 0x01;
const FLAG_USER_VERIFIED: u8 = 0x04;

/// The assertion as the browser script posts it: a `PublicKeyCredential`
/// with an `AuthenticatorAssertionResponse`, binary fields base64url.
#[derive(Debug, Deserialize)]
pub(super) struct Assertion {
    #[serde(rename = "rawId", deserialize_with = "base64url")]
    pub(super) raw_id: Vec<u8>,
    response: AssertionResponse,
}

#[derive(Debug, Deserialize)]
struct AssertionResponse {
    #[serde(rename = "authenticatorData", deserialize_with = "base64url")]
    authenticator_data: Vec<u8>,
    #[serde(rename = "clientDataJSON", deserialize_with = "base64url")]
    client_data_json: Vec<u8>,
    #[serde(deserialize_with = "base64url")]
    signature: Vec<u8>,
}

/// The signed client data, as far as it is checked.
#[derive(Deserialize)]
struct ClientData {
    #[serde(rename = "type")]
    ceremony: String,
    challenge: String,
    origin: String,
}

/// What the assertion must prove.
pub(super) struct Expected<'a> {
    pub(super) challenge: &'a [u8],
    pub(super) origin: &'a Origin,
    pub(super) rp_id_hash: &'a [u8; 32],
    pub(super) public_key: &'a VerifyingKey,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum AssertionError {
    ClientDataMalformed,
    WrongCeremony,
    ChallengeMismatch,
    OriginMismatch,
    AuthenticatorDataMalformed,
    RpIdMismatch,
    UserNotPresent,
    UserNotVerified,
    SignatureInvalid,
}

impl Display for AssertionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ClientDataMalformed => "client data is malformed",
            Self::WrongCeremony => "client data is not from a webauthn.get ceremony",
            Self::ChallengeMismatch => "challenge does not match the one issued",
            Self::OriginMismatch => "origin does not match the configured origin",
            Self::AuthenticatorDataMalformed => "authenticator data is too short",
            Self::RpIdMismatch => "relying-party id hash does not match",
            Self::UserNotPresent => "user presence flag is not set",
            Self::UserNotVerified => "user verification flag is not set",
            Self::SignatureInvalid => "signature does not verify under the registered key",
        })
    }
}

impl Assertion {
    /// Checks the assertion against `expected`. Every check is on data under
    /// the signature, and the signature is checked last so no earlier check
    /// can be skipped by a forged response.
    pub(super) fn verify(&self, expected: &Expected<'_>) -> Result<(), AssertionError> {
        let client_data: ClientData = serde_json::from_slice(&self.response.client_data_json)
            .map_err(|_| AssertionError::ClientDataMalformed)?;
        if client_data.ceremony != "webauthn.get" {
            return Err(AssertionError::WrongCeremony);
        }
        let challenge = BASE64_URL_SAFE_NO_PAD
            .decode(&client_data.challenge)
            .map_err(|_| AssertionError::ClientDataMalformed)?;
        if !bool::from(challenge.ct_eq(expected.challenge)) {
            return Err(AssertionError::ChallengeMismatch);
        }
        let origin = Url::parse(&client_data.origin)
            .map(|url| url.origin())
            .map_err(|_| AssertionError::OriginMismatch)?;
        if &origin != expected.origin {
            return Err(AssertionError::OriginMismatch);
        }

        let authenticator_data = &self.response.authenticator_data;
        if authenticator_data.len() < AUTHENTICATOR_DATA_MIN_LEN {
            return Err(AssertionError::AuthenticatorDataMalformed);
        }
        if !bool::from(authenticator_data[..32].ct_eq(expected.rp_id_hash)) {
            return Err(AssertionError::RpIdMismatch);
        }
        let flags = authenticator_data[32];
        if flags & FLAG_USER_PRESENT == 0 {
            return Err(AssertionError::UserNotPresent);
        }
        if flags & FLAG_USER_VERIFIED == 0 {
            return Err(AssertionError::UserNotVerified);
        }

        // The authenticator signs authenticatorData || SHA-256(clientDataJSON).
        let mut signed = authenticator_data.clone();
        signed.extend_from_slice(&Sha256::digest(&self.response.client_data_json));
        let signature = Signature::from_der(&self.response.signature)
            .map_err(|_| AssertionError::SignatureInvalid)?;
        expected
            .public_key
            .verify(&signed, &signature)
            .map_err(|_| AssertionError::SignatureInvalid)
    }
}

fn base64url<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
    let encoded = String::deserialize(deserializer)?;
    BASE64_URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(serde::de::Error::custom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csb::login::test_support::{
        TEST_ORIGIN, TestAssertion, test_credential_id, test_signing_key,
    };

    const CHALLENGE: [u8; 32] = [7; 32];

    fn verify(assertion: &TestAssertion) -> Result<(), AssertionError> {
        let parsed: Assertion = serde_json::from_str(&assertion.json()).expect("assertion json");
        assert_eq!(parsed.raw_id, assertion.credential_id);
        let origin = Url::parse(TEST_ORIGIN).expect("url").origin();
        let rp_id_hash: [u8; 32] = Sha256::digest(b"csb.example.nl").into();
        parsed.verify(&Expected {
            challenge: &CHALLENGE,
            origin: &origin,
            rp_id_hash: &rp_id_hash,
            public_key: test_signing_key().verifying_key(),
        })
    }

    #[test]
    fn accepts_a_valid_assertion() {
        assert_eq!(verify(&TestAssertion::new(&CHALLENGE)), Ok(()));
    }

    type Tamper = fn(&mut TestAssertion);

    #[test]
    fn rejects_each_tampered_field() {
        let cases: [(&str, Tamper, AssertionError); 8] = [
            (
                "ceremony",
                |a| a.ceremony = "webauthn.create",
                AssertionError::WrongCeremony,
            ),
            (
                "challenge",
                |a| a.challenge = vec![8; 32],
                AssertionError::ChallengeMismatch,
            ),
            (
                "challenge length",
                |a| a.challenge = CHALLENGE[..31].to_vec(),
                AssertionError::ChallengeMismatch,
            ),
            (
                "origin",
                |a| a.origin = "https://evil.example".to_string(),
                AssertionError::OriginMismatch,
            ),
            (
                "origin scheme",
                |a| a.origin = "http://csb.example.nl".to_string(),
                AssertionError::OriginMismatch,
            ),
            (
                "rp id",
                |a| a.rp_id = "example.nl".to_string(),
                AssertionError::RpIdMismatch,
            ),
            (
                "user presence",
                |a| a.user_present = false,
                AssertionError::UserNotPresent,
            ),
            (
                "user verification",
                |a| a.user_verified = false,
                AssertionError::UserNotVerified,
            ),
        ];
        for (what, tamper, expected) in cases {
            let mut assertion = TestAssertion::new(&CHALLENGE);
            tamper(&mut assertion);
            assert_eq!(verify(&assertion), Err(expected), "{what}");
        }
    }

    #[test]
    fn rejects_a_signature_by_another_key() {
        let mut assertion = TestAssertion::new(&CHALLENGE);
        assertion.signing_key =
            p256::ecdsa::SigningKey::from_bytes(&[0x43; 32].into()).expect("valid scalar");
        assert_eq!(verify(&assertion), Err(AssertionError::SignatureInvalid));
    }

    #[test]
    fn rejects_malformed_responses() {
        let valid: serde_json::Value =
            serde_json::from_str(&TestAssertion::new(&CHALLENGE).json()).expect("json");

        let mut no_sig = valid.clone();
        no_sig["response"]["signature"] = "AAAA".into();
        let parsed: Assertion = serde_json::from_value(no_sig).expect("parses");
        let origin = Url::parse(TEST_ORIGIN).expect("url").origin();
        let rp_id_hash: [u8; 32] = Sha256::digest(b"csb.example.nl").into();
        let signing_key = test_signing_key();
        let expected = Expected {
            challenge: &CHALLENGE,
            origin: &origin,
            rp_id_hash: &rp_id_hash,
            public_key: signing_key.verifying_key(),
        };
        assert_eq!(
            parsed.verify(&expected),
            Err(AssertionError::SignatureInvalid)
        );

        let mut short = valid.clone();
        short["response"]["authenticatorData"] = "AAAA".into();
        let parsed: Assertion = serde_json::from_value(short).expect("parses");
        assert_eq!(
            parsed.verify(&expected),
            Err(AssertionError::AuthenticatorDataMalformed)
        );

        let mut garbage = valid.clone();
        garbage["response"]["clientDataJSON"] = "AAAA".into();
        let parsed: Assertion = serde_json::from_value(garbage).expect("parses");
        assert_eq!(
            parsed.verify(&expected),
            Err(AssertionError::ClientDataMalformed)
        );

        let mut not_base64 = valid;
        not_base64["rawId"] = "not base64!".into();
        assert!(serde_json::from_value::<Assertion>(not_base64).is_err());
        assert!(serde_json::from_str::<Assertion>("{}").is_err());

        let _ = test_credential_id();
    }
}
