//! The in-flight WebAuthn ceremony state, carried between the start and
//! finish requests in an encrypted cookie bound to the browser that started
//! the ceremony, like the GitHub `state` cookie. The cookie holds the
//! webauthn-rs state (challenge and expected credentials) plus a one-shot
//! nonce registered in the pending-request store, so a finish request can be
//! accepted only once and only by the browser the challenge was issued to.
//! The server-side 15-minute pending TTL bounds the cookie's useful life.

use axum_extra::extract::{
    CookieJar,
    cookie::{Cookie, SameSite},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use secrecy::SecretString;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tracing::warn;
use webauthn_rs::prelude::{PasskeyAuthentication, PasskeyRegistration};

use super::derive_key;
use crate::{
    AppError, CsbUser, PasskeyAccountId, PasskeyAccountName, PasskeyLabel, crypto::EventCipher,
};

/// Name of the ceremony-state cookie. Like the session cookie, the `__Host-`
/// prefix (production only) forbids a `Domain` and requires `Secure` + `Path=/`.
#[cfg(feature = "dev-features")]
pub const STATE_COOKIE_NAME: &str = "EKS_PASSKEY_STATE";
#[cfg(not(feature = "dev-features"))]
pub const STATE_COOKIE_NAME: &str = "__Host-EKS_PASSKEY_STATE";

/// Domain-separation salt, distinct from the stream key-wrapping salt.
const STATE_HKDF_SALT: &[u8] = b"e-KS passkey ceremony state v1";
const STATE_HKDF_INFO: &[u8] = b"passkey-state-key:v1";

/// Which ceremony a sealed state belongs to. Bound into the ciphertext as
/// associated data, so a registration state can never be opened as a login
/// state or the other way round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Login,
    Register,
}

impl Purpose {
    fn aad(self) -> &'static [u8] {
        match self {
            Purpose::Login => b"passkey-ceremony:login",
            Purpose::Register => b"passkey-ceremony:register",
        }
    }
}

/// State of a login ceremony. An unknown account name gets a decoy challenge
/// so the login endpoint does not reveal which names exist; the decoy state
/// records only that the finish must fail.
#[derive(Debug, Serialize, Deserialize)]
pub enum LoginCeremony {
    Real {
        nonce: String,
        account_id: PasskeyAccountId,
        authentication: PasskeyAuthentication,
    },
    Decoy {
        nonce: String,
    },
}

impl LoginCeremony {
    pub fn nonce(&self) -> &str {
        match self {
            LoginCeremony::Real { nonce, .. } | LoginCeremony::Decoy { nonce } => nonce,
        }
    }
}

/// Which account a registration adds its passkey to. A new account is only
/// persisted when the ceremony finishes, so abandoned registrations leave no
/// empty accounts behind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegisterTarget {
    Existing(PasskeyAccountId),
    New {
        id: PasskeyAccountId,
        name: PasskeyAccountName,
    },
}

impl RegisterTarget {
    pub fn account_id(&self) -> PasskeyAccountId {
        match self {
            RegisterTarget::Existing(id) | RegisterTarget::New { id, .. } => *id,
        }
    }
}

/// State of a registration ceremony, including who started it: the finish
/// request must come from the same session user.
#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterCeremony {
    pub nonce: String,
    pub label: PasskeyLabel,
    pub created_by: CsbUser,
    pub target: RegisterTarget,
    pub registration: PasskeyRegistration,
}

/// Seals and opens ceremony state with a key derived from the master secret.
#[derive(Clone)]
pub struct PasskeyStateCipher {
    cipher: EventCipher,
}

impl PasskeyStateCipher {
    /// Derive the state key from the master secret with HKDF-SHA256.
    pub fn new(master: &SecretString) -> Self {
        Self {
            cipher: EventCipher::from_key(&derive_key(master, STATE_HKDF_SALT, STATE_HKDF_INFO)),
        }
    }

