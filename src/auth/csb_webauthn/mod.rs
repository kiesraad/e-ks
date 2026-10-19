//! The security-key (WebAuthn) login mechanism for CSB (central electoral
//! committee) users: how a login is proven. The HTTP endpoints that drive
//! it live in `crate::csb::login`.
//!
//! Fully separate from the political-group login (SAML DigiD/TVS, implemented
//! in the auth-service): committee members are registered in advance through
//! the configuration ([`crate::CsbWebauthnConfig`]: a username plus the user
//! handle, credential id and public key of their security key, enrolled with
//! `bin/enrol_csb_user`, see `docs/csb-enrolment.md`) and prove possession of
//! that key with user verification (the key's PIN) on every login. There is
//! no registration endpoint; the only state a login leaves behind is its
//! signature counter, recorded with the login event on the CSB main stream.
//!
//! The ceremony follows §7.2 of the specification,
//! <https://w3c.github.io/webauthn/#sctn-verifying-assertion>; step numbers
//! in this module's comments are those of that section.
//!
//! - `challenge` mints the random challenge (§13.4.3) and registers it
//!   one-shot in the pending-request store; the finish consumes the one the
//!   key signed (replay defence).
//! - `assertion` verifies what the browser posts against the configured
//!   credential record: steps 3, 4 and 7 to 21 and 23.
//! - `verify_login` ties the two together: steps 5 and 6 (the credential
//!   record), step 22 (the signature counter against the one recorded at the
//!   key's last login; a counter that does not advance means a possibly
//!   cloned key) and step 25 (any failure fails the login). It yields the
//!   `VerifiedLogin` for the caller to record (step 24).

mod assertion;
mod challenge;

use std::fmt::Display;

pub(crate) use challenge::{Challenge, consume_challenge};

use crate::{CredentialId, CsbUsername, CsbWebauthnConfig, PendingRequestStore};
use assertion::{Assertion, AssertionError, Expected};

/// A login that passed every check: who logged in, with which key, and the
/// signature counter that key reported. Step 24 (updating the credential
/// record) is the caller's, after its own checks, as the step prescribes.
#[derive(Debug)]
pub(crate) struct VerifiedLogin {
    pub(crate) username: CsbUsername,
    pub(crate) credential_id: CredentialId,
    /// `authData.signCount`, to replace the recorded one (step 24).
    pub(crate) sign_count: u32,
}

/// Why a login attempt failed: for the log, never for the user.
#[derive(Debug)]
pub(crate) enum LoginFailure {
    /// Not a `PublicKeyCredential` with an `AuthenticatorAssertionResponse`
    /// (step 3).
    AssertionMalformed(serde_json::Error),
    /// Not the shape of a challenge this server mints (§13.4.3).
    ChallengeMalformed,
    /// Not issued by this server, expired, or answered before (§13.4.3).
    ChallengeNotPending,
    /// `credential.rawId` is not among the configured credentials (step 5).
    UnknownKey,
    /// A check on the assertion itself failed (steps 6 to 23).
    Rejected {
        username: CsbUsername,
        error: AssertionError,
    },
}

impl LoginFailure {
    /// The user the answering key is registered for, when the key is known.
    pub(crate) fn username(&self) -> Option<&CsbUsername> {
        match self {
            Self::Rejected { username, .. } => Some(username),
            _ => None,
        }
    }
}

impl Display for LoginFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AssertionMalformed(err) => write!(f, "assertion is malformed: {err}"),
            Self::ChallengeMalformed => f.write_str("challenge is not 32 bytes base64url"),
            Self::ChallengeNotPending => f.write_str("challenge is unknown, expired, or replayed"),
            Self::UnknownKey => f.write_str("assertion is for an unknown security key"),
            Self::Rejected { error, .. } => write!(f, "assertion rejected: {error}"),
        }
    }
}

