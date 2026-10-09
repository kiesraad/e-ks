//! Verification of a WebAuthn assertion for the CSB login: the relying-party
//! steps of §7.2 of the specification
//! (<https://w3c.github.io/webauthn/#sctn-verifying-assertion>), for the one
//! shape in use: ES256 keys, non-resident credentials, no extensions, no
//! framing. Step numbers in the comments are those of that section. Only the
//! data the authenticator signed is trusted; the rest of what the browser
//! sends is checked for shape and otherwise ignored.
//!
//! Steps 1 and 2 (the options and the `get()` call) are the login page and
//! the browser script; steps 5, 6, 22 and 24 (the credential record and the
//! signature counter) are in the parent module. Step 19 (backup-state
//! policy) is not applied: the configuration records no backup eligibility.

use std::fmt::Display;

use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use p256::ecdsa::{Signature, VerifyingKey, signature::Verifier};
use serde::{Deserialize, Deserializer};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use url::{Origin, Url};

use crate::CredentialId;

/// `authenticatorData` without extensions or attested credential data
/// (§6.1): the relying-party id hash (32), flags (1) and the signature
/// counter (4).
const AUTHENTICATOR_DATA_LEN: usize = 37;
const FLAG_USER_PRESENT: u8 = 0x01;
const FLAG_USER_VERIFIED: u8 = 0x04;
const FLAG_BACKUP_ELIGIBLE: u8 = 0x08;
const FLAG_BACKUP_STATE: u8 = 0x10;
const FLAG_ATTESTED_CREDENTIAL_DATA: u8 = 0x40;
const FLAG_EXTENSION_DATA: u8 = 0x80;

/// The assertion as the browser script posts it: a `PublicKeyCredential`
/// with an `AuthenticatorAssertionResponse`, binary fields base64url.
#[derive(Debug, Deserialize)]
pub(super) struct Assertion {
    #[serde(rename = "type")]
    credential_type: String,
    #[serde(rename = "rawId")]
    pub(super) raw_id: CredentialId,
    response: AssertionResponse,
    /// Step 4: the client extension outputs.
    #[serde(rename = "clientExtensionResults", default)]
    client_extension_results: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct AssertionResponse {
    #[serde(rename = "authenticatorData", deserialize_with = "base64url")]
    authenticator_data: Vec<u8>,
    #[serde(rename = "clientDataJSON", deserialize_with = "base64url")]
    client_data_json: Vec<u8>,
    #[serde(deserialize_with = "base64url")]
    signature: Vec<u8>,
    #[serde(
        rename = "userHandle",
        default,
        deserialize_with = "optional_base64url"
    )]
    user_handle: Option<Vec<u8>>,
}

/// The signed client data (§5.8.1), as far as it is checked.
#[derive(Deserialize)]
struct ClientData {
    #[serde(rename = "type")]
    ceremony: String,
    challenge: String,
    origin: String,
    #[serde(rename = "crossOrigin", default)]
    cross_origin: bool,
    #[serde(rename = "topOrigin")]
    top_origin: Option<String>,
}

/// What the assertion must prove: the credential record (§4, the configured
/// user) and the ceremony it answers.
pub(super) struct Expected<'a> {
    /// Base64url, as issued.
    pub(super) challenge: &'a str,
    pub(super) origin: &'a Origin,
    pub(super) rp_id_hash: &'a [u8; 32],
    pub(super) user_handle: &'a [u8],
    pub(super) public_key: &'a VerifyingKey,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AssertionError {
    NotPublicKeyCredential,
    UserHandleMismatch,
    ClientDataMalformed,
    WrongCeremony,
    ChallengeMismatch,
    OriginMismatch,
    CrossOrigin,
    AuthenticatorDataMalformed,
    RpIdMismatch,
    UserNotPresent,
    UserNotVerified,
    BackupStateInconsistent,
    UnrequestedOutputs,
    SignatureInvalid,
    SignCountNotAdvanced,
}

