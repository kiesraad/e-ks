//! Security-key (WebAuthn) login for CSB (central electoral committee) users.
//!
//! Fully separate from the political-group login (SAML DigiD/TVS, implemented
//! in the auth-service): committee members are registered in advance through
//! the configuration ([`crate::CsbWebauthnConfig`]: a username plus the
//! credential id and public key of their security key, enrolled with
//! `bin/enrol_csb_user`, see `docs/csb-enrolment.md`) and prove possession of
//! that key with user verification (the key's PIN) on every login. Nothing is
//! stored at runtime and there is no registration endpoint.
//!
//! Flow and defences:
//! - `GET /csb/login` mints a random challenge, registers it one-shot in the
//!   pending-request store (15-minute TTL), binds it to the browser with a
//!   short-lived cookie ([`state_cookie`]), and renders the page that runs
//!   `navigator.credentials.get` with every configured credential id, so the
//!   key identifies the user and no username is typed.
//! - `POST /csb/login/finish` consumes the challenge (replay defence), looks
//!   the credential id up in the configuration, and verifies the assertion
//!   ([`assertion`]): ceremony type, challenge, origin, relying-party id,
//!   user presence and verification, and the signature under the configured
//!   public key.
//! - A successful login drops any pre-existing session (fixation defence) and
//!   creates a [`crate::SessionUser::CentralElectoralCommittee`] session,
//!   recording the login on the shared CSB main stream for the audit log.
//! - Every failure ends on the login page with one generic message, so
//!   nothing reveals which check failed.
//!
//! Signature counters are not tracked (the configuration is static), so a
//! cloned key is not detected; the key's PIN and physical presence remain
//! required.

mod assertion;
mod pages;
mod paths;
mod state_cookie;

pub use pages::public_router;
pub use paths::{CsbLoginFinishPath, CsbLoginPath};

use crate::{AppError, Config, CsbWebauthnConfig};

/// The security-key config, or 404 when this deployment has no CSB login.
fn require_webauthn(config: &Config) -> Result<&CsbWebauthnConfig, AppError> {
    config
        .csb_webauthn
        .as_ref()
        .ok_or(AppError::GenericNotFound)
}

/// Pending-request id for a login challenge (base64url). Namespaced so
/// challenges can never collide with SAML AuthnRequest ids in the shared
/// store.
fn pending_challenge_id(challenge: &str) -> String {
    format!("csb-webauthn:{challenge}")
}

#[cfg(test)]
pub(crate) mod test_support {
    use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
    use p256::ecdsa::{Signature, SigningKey, signature::Signer};
    use sha2::{Digest, Sha256};

    pub(crate) use crate::core::csb_webauthn_test_support::*;
    use crate::{Config, CsbUsername, CsbWebauthnConfig};

    /// The username whose security key is the test key.
    pub(crate) fn alice() -> CsbUsername {
        "alice".parse().expect("valid username")
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
    /// `challenge`, with every signed field adjustable so tests can produce
    /// assertions that must be rejected.
    pub(crate) struct TestAssertion {
        pub(crate) challenge: Vec<u8>,
        pub(crate) ceremony: &'static str,
        pub(crate) origin: String,
        pub(crate) rp_id: String,
        pub(crate) user_present: bool,
        pub(crate) user_verified: bool,
        pub(crate) credential_id: Vec<u8>,
        pub(crate) signing_key: SigningKey,
    }

    impl TestAssertion {
        pub(crate) fn new(challenge: &[u8]) -> Self {
            Self {
                challenge: challenge.to_vec(),
                ceremony: "webauthn.get",
                origin: TEST_ORIGIN.to_string(),
                rp_id: "csb.example.nl".to_string(),
                user_present: true,
                user_verified: true,
                credential_id: test_credential_id(),
                signing_key: test_signing_key(),
            }
        }

        /// The assertion as the browser script posts it.
        pub(crate) fn json(&self) -> String {
            let client_data = serde_json::to_vec(&serde_json::json!({
                "type": self.ceremony,
                "challenge": BASE64_URL_SAFE_NO_PAD.encode(&self.challenge),
                "origin": self.origin,
                "crossOrigin": false,
            }))
            .expect("json");

            let mut flags = 0u8;
            if self.user_present {
                flags |= 0x01;
            }
            if self.user_verified {
                flags |= 0x04;
            }
            let mut auth_data: Vec<u8> = Sha256::digest(self.rp_id.as_bytes()).to_vec();
            auth_data.push(flags);
            auth_data.extend_from_slice(&7u32.to_be_bytes());

            let mut signed = auth_data.clone();
            signed.extend_from_slice(&Sha256::digest(&client_data));
            let signature: Signature = self.signing_key.sign(&signed);

            let b64 = |bytes: &[u8]| BASE64_URL_SAFE_NO_PAD.encode(bytes);
            serde_json::json!({
                "id": b64(&self.credential_id),
                "rawId": b64(&self.credential_id),
                "type": "public-key",
                "response": {
                    "authenticatorData": b64(&auth_data),
                    "clientDataJSON": b64(&client_data),
                    "signature": b64(signature.to_der().as_bytes()),
                    "userHandle": null,
                },
                "clientExtensionResults": {},
            })
            .to_string()
        }
    }

    /// A valid assertion of the test key for `challenge`.
    pub(crate) fn signed_assertion(challenge: &[u8]) -> String {
        TestAssertion::new(challenge).json()
    }
}