    /// The state as an encrypted, browser-bound cookie.
    pub fn seal<T: Serialize>(
        &self,
        state: &T,
        purpose: Purpose,
    ) -> Result<Cookie<'static>, AppError> {
        let sealed = self.cipher.encrypt(state, purpose.aad())?;
        Ok(build_state_cookie(URL_SAFE_NO_PAD.encode(sealed)))
    }

    /// The state from the request's cookie, or `None` when there is none or
    /// it was not sealed by this cipher for this purpose. Fails closed.
    pub fn open<T: DeserializeOwned>(&self, jar: &CookieJar, purpose: Purpose) -> Option<T> {
        let cookie = jar.get(STATE_COOKIE_NAME)?;
        let Ok(sealed) = URL_SAFE_NO_PAD.decode(cookie.value()) else {
            warn!("passkey ceremony state cookie is not base64url");
            return None;
        };
        match self.cipher.decrypt(sealed, purpose.aad()) {
            Ok(state) => Some(state),
            Err(err) => {
                warn!("passkey ceremony state cookie rejected: {err:?}");
                None
            }
        }
    }
}

fn apply_state_cookie_attributes(cookie: &mut Cookie<'static>) {
    cookie.set_http_only(true);
    #[cfg(feature = "dev-features")]
    cookie.set_secure(false);
    #[cfg(not(feature = "dev-features"))]
    cookie.set_secure(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_path("/");
}

fn build_state_cookie(value: String) -> Cookie<'static> {
    let mut cookie = Cookie::new(STATE_COOKIE_NAME, value);
    apply_state_cookie_attributes(&mut cookie);
    cookie
}

/// Expired twin of the state cookie for clearing it; attributes must match.
pub fn build_state_removal_cookie() -> Cookie<'static> {
    let mut cookie = Cookie::from(STATE_COOKIE_NAME);
    apply_state_cookie_attributes(&mut cookie);
    cookie
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher() -> PasskeyStateCipher {
        PasskeyStateCipher::new(&SecretString::from("test-master-secret"))
    }

    fn jar_with(cookie: Cookie<'static>) -> CookieJar {
        CookieJar::new().add(cookie)
    }

    #[test]
    fn seals_and_opens_for_the_same_purpose() {
        let cipher = cipher();
        let state = LoginCeremony::Decoy {
            nonce: "nonce-1".to_string(),
        };
        let cookie = cipher.seal(&state, Purpose::Login).expect("sealed");
        assert_eq!(cookie.name(), STATE_COOKIE_NAME);
        assert!(cookie.http_only().unwrap_or(false));
        assert_eq!(cookie.same_site(), Some(SameSite::Lax));

        let opened: LoginCeremony = cipher
            .open(&jar_with(cookie), Purpose::Login)
            .expect("opened");
        assert_eq!(opened.nonce(), "nonce-1");
    }

    #[test]
    fn rejects_other_purpose_other_key_tampering_and_garbage() {
        let cipher = cipher();
        let state = LoginCeremony::Decoy {
            nonce: "nonce-1".to_string(),
        };
        let cookie = cipher.seal(&state, Purpose::Login).expect("sealed");

        let other_purpose: Option<LoginCeremony> =
            cipher.open(&jar_with(cookie.clone()), Purpose::Register);
        assert!(other_purpose.is_none());

        let other_key = PasskeyStateCipher::new(&SecretString::from("another-secret"));
        let foreign: Option<LoginCeremony> =
            other_key.open(&jar_with(cookie.clone()), Purpose::Login);
        assert!(foreign.is_none());

        let mut tampered = URL_SAFE_NO_PAD.decode(cookie.value()).unwrap();
        let last = tampered.len() - 1;
        tampered[last] ^= 0x01;
        let tampered = build_state_cookie(URL_SAFE_NO_PAD.encode(tampered));
        let opened: Option<LoginCeremony> = cipher.open(&jar_with(tampered), Purpose::Login);
        assert!(opened.is_none());

        let garbage: Option<LoginCeremony> = cipher.open(
            &jar_with(build_state_cookie("!!".to_string())),
            Purpose::Login,
        );
        assert!(garbage.is_none());

        let absent: Option<LoginCeremony> = cipher.open(&CookieJar::new(), Purpose::Login);
        assert!(absent.is_none());
    }

    #[test]
    fn removal_cookie_matches_the_state_cookie() {
        let removal = build_state_removal_cookie();
        assert_eq!(removal.name(), STATE_COOKIE_NAME);
        assert_eq!(removal.path(), Some("/"));
        assert_eq!(removal.same_site(), Some(SameSite::Lax));
    }
}