/// Finds the key that answered, burns the challenge it signed, verifies the
/// assertion the browser posted and checks its signature counter against
/// `last_sign_count`, the counter recorded at that key's previous login (0
/// when there was none). Fails closed (step 25): any error means "login
/// failed".
pub(crate) async fn verify_login(
    pending: &PendingRequestStore,
    config: &CsbWebauthnConfig,
    credential: &str,
    last_sign_count: impl FnOnce(&CredentialId) -> u32,
) -> Result<VerifiedLogin, LoginFailure> {
    // Step 3: the response must parse as an `AuthenticatorAssertionResponse`.
    let assertion: Assertion =
        serde_json::from_str(credential).map_err(LoginFailure::AssertionMalformed)?;
    // Step 5: the options listed every configured key, so the one that
    // answered must be among them. Step 6: its record names the user, as the
    // user is not identified before the ceremony.
    let user = config
        .user_by_credential_id(&assertion.raw_id)
        .ok_or(LoginFailure::UnknownKey)?;
    let rejected = |error| LoginFailure::Rejected {
        username: user.username.clone(),
        error,
    };
    // §13.4.3: the challenge the key signed must be one this server issued
    // and not answered before; step 11 then holds the signature to it.
    let challenge = assertion.challenge().map_err(rejected)?;
    consume_challenge(pending, &challenge).await?;
    let sign_count = assertion
        .verify(&Expected {
            // Step 11: `pkOptions.challenge`, as issued.
            challenge: &challenge,
            // Step 12: the one origin expected (§13.5.9).
            origin: &config.origin,
            // Step 15: SHA-256 of the expected RP ID.
            rp_id_hash: &config.rp_id_hash(),
            // Step 21: `credentialRecord.publicKey`.
            public_key: &user.public_key,
        })
        .map_err(rejected)?;
    // Step 22, after every other check (§6.1.1): a counter that did not
    // advance since the key's last login signals a cloned key, which fails
    // the login here (the step leaves that to the relying party). Two zeroes
    // mean the key does not count.
    let stored = last_sign_count(&user.credential_id);
    if (sign_count != 0 || stored != 0) && sign_count <= stored {
        return Err(rejected(AssertionError::SignCountNotAdvanced));
    }
    Ok(VerifiedLogin {
        username: user.username.clone(),
        credential_id: user.credential_id.clone(),
        sign_count,
    })
}

#[cfg(test)]
pub(crate) mod test_support {
    use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
    use p256::ecdsa::{Signature, SigningKey, signature::Signer};
    use sha2::{Digest, Sha256};

    pub(crate) use crate::core::csb_webauthn_test_support::*;
    use crate::{Config, CredentialId, CsbUsername, CsbWebauthnConfig};

    /// The username whose security key is the test key.
    pub(crate) fn alice() -> CsbUsername {
        "alice".parse().expect("valid username")
    }

    /// A login of `alice` with the test key, as `verify_login` yields it.
    pub(crate) fn alice_login() -> super::VerifiedLogin {
        super::VerifiedLogin {
            username: alice(),
            credential_id: test_credential_id(),
            sign_count: 7,
        }
    }

    /// Test config with the security-key login enabled for `alice`.
    pub(crate) fn webauthn_test_config() -> Config {
        let mut config = Config::new_test();
        let users = format!("{}:{}", alice(), test_credential_encoded());
        config.csb_webauthn =
            Some(CsbWebauthnConfig::parse(TEST_ORIGIN, Some(&users)).expect("valid config"));
        config
    }

    /// An assertion as the test security key would produce it for
    /// `challenge`, with every checked field adjustable so tests can produce
    /// assertions that must be rejected.
    pub(crate) struct TestAssertion {
        pub(crate) credential_type: &'static str,
        pub(crate) challenge: Vec<u8>,
        pub(crate) ceremony: &'static str,
        pub(crate) origin: String,
        /// `crossOrigin` in the client data; `None` leaves it out.
        pub(crate) cross_origin: Option<bool>,
        pub(crate) top_origin: Option<String>,
        /// Prefix the client data with a UTF-8 byte order mark.
        pub(crate) client_data_bom: bool,
        pub(crate) rp_id: String,
        pub(crate) user_present: bool,
        pub(crate) user_verified: bool,
        /// Further flag bits (backup, attested data, extensions).
        pub(crate) extra_flags: u8,
        pub(crate) sign_count: u32,
        /// Bytes after the signature counter in the authenticator data.
        pub(crate) trailing_data: Vec<u8>,
        pub(crate) client_extension_results: serde_json::Value,
        pub(crate) credential_id: CredentialId,
        pub(crate) signing_key: SigningKey,
    }