impl Display for AssertionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotPublicKeyCredential => "credential is not of type public-key",
            Self::UserHandleMismatch => "user handle is not the registered user's",
            Self::ClientDataMalformed => "client data is malformed",
            Self::WrongCeremony => "client data is not from a webauthn.get ceremony",
            Self::ChallengeMismatch => "challenge does not match the one issued",
            Self::OriginMismatch => "origin does not match the configured origin",
            Self::CrossOrigin => "client data reports a cross-origin (framed) ceremony",
            Self::AuthenticatorDataMalformed => "authenticator data has the wrong length",
            Self::RpIdMismatch => "relying-party id hash does not match",
            Self::UserNotPresent => "user presence flag is not set",
            Self::UserNotVerified => "user verification flag is not set",
            Self::BackupStateInconsistent => "backed-up flag is set on a non-backup-eligible key",
            Self::UnrequestedOutputs => {
                "assertion carries extension outputs or attested credential data that were not requested"
            }
            Self::SignatureInvalid => "signature does not verify under the registered key",
            Self::SignCountNotAdvanced => "signature counter did not advance (cloned key?)",
        })
    }
}

impl Assertion {
    /// The challenge the client data names (step 11), for the caller to
    /// consume from the one-shot store before verifying.
    pub(super) fn challenge(&self) -> Result<String, AssertionError> {
        Ok(self.client_data()?.challenge)
    }

    /// Steps 7 to 9: UTF-8 decode the client data, stripping a byte order
    /// mark, and parse it as JSON.
    fn client_data(&self) -> Result<ClientData, AssertionError> {
        let json = self
            .response
            .client_data_json
            .strip_prefix(b"\xEF\xBB\xBF")
            .unwrap_or(&self.response.client_data_json);
        serde_json::from_slice(json).map_err(|_| AssertionError::ClientDataMalformed)
    }

    /// Steps 3, 4 and 7 to 21 and 23. Every check but the first two is on
    /// data under the signature, which is checked last so a rejection names
    /// the first check that failed. Yields the signature counter for step 22.
    pub(super) fn verify(&self, expected: &Expected<'_>) -> Result<u32, AssertionError> {
        // Step 3: serialised, a `PublicKeyCredential` carrying an
        // `AuthenticatorAssertionResponse` is one of type `public-key` with
        // the response fields parsed above.
        if self.credential_type != "public-key" {
            return Err(AssertionError::NotPublicKeyCredential);
        }
        // Step 6: when the key reports a user handle, it must be the one of
        // the account the credential is registered for.
        if let Some(user_handle) = &self.response.user_handle
            && !bool::from(user_handle.ct_eq(expected.user_handle))
        {
            return Err(AssertionError::UserHandleMismatch);
        }
        self.verify_client_data(expected)?;
        let sign_count = self.verify_authenticator_data(expected)?;
        self.verify_signature(expected.public_key)?;
        Ok(sign_count)
    }

    /// Steps 7 to 14: the client data.
    fn verify_client_data(&self, expected: &Expected<'_>) -> Result<(), AssertionError> {
        let client_data = self.client_data()?;
        // Step 10.
        if client_data.ceremony != "webauthn.get" {
            return Err(AssertionError::WrongCeremony);
        }
        // Step 11: the challenge as the options carried it, base64url.
        if !bool::from(
            client_data
                .challenge
                .as_bytes()
                .ct_eq(expected.challenge.as_bytes()),
        ) {
            return Err(AssertionError::ChallengeMismatch);
        }
        // Step 12: the one expected origin is the configured one (§13.5.9).
        let origin = Url::parse(&client_data.origin)
            .map(|url| url.origin())
            .map_err(|_| AssertionError::OriginMismatch)?;
        if &origin != expected.origin {
            return Err(AssertionError::OriginMismatch);
        }
        // Steps 13 and 14: the login page is never framed, so neither the
        // cross-origin flag nor a top origin is expected.
        if client_data.cross_origin || client_data.top_origin.is_some() {
            return Err(AssertionError::CrossOrigin);
        }
        Ok(())
    }

