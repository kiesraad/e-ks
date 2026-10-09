//! Configuration of the CSB security-key login: the WebAuthn relying party
//! and the committee members registered in advance, each with the credential
//! id and public key of their security key (see `docs/csb-enrolment.md`).

use base64::{Engine, prelude::BASE64_STANDARD};
use p256::{ecdsa::VerifyingKey, pkcs8::DecodePublicKey};
use sha2::{Digest, Sha256};
use url::{Origin, Url};

use crate::{AppError, CsbUsername};

/// Security-key (WebAuthn) login for CSB users.
///
/// Fully separate from the political-group login (SAML, auth-service):
/// committee members prove possession of a pre-registered security key, with
/// user verification (PIN) required.
#[derive(Debug, Clone)]
pub struct CsbWebauthnConfig {
    /// The origin the CSB section is served on; assertions must come from it.
    pub origin: Origin,
    /// The relying-party id: the origin's host.
    pub rp_id: String,
    /// Committee members allowed to log in, with their registered keys.
    pub users: Vec<CsbWebauthnUser>,
}

/// One pre-registered committee member.
#[derive(Debug, Clone)]
pub struct CsbWebauthnUser {
    pub username: CsbUsername,
    /// The security key's credential id, as the authenticator reports it.
    pub credential_id: Vec<u8>,
    /// The ES256 (P-256) public key of that credential.
    pub public_key: VerifyingKey,
}

impl CsbWebauthnConfig {
    /// Parses `origin` (`CSB_WEBAUTHN_ORIGIN`, e.g. `https://csb.example.nl`)
    /// and the comma-separated `username:credential-id:public-key` list
    /// (`CSB_WEBAUTHN_USERS`; both blobs base64 as `fido2-cred` prints them,
    /// the key as the body of its PEM). Strict: one malformed entry rejects
    /// the whole configuration rather than silently dropping a user.
    pub fn parse(origin: &str, users: Option<&str>) -> Result<Self, AppError> {
        let config_error = |message: String| AppError::ConfigLoadError(message);

        let url = Url::parse(origin.trim())
            .map_err(|err| config_error(format!("CSB_WEBAUTHN_ORIGIN: {err}")))?;
        let rp_id = url
            .host_str()
            .filter(|_| matches!(url.scheme(), "https" | "http"))
            .ok_or_else(|| {
                config_error(
                    "CSB_WEBAUTHN_ORIGIN must be an http(s) origin with a host".to_string(),
                )
            })?
            .to_string();

        let users = users
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(parse_user)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| config_error(format!("CSB_WEBAUTHN_USERS: {err}")))?;

        for (index, user) in users.iter().enumerate() {
            let earlier = &users[..index];
            if earlier.iter().any(|other| other.username == user.username) {
                return Err(config_error(format!(
                    "CSB_WEBAUTHN_USERS: username {} is listed twice",
                    user.username
                )));
            }
            if earlier
                .iter()
                .any(|other| other.credential_id == user.credential_id)
            {
                return Err(config_error(format!(
                    "CSB_WEBAUTHN_USERS: the security key of {} is registered for another user too",
                    user.username
                )));
            }
        }

        Ok(Self {
            origin: url.origin(),
            rp_id,
            users,
        })
    }

    /// SHA-256 of the relying-party id, as the authenticator signs it.
    pub fn rp_id_hash(&self) -> [u8; 32] {
        Sha256::digest(self.rp_id.as_bytes()).into()
    }

    pub fn user_by_credential_id(&self, credential_id: &[u8]) -> Option<&CsbWebauthnUser> {
        self.users
            .iter()
            .find(|user| user.credential_id == credential_id)
    }
}

