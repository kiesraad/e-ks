//! The configured WebAuthn relying party, bundled with the credential store
//! and the ceremony-state cipher the login and management handlers need.

use std::sync::Arc;

use secrecy::SecretString;
use tracing::warn;
use webauthn_rs::{
    DEFAULT_AUTHENTICATOR_TIMEOUT,
    prelude::{RequestChallengeResponse, Webauthn, WebauthnBuilder, WebauthnError},
};
use webauthn_rs_core::{
    fake::{FakePasskeyDistribution, WebauthnFakeCredentialGenerator},
    proto::{AllowCredentials, PublicKeyCredentialRequestOptions, UserVerificationPolicy},
};

use super::{CsbPasskeyStore, PasskeyStateCipher, derive_key};
use crate::{AppError, AppRequestState, CsbPasskeyConfig};

/// Passkeys one account may hold. Bounds the authentication state that
/// travels in the ceremony cookie, which embeds every allowed credential.
pub const MAX_PASSKEYS_PER_ACCOUNT: usize = 5;

/// Shown by authenticators as the site asking for the passkey.
const RP_NAME: &str = "Kiesraad centraal stembureau";

/// Domain-separation salt for the decoy credential generator's HMAC key.
const DECOY_HKDF_SALT: &[u8] = b"e-KS passkey decoy credentials v1";
const DECOY_HKDF_INFO: &[u8] = b"passkey-decoy-key:v1";

/// The passkey login of this deployment.
#[derive(Clone)]
pub struct PasskeyLogin {
    webauthn: Arc<Webauthn>,
    /// Deterministic fake credential ids for names no account has, so the
    /// login start cannot be used to enumerate account names. Keyed from the
    /// master secret: the key must stay unknown for the decoys to be
    /// indistinguishable from real credentials.
    decoys: Arc<WebauthnFakeCredentialGenerator<FakePasskeyDistribution>>,
    rp_id: String,
    store: CsbPasskeyStore,
    state: PasskeyStateCipher,
}

impl PasskeyLogin {
    pub fn new(
        config: &CsbPasskeyConfig,
        store: CsbPasskeyStore,
        master: &SecretString,
    ) -> Result<Self, AppError> {
        let webauthn = WebauthnBuilder::new(&config.rp_id, &config.origin)
            .and_then(|builder| builder.rp_name(RP_NAME).build())
            .map_err(|err| {
                AppError::ConfigLoadError(format!(
                    "CSB_PASSKEY_ORIGIN {} is not a usable WebAuthn origin: {err}",
                    config.origin
                ))
            })?;
        let decoy_key = derive_key(master, DECOY_HKDF_SALT, DECOY_HKDF_INFO);
        let decoys = WebauthnFakeCredentialGenerator::new(decoy_key.as_ref()).map_err(|err| {
            AppError::ConfigLoadError(format!("passkey decoy generator rejected its key: {err}"))
        })?;

        Ok(Self {
            webauthn: Arc::new(webauthn),
            decoys: Arc::new(decoys),
            rp_id: config.rp_id.clone(),
            store,
            state: PasskeyStateCipher::new(master),
        })
    }

    pub fn webauthn(&self) -> &Webauthn {
        &self.webauthn
    }

    pub fn store(&self) -> &CsbPasskeyStore {
        &self.store
    }

    pub fn state(&self) -> &PasskeyStateCipher {
        &self.state
    }

    /// A challenge for a name no account has, shaped like the one
    /// [`Webauthn::start_passkey_authentication`] produces for a real
    /// account: same options, fake but stable credential ids for the name.
    /// The browser then reports "no matching passkey", exactly as it does for
    /// a real account used from the wrong device.
    pub fn decoy_challenge(&self, name: &str) -> Result<RequestChallengeResponse, AppError> {
        let credential_ids = self
            .decoys
            .generate(name.trim().to_lowercase().as_bytes())
            .map_err(user_error)?;
        let allow_credentials = credential_ids
            .into_iter()
            .map(|id| AllowCredentials {
                type_: "public-key".to_string(),
                id,
                transports: None,
            })
            .collect();
        let timeout = u32::try_from(DEFAULT_AUTHENTICATOR_TIMEOUT.as_millis())
            .expect("default timeout fits in u32");

        Ok(RequestChallengeResponse {
            public_key: PublicKeyCredentialRequestOptions {
                challenge: rand::random::<[u8; 32]>().to_vec(),
                timeout: Some(timeout),
                rp_id: self.rp_id.clone(),
                allow_credentials,
                user_verification: UserVerificationPolicy::Required,
                hints: None,
                extensions: None,
            },
            mediation: None,
        })
    }
}

/// A WebAuthn failure is a bad, stale or foreign ceremony response, never a
/// server fault: logged, then reported generically.
pub fn user_error(err: WebauthnError) -> AppError {
    warn!("WebAuthn ceremony rejected: {err}");
    AppError::UserError("The passkey could not be verified".to_string())
}

/// The passkey login, or 404 when this deployment has none.
pub fn require_passkey_login<S: AppRequestState>(state: &S) -> Result<&PasskeyLogin, AppError> {
    state.passkeys().ok_or(AppError::GenericNotFound)
}

/// Pending-request ids for the ceremony nonces, namespaced like the OAuth
/// `state` so they can never collide with other one-shot ids in the store.
pub fn pending_login_id(nonce: &str) -> String {
    format!("passkey-login:{nonce}")
}

pub fn pending_register_id(nonce: &str) -> String {
    format!("passkey-register:{nonce}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::passkey::test_support::test_login;

    #[test]
    fn decoy_challenges_are_stable_per_name_and_look_real() {
        let login = test_login();

        let first = login.decoy_challenge("Nobody").unwrap();
        let second = login.decoy_challenge("  nobody ").unwrap();
        let other = login.decoy_challenge("Someone else").unwrap();

        let ids = |rcr: &RequestChallengeResponse| {
            rcr.public_key
                .allow_credentials
                .iter()
                .map(|c| c.id.clone())
                .collect::<Vec<_>>()
        };
        assert!(!ids(&first).is_empty());
        assert_eq!(ids(&first), ids(&second), "same name, same credential ids");
        assert_ne!(ids(&first), ids(&other));
        assert_ne!(
            first.public_key.challenge, second.public_key.challenge,
            "the challenge itself is fresh"
        );
        assert_eq!(first.public_key.rp_id, "localhost");
        assert_eq!(
            first.public_key.user_verification,
            UserVerificationPolicy::Required
        );
    }

    #[test]
    fn decoy_matches_the_shape_of_a_real_challenge() {
        let login = test_login();
        let passkey = crate::auth::passkey::test_support::test_passkey(1);
        let (real, _) = login
            .webauthn()
            .start_passkey_authentication(&[passkey])
            .unwrap();
        let decoy = login.decoy_challenge("Nobody").unwrap();

        let real = serde_json::to_value(&real).unwrap();
        let decoy = serde_json::to_value(&decoy).unwrap();
        let keys = |value: &serde_json::Value| {
            let mut keys: Vec<_> = value["publicKey"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect();
            keys.sort();
            keys
        };
        assert_eq!(keys(&real), keys(&decoy));
        assert_eq!(real["publicKey"]["timeout"], decoy["publicKey"]["timeout"]);
        assert_eq!(
            real["publicKey"]["userVerification"],
            decoy["publicKey"]["userVerification"]
        );
    }

    #[test]
    fn pending_ids_are_namespaced() {
        assert_eq!(pending_login_id("abc"), "passkey-login:abc");
        assert_eq!(pending_register_id("abc"), "passkey-register:abc");
    }
}