    impl TestAssertion {
        pub(crate) fn new(challenge: &[u8]) -> Self {
            Self {
                credential_type: "public-key",
                challenge: challenge.to_vec(),
                ceremony: "webauthn.get",
                origin: TEST_ORIGIN.to_string(),
                cross_origin: None,
                top_origin: None,
                client_data_bom: false,
                rp_id: "csb.example.nl".to_string(),
                user_present: true,
                user_verified: true,
                extra_flags: 0,
                sign_count: 7,
                trailing_data: Vec::new(),
                client_extension_results: serde_json::json!({}),
                credential_id: test_credential_id(),
                signing_key: test_signing_key(),
            }
        }

        /// The assertion as the browser script posts it.
        pub(crate) fn json(&self) -> String {
            let mut client_data = serde_json::json!({
                "type": self.ceremony,
                "challenge": BASE64_URL_SAFE_NO_PAD.encode(&self.challenge),
                "origin": self.origin,
            });
            if let Some(cross_origin) = self.cross_origin {
                client_data["crossOrigin"] = cross_origin.into();
            }
            if let Some(top_origin) = &self.top_origin {
                client_data["topOrigin"] = top_origin.as_str().into();
            }
            let mut client_data = serde_json::to_vec(&client_data).expect("json");
            if self.client_data_bom {
                client_data.splice(0..0, *b"\xEF\xBB\xBF");
            }

            let mut flags = self.extra_flags;
            if self.user_present {
                flags |= 0x01;
            }
            if self.user_verified {
                flags |= 0x04;
            }
            let mut auth_data: Vec<u8> = Sha256::digest(self.rp_id.as_bytes()).to_vec();
            auth_data.push(flags);
            auth_data.extend_from_slice(&self.sign_count.to_be_bytes());
            auth_data.extend_from_slice(&self.trailing_data);

            let mut signed = auth_data.clone();
            signed.extend_from_slice(&Sha256::digest(&client_data));
            let signature: Signature = self.signing_key.sign(&signed);

            let b64 = |bytes: &[u8]| BASE64_URL_SAFE_NO_PAD.encode(bytes);
            serde_json::json!({
                "id": self.credential_id.to_string(),
                "rawId": self.credential_id.to_string(),
                "type": self.credential_type,
                "response": {
                    "authenticatorData": b64(&auth_data),
                    "clientDataJSON": b64(&client_data),
                    "signature": b64(signature.to_der().as_bytes()),
                },
                "clientExtensionResults": self.client_extension_results,
            })
            .to_string()
        }
    }

    /// A valid assertion of the test key for `challenge`.
    pub(crate) fn signed_assertion(challenge: &[u8]) -> String {
        TestAssertion::new(challenge).json()
    }
}

