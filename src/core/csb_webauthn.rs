//! Configuration of the CSB security-key login: the WebAuthn relying party
//! and the committee members registered in advance, each with the credential
//! id and public key of their security key: the credential record of §4 of
//! the specification (<https://w3c.github.io/webauthn/#credential-record>),
//! as far as a non-discoverable credential needs one. See
//! `docs/csb-enrolment.md`.

use std::fmt::Display;

use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use p256::{ecdsa::VerifyingKey, pkcs8::DecodePublicKey};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use url::{Origin, Url};

use crate::{AppError, CsbUsername};

/// A security key's credential id (§4 of the WebAuthn specification,
/// <https://w3c.github.io/webauthn/#credential-id>), as the authenticator
/// reports it. Displayed and serialised as base64url, like the browser's
/// `credential.id`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CredentialId(Vec<u8>);

impl From<Vec<u8>> for CredentialId {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl Display for CredentialId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&BASE64_URL_SAFE_NO_PAD.encode(&self.0))
    }
}

impl Serialize for CredentialId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for CredentialId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        BASE64_URL_SAFE_NO_PAD
            .decode(encoded)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

/// Security-key (WebAuthn) login for CSB users.
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
    pub credential_id: CredentialId,
    /// The ES256 (P-256) public key of that credential.
    pub public_key: VerifyingKey,
}

impl CsbWebauthnConfig {
    /// Parses `origin` (`CSB_WEBAUTHN_ORIGIN`, e.g. `https://csb.example.nl`)
    /// and the comma-separated `username:credential-id:public-key` list
    /// (`CSB_WEBAUTHN_USERS`, blobs base64url without padding, the key
    /// its SPKI DER, as `bin/enrol_csb_user` prints them). Strict: one
    /// malformed entry rejects the whole configuration rather than silently
    /// dropping a user.
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

    pub fn user_by_credential_id(&self, credential_id: &CredentialId) -> Option<&CsbWebauthnUser> {
        self.users
            .iter()
            .find(|user| &user.credential_id == credential_id)
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
    let credential_id = BASE64_URL_SAFE_NO_PAD
        .decode(credential_id)
        .map_err(|_| format!("credential id of {username} is not unpadded base64url"))?;
    if credential_id.is_empty() {
        return Err(format!("credential id of {username} is empty"));
    }
    let public_key = BASE64_URL_SAFE_NO_PAD
        .decode(public_key)
        .map_err(|_| format!("public key of {username} is not unpadded base64url"))?;
    let public_key = VerifyingKey::from_public_key_der(&public_key)
        .map_err(|_| format!("public key of {username} is not a P-256 public key"))?;
    Ok(CsbWebauthnUser {
        username: username.parse()?,
        credential_id: CredentialId(credential_id),
        public_key,
    })
}

#[cfg(test)]
pub(crate) mod test_support {
    use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
    use p256::{ecdsa::SigningKey, pkcs8::EncodePublicKey};

    use super::CredentialId;

    pub(crate) const TEST_ORIGIN: &str = "https://csb.example.nl";

    /// Private key of the test security key, so tests can sign assertions.
    pub(crate) fn test_signing_key() -> SigningKey {
        SigningKey::from_bytes(&[0x42; 32].into()).expect("valid scalar")
    }

    pub(crate) fn test_credential_id() -> CredentialId {
        CredentialId(vec![0x11; 64])
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
            test_credential_id(),
            BASE64_URL_SAFE_NO_PAD.encode(der.as_bytes())
        )
    }
}

#[cfg(test)]
mod tests {
    use base64::prelude::BASE64_STANDARD;

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
        assert!(
            config
                .user_by_credential_id(&CredentialId(vec![0x22; 64]))
                .is_none()
        );
    }

    /// The id travels as base64url, in the ceremony options and in events.
    #[test]
    fn credential_id_serialises_as_base64url() {
        let id = CredentialId(vec![0xfb, 0xff, 0xfe]);
        assert_eq!(id.to_string(), "-__-");
        assert_eq!(serde_json::to_string(&id).expect("json"), "\"-__-\"");
        assert_eq!(
            serde_json::from_str::<CredentialId>("\"-__-\"").expect("parses"),
            id
        );
        assert!(serde_json::from_str::<CredentialId>("\"not base64!\"").is_err());
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
        let [id, key]: [&str; 2] = credential
            .split(':')
            .collect::<Vec<_>>()
            .try_into()
            .expect("two parts");
        for users in [
            "alice".to_string(),
            format!("alice:{key}"),
            format!("alice:{id}"),
            format!("alice:{credential}:extra"),
            format!("al ice:{credential}"),
            format!("alice:{credential},alice:{credential}"),
            format!("alice:not*base64:{key}"),
            format!("alice::{key}"),
            format!("alice:{id}:{}", BASE64_URL_SAFE_NO_PAD.encode("not a key")),
            // Standard base64 (padded, with + and /) is not accepted.
            format!("alice:{id}:{}", BASE64_STANDARD.encode([0xfb, 0xff])),
            format!("alice:{id}=:{key}"),
        ] {
            assert!(
                CsbWebauthnConfig::parse(TEST_ORIGIN, Some(&users)).is_err(),
                "{users}"
            );
        }
    }
}