    /// Steps 15 to 18 and 23: the authenticator data (§6.1), yielding its
    /// signature counter.
    fn verify_authenticator_data(&self, expected: &Expected<'_>) -> Result<u32, AssertionError> {
        let authenticator_data = &self.response.authenticator_data;
        if authenticator_data.len() < AUTHENTICATOR_DATA_LEN {
            return Err(AssertionError::AuthenticatorDataMalformed);
        }
        // Step 15.
        if !bool::from(authenticator_data[..32].ct_eq(expected.rp_id_hash)) {
            return Err(AssertionError::RpIdMismatch);
        }
        let flags = authenticator_data[32];
        // Step 16.
        if flags & FLAG_USER_PRESENT == 0 {
            return Err(AssertionError::UserNotPresent);
        }
        // Step 17: the options request `userVerification: "required"`.
        if flags & FLAG_USER_VERIFIED == 0 {
            return Err(AssertionError::UserNotVerified);
        }
        // Step 18.
        if flags & FLAG_BACKUP_ELIGIBLE == 0 && flags & FLAG_BACKUP_STATE != 0 {
            return Err(AssertionError::BackupStateInconsistent);
        }
        // Step 23: no extensions were requested, so unsolicited extension
        // outputs (client or authenticator) are rejected rather than
        // ignored, as is attested credential data. The authenticator data
        // then holds nothing beyond the signature counter.
        if flags & (FLAG_ATTESTED_CREDENTIAL_DATA | FLAG_EXTENSION_DATA) != 0
            || !self.client_extension_results.is_empty()
        {
            return Err(AssertionError::UnrequestedOutputs);
        }
        if authenticator_data.len() != AUTHENTICATOR_DATA_LEN {
            return Err(AssertionError::AuthenticatorDataMalformed);
        }
        Ok(u32::from_be_bytes(
            authenticator_data[33..37]
                .try_into()
                .expect("four counter bytes"),
        ))
    }