#[cfg(test)]
mod tests {
    use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};

    use super::{test_support::*, *};

    /// Issues a challenge; the bytes the key signs.
    async fn started(pending: &PendingRequestStore) -> Vec<u8> {
        let challenge = Challenge::issue(pending).await;
        BASE64_URL_SAFE_NO_PAD
            .decode(challenge.as_str())
            .expect("base64url")
    }

    /// No earlier login: nothing recorded for any key.
    fn no_counter(_: &CredentialId) -> u32 {
        0
    }

    #[tokio::test]
    async fn valid_assertion_yields_the_registered_user_and_counter() {
        let config = webauthn_test_config();
        let config = config.csb_webauthn.as_ref().expect("config");
        let pending = PendingRequestStore::default();
        let challenge = started(&pending).await;

        let login = verify_login(&pending, config, &signed_assertion(&challenge), no_counter)
            .await
            .expect("valid login");
        assert_eq!(login.username, alice());
        assert_eq!(login.credential_id, test_credential_id());
        assert_eq!(login.sign_count, 7);
    }

    /// The same challenge cannot verify twice, and a rejected assertion burns
    /// it too.
    #[tokio::test]
    async fn challenge_is_burnt_by_any_attempt() {
        let config = webauthn_test_config();
        let config = config.csb_webauthn.as_ref().expect("config");
        let pending = PendingRequestStore::default();

        let challenge = started(&pending).await;
        let assertion = signed_assertion(&challenge);
        assert!(
            verify_login(&pending, config, &assertion, no_counter)
                .await
                .is_ok()
        );
        assert!(matches!(
            verify_login(&pending, config, &assertion, no_counter).await,
            Err(LoginFailure::ChallengeNotPending)
        ));

        let challenge = started(&pending).await;
        let mut bad = TestAssertion::new(&challenge);
        bad.origin = "https://evil.example".to_string();
        let failure = verify_login(&pending, config, &bad.json(), no_counter)
            .await
            .expect_err("wrong origin");
        assert!(matches!(
            failure,
            LoginFailure::Rejected {
                error: AssertionError::OriginMismatch,
                ..
            }
        ));
        assert_eq!(failure.username(), Some(&alice()));
        let good = signed_assertion(&challenge);
        assert!(matches!(
            verify_login(&pending, config, &good, no_counter).await,
            Err(LoginFailure::ChallengeNotPending)
        ));
    }

    /// The reported counter must exceed the one recorded for the key
    /// (step 22), unless neither counts.
    #[tokio::test]
    async fn signature_counter_must_advance_on_the_recorded_one() {
        let config = webauthn_test_config();
        let config = config.csb_webauthn.as_ref().expect("config");
        let pending = PendingRequestStore::default();

        for (recorded, reported, accepted) in [
            (0, 0, true),
            (0, 1, true),
            (10, 11, true),
            (10, 10, false),
            (10, 9, false),
            (10, 0, false),
        ] {
            let challenge = started(&pending).await;
            let mut assertion = TestAssertion::new(&challenge);
            assertion.sign_count = reported;
            let result = verify_login(&pending, config, &assertion.json(), |id| {
                assert_eq!(id, &test_credential_id());
                recorded
            })
            .await;
            assert_eq!(result.is_ok(), accepted, "{recorded} -> {reported}");
            if !accepted {
                assert!(matches!(
                    result,
                    Err(LoginFailure::Rejected {
                        error: AssertionError::SignCountNotAdvanced,
                        ..
                    })
                ));
            }
        }
    }

    #[tokio::test]
    async fn unknown_key_unissued_challenge_and_malformed_credentials_fail() {
        let config = webauthn_test_config();
        let config = config.csb_webauthn.as_ref().expect("config");
        let pending = PendingRequestStore::default();

        let challenge = started(&pending).await;
        let mut unknown = TestAssertion::new(&challenge);
        unknown.credential_id = vec![0x22; 64].into();
        assert!(matches!(
            verify_login(&pending, config, &unknown.json(), no_counter).await,
            Err(LoginFailure::UnknownKey)
        ));

        // A challenge this server never issued, and one of the wrong size.
        assert!(matches!(
            verify_login(&pending, config, &signed_assertion(&[9; 32]), no_counter).await,
            Err(LoginFailure::ChallengeNotPending)
        ));
        assert!(matches!(
            verify_login(&pending, config, &signed_assertion(&[9; 31]), no_counter).await,
            Err(LoginFailure::ChallengeMalformed)
        ));

        for credential in ["", "{}", "not json", r#"{"rawId":"AAAA","response":{}}"#] {
            assert!(
                matches!(
                    verify_login(&pending, config, credential, no_counter).await,
                    Err(LoginFailure::AssertionMalformed(_))
                ),
                "{credential}"
            );
        }
    }
}