/// One `username:credential-id:public-key` entry.
fn parse_user(entry: &str) -> Result<CsbWebauthnUser, String> {
    let mut parts = entry.split(':');
    let (Some(username), Some(credential_id), Some(public_key), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(format!(
            "entry {entry:?} is not username:credential-id:public-key"
        ));
    };
    let credential_id = BASE64_STANDARD
        .decode(credential_id)
        .map_err(|_| format!("credential id of {username} is not base64"))?;
    if credential_id.is_empty() {
        return Err(format!("credential id of {username} is empty"));
    }
    let public_key = BASE64_STANDARD
        .decode(public_key)
        .map_err(|_| format!("public key of {username} is not base64"))?;
    let public_key = VerifyingKey::from_public_key_der(&public_key)
        .map_err(|_| format!("public key of {username} is not a P-256 public key"))?;
    Ok(CsbWebauthnUser {
        username: username.parse()?,
        credential_id,
        public_key,
    })
}

#[cfg(test)]
pub(crate) mod test_support {
    use base64::{Engine, prelude::BASE64_STANDARD};
    use p256::{ecdsa::SigningKey, pkcs8::EncodePublicKey};

    pub(crate) const TEST_ORIGIN: &str = "https://csb.example.nl";

    /// Private key of the test security key, so tests can sign assertions.
    pub(crate) fn test_signing_key() -> SigningKey {
        SigningKey::from_bytes(&[0x42; 32].into()).expect("valid scalar")
    }

    pub(crate) fn test_credential_id() -> Vec<u8> {
        vec![0x11; 64]
    }

    /// The `credential-id:public-key` part of a configuration entry for the
    /// test key, as `bin/enrol_csb_user` prints it.
    pub(crate) fn test_credential_encoded() -> String {
        let der = test_signing_key()
            .verifying_key()
            .to_public_key_der()
            .expect("spki der");
        format!(
            "{}:{}",
            BASE64_STANDARD.encode(test_credential_id()),
            BASE64_STANDARD.encode(der.as_bytes())
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{test_support::*, *};

    #[test]
    fn parse_builds_relying_party_and_users() {
        let users = format!("alice:{0}, bob:{0}", test_credential_encoded());
        // Two users with the same key is a misconfiguration.
        assert!(CsbWebauthnConfig::parse(TEST_ORIGIN, Some(&users)).is_err());

        let users = format!("alice:{}", test_credential_encoded());
        let config = CsbWebauthnConfig::parse(TEST_ORIGIN, Some(&users)).expect("config");
        assert_eq!(config.rp_id, "csb.example.nl");
        assert_eq!(
            config.origin,
            Url::parse(TEST_ORIGIN).expect("url").origin()
        );
        assert_eq!(config.users.len(), 1);
        let alice = config
            .user_by_credential_id(&test_credential_id())
            .expect("alice");
        assert_eq!(alice.username.as_str(), "alice");
        assert_eq!(&alice.public_key, test_signing_key().verifying_key());
        assert!(config.user_by_credential_id(&[0x22; 64]).is_none());
    }

    #[test]
    fn parse_accepts_an_origin_without_users() {
        let config = CsbWebauthnConfig::parse("http://localhost:3000", None).expect("config");
        assert_eq!(config.rp_id, "localhost");
        assert!(config.users.is_empty());
    }

    #[test]
    fn parse_rejects_bad_origins_and_entries() {
        for origin in ["csb.example.nl", "mailto:x", "file:///tmp"] {
            assert!(CsbWebauthnConfig::parse(origin, None).is_err(), "{origin}");
        }

        let credential = test_credential_encoded();
        let (id, key) = credential.split_once(':').expect("two parts");
        for users in [
            "alice".to_string(),
            format!("alice:{id}"),
            format!("alice:{credential}:extra"),
            format!("al ice:{credential}"),
            format!("alice:{credential},alice:{credential}"),
            format!("alice:not*base64:{key}"),
            format!("alice::{key}"),
            format!("alice:{id}:{}", BASE64_STANDARD.encode("not a key")),
        ] {
            assert!(
                CsbWebauthnConfig::parse(TEST_ORIGIN, Some(&users)).is_err(),
                "{users}"
            );
        }
    }
}