    /// Steps 20 and 21: the signature is over
    /// authenticatorData || SHA-256(clientDataJSON).
    fn verify_signature(&self, public_key: &VerifyingKey) -> Result<(), AssertionError> {
        let mut signed = self.response.authenticator_data.clone();
        signed.extend_from_slice(&Sha256::digest(&self.response.client_data_json));
        let signature = Signature::from_der(&self.response.signature)
            .map_err(|_| AssertionError::SignatureInvalid)?;
        public_key
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

/// A user handle is never empty (§14.6.1), so an empty one counts as absent.
fn optional_base64url<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<u8>>, D::Error> {
    Option::<String>::deserialize(deserializer)?
        .map(|encoded| {
            BASE64_URL_SAFE_NO_PAD
                .decode(encoded)
                .map_err(serde::de::Error::custom)
        })
        .transpose()
        .map(|handle| handle.filter(|bytes| !bytes.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::csb_webauthn::test_support::{
        TEST_ORIGIN, TestAssertion, test_signing_key, test_user_handle,
    };

    const CHALLENGE: [u8; 32] = [7; 32];

    fn verify_parsed(parsed: &Assertion) -> Result<u32, AssertionError> {
        let origin = Url::parse(TEST_ORIGIN).expect("url").origin();
        let rp_id_hash: [u8; 32] = Sha256::digest(b"csb.example.nl").into();
        let signing_key = test_signing_key();
        parsed.verify(&Expected {
            challenge: &BASE64_URL_SAFE_NO_PAD.encode(CHALLENGE),
            origin: &origin,
            rp_id_hash: &rp_id_hash,
            user_handle: &test_user_handle(),
            public_key: signing_key.verifying_key(),
        })
    }

    fn verify(assertion: &TestAssertion) -> Result<u32, AssertionError> {
        let parsed: Assertion = serde_json::from_str(&assertion.json()).expect("assertion json");
        assert_eq!(parsed.raw_id, assertion.credential_id);
        verify_parsed(&parsed)
    }

    #[test]
    fn challenge_is_read_from_the_client_data() {
        let mut assertion = TestAssertion::new(&CHALLENGE);
        assertion.client_data_bom = true;
        let parsed: Assertion = serde_json::from_str(&assertion.json()).expect("assertion json");
        assert_eq!(
            parsed.challenge().as_deref(),
            Ok(BASE64_URL_SAFE_NO_PAD.encode(CHALLENGE).as_str())
        );
    }

    #[test]
    fn accepts_a_valid_assertion_and_yields_the_counter() {
        let mut assertion = TestAssertion::new(&CHALLENGE);
        assertion.sign_count = 42;
        assert_eq!(verify(&assertion), Ok(42));

        // A key that reports its user handle (step 6) and a client that
        // marks the ceremony same-origin or prefixes a byte order mark.
        assertion.user_handle = Some(test_user_handle());
        assertion.cross_origin = Some(false);
        assertion.client_data_bom = true;
        assert_eq!(verify(&assertion), Ok(42));

        // An empty user handle is as good as none.
        assertion.user_handle = Some(Vec::new());
        assert_eq!(verify(&assertion), Ok(42));
    }

    type Tamper = fn(&mut TestAssertion);

    fn assert_rejected<const N: usize>(cases: [(&str, Tamper, AssertionError); N]) {
        for (what, tamper, expected) in cases {
            let mut assertion = TestAssertion::new(&CHALLENGE);
            tamper(&mut assertion);
            assert_eq!(verify(&assertion), Err(expected), "{what}");
        }
    }

    /// Steps 3 to 14: the credential and the client data.
    #[test]
    fn rejects_tampered_credential_and_client_data() {
        assert_rejected([
            (
                "credential type",
                |a| a.credential_type = "public-key-x",
                AssertionError::NotPublicKeyCredential,
            ),
            (
                "user handle",
                |a| a.user_handle = Some(vec![0x44; 32]),
                AssertionError::UserHandleMismatch,
            ),
            (
                "user handle length",
                |a| a.user_handle = Some(test_user_handle()[..31].to_vec()),
                AssertionError::UserHandleMismatch,
            ),
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
                "cross origin",
                |a| a.cross_origin = Some(true),
                AssertionError::CrossOrigin,
            ),
            (
                "top origin",
                |a| a.top_origin = Some(TEST_ORIGIN.to_string()),
                AssertionError::CrossOrigin,
            ),
        ]);
    }

    /// Steps 15 to 18 and 23: the authenticator data and extension outputs.
    #[test]
    fn rejects_tampered_authenticator_data() {
        assert_rejected([
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
            (
                "backup state without eligibility",
                |a| a.extra_flags = FLAG_BACKUP_STATE,
                AssertionError::BackupStateInconsistent,
            ),
            (
                "attested credential data",
                |a| {
                    a.extra_flags = FLAG_ATTESTED_CREDENTIAL_DATA;
                    a.trailing_data = vec![0; 16];
                },
                AssertionError::UnrequestedOutputs,
            ),
            (
                "authenticator extension outputs",
                |a| {
                    a.extra_flags = FLAG_EXTENSION_DATA;
                    a.trailing_data = vec![0xa0];
                },
                AssertionError::UnrequestedOutputs,
            ),
            (
                "client extension outputs",
                |a| a.client_extension_results = serde_json::json!({ "appid": true }),
                AssertionError::UnrequestedOutputs,
            ),
            (
                "trailing bytes without a flag announcing them",
                |a| a.trailing_data = vec![0; 4],
                AssertionError::AuthenticatorDataMalformed,
            ),
        ]);
    }

    /// A backup-eligible key may report either backup state (step 18).
    #[test]
    fn accepts_backup_flags_of_an_eligible_key() {
        for extra_flags in [
            FLAG_BACKUP_ELIGIBLE,
            FLAG_BACKUP_ELIGIBLE | FLAG_BACKUP_STATE,
        ] {
            let mut assertion = TestAssertion::new(&CHALLENGE);
            assertion.extra_flags = extra_flags;
            assert_eq!(verify(&assertion), Ok(7), "{extra_flags:#x}");
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
        assert_eq!(
            verify_parsed(&parsed),
            Err(AssertionError::SignatureInvalid)
        );

        let mut short = valid.clone();
        short["response"]["authenticatorData"] = "AAAA".into();
        let parsed: Assertion = serde_json::from_value(short).expect("parses");
        assert_eq!(
            verify_parsed(&parsed),
            Err(AssertionError::AuthenticatorDataMalformed)
        );

        let mut garbage = valid.clone();
        garbage["response"]["clientDataJSON"] = "AAAA".into();
        let parsed: Assertion = serde_json::from_value(garbage).expect("parses");
        assert_eq!(parsed.challenge(), Err(AssertionError::ClientDataMalformed));
        assert_eq!(
            verify_parsed(&parsed),
            Err(AssertionError::ClientDataMalformed)
        );

        for (field, value) in [
            ("rawId", serde_json::json!("not base64!")),
            ("clientExtensionResults", serde_json::json!([])),
        ] {
            let mut bad = valid.clone();
            bad[field] = value;
            assert!(serde_json::from_value::<Assertion>(bad).is_err(), "{field}");
        }
        let mut no_type = valid.clone();
        no_type.as_object_mut().expect("object").remove("type");
        assert!(serde_json::from_value::<Assertion>(no_type).is_err());
        let mut bad_handle = valid;
        bad_handle["response"]["userHandle"] = "not base64!".into();
        assert!(serde_json::from_value::<Assertion>(bad_handle).is_err());
        assert!(serde_json::from_str::<Assertion>("{}").is_err());
    }
}
