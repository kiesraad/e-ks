//! Passkey (WebAuthn) login for committee members.
//!
//! A committee member registers one or more passkeys under a passkey
//! *account* (the WebAuthn user handle) from an existing CSB session. The
//! account name is what they type at login, so the server can hand the browser
//! the matching credential ids: username-first, which keeps hardware keys
//! without resident storage working. Accounts and passkeys live in
//! [`CsbPasskeyStore`], the in-flight ceremony state travels in an encrypted
//! cookie ([`state_cookie`]), and [`PasskeyLogin`] bundles both with the
//! configured relying party.

mod login;
mod state_cookie;
mod store;

#[cfg(feature = "database")]
mod db;

#[cfg(test)]
pub(crate) mod test_support;

use std::{fmt::Display, str::FromStr};

use hkdf::Hkdf;
use secrecy::{ExposeSecret, SecretString, zeroize::Zeroizing};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::form::ValidationError;

pub use login::{
    MAX_PASSKEYS_PER_ACCOUNT, PasskeyLogin, pending_login_id, pending_register_id,
    require_passkey_login, user_error,
};
#[cfg(test)]
pub use state_cookie::STATE_COOKIE_NAME;
pub use state_cookie::{
    LoginCeremony, PasskeyStateCipher, Purpose, RegisterCeremony, RegisterTarget,
    build_state_removal_cookie,
};
pub use store::{CsbPasskeyStore, PasskeyAccount, StoredPasskey};

crate::id_newtype!(
    /// A passkey account: the WebAuthn user handle one or more passkeys
    /// belong to.
    pub struct PasskeyAccountId
);

crate::id_newtype!(
    /// A registered passkey, as addressed in routes.
    pub struct PasskeyId
);

/// A 256-bit key derived from the master secret with HKDF-SHA256, under a
/// salt and info of its own so it is unrelated to the stream keys.
fn derive_key(master: &SecretString, salt: &[u8], info: &[u8]) -> Zeroizing<[u8; 32]> {
    let hk = Hkdf::<Sha256>::new(Some(salt), master.expose_secret().as_bytes());
    let mut key = Zeroizing::new([0u8; 32]);
    hk.expand(info, key.as_mut())
        .expect("32 bytes is within HKDF-SHA256 output limit");
    key
}

const NAME_MIN_LEN: usize = 2;
const NAME_MAX_LEN: usize = 64;
const LABEL_MAX_LEN: usize = 64;

/// The name a committee member types at login and that the audit log shows
/// for their passkey sessions. Unique per deployment, compared case-insensitively.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PasskeyAccountName(String);

impl PasskeyAccountName {
    /// Case-folded form used for uniqueness: names differing only in case
    /// are the same account.
    pub fn normalized(&self) -> String {
        self.0.to_lowercase()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for PasskeyAccountName {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        let len = value.chars().count();
        if len < NAME_MIN_LEN {
            return Err(ValidationError::ValueTooShort(len, NAME_MIN_LEN));
        }
        if len > NAME_MAX_LEN {
            return Err(ValidationError::ValueTooLong(len, NAME_MAX_LEN));
        }
        let allowed =
            |c: char| c.is_alphanumeric() || matches!(c, ' ' | '.' | '-' | '_' | '@' | '\'');
        if !value.chars().all(allowed) {
            return Err(ValidationError::InvalidValue);
        }
        Ok(Self(value.to_string()))
    }
}

impl TryFrom<String> for PasskeyAccountName {
    type Error = ValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<PasskeyAccountName> for String {
    fn from(name: PasskeyAccountName) -> Self {
        name.0
    }
}

impl Display for PasskeyAccountName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// What a member calls one of their passkeys ("YubiKey", "iPhone"), so the
/// right one can be revoked later.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PasskeyLabel(String);

impl PasskeyLabel {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for PasskeyLabel {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        let len = value.chars().count();
        if len == 0 {
            return Err(ValidationError::ValueShouldNotBeEmpty);
        }
        if len > LABEL_MAX_LEN {
            return Err(ValidationError::ValueTooLong(len, LABEL_MAX_LEN));
        }
        if value.chars().any(char::is_control) {
            return Err(ValidationError::InvalidValue);
        }
        Ok(Self(value.to_string()))
    }
}

impl TryFrom<String> for PasskeyLabel {
    type Error = ValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<PasskeyLabel> for String {
    fn from(label: PasskeyLabel) -> Self {
        label.0
    }
}

impl Display for PasskeyLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_name_is_trimmed_and_bounded() {
        let name: PasskeyAccountName = "  Jan de Vries ".parse().expect("valid name");
        assert_eq!(name.as_str(), "Jan de Vries");
        assert_eq!(name.normalized(), "jan de vries");
        assert_eq!(name.to_string(), "Jan de Vries");

        assert!("J".parse::<PasskeyAccountName>().is_err());
        assert!("".parse::<PasskeyAccountName>().is_err());
        assert!("x".repeat(65).parse::<PasskeyAccountName>().is_err());
        assert!("x".repeat(64).parse::<PasskeyAccountName>().is_ok());
    }

    #[test]
    fn account_name_rejects_control_and_markup_characters() {
        assert!("jan\tde vries".parse::<PasskeyAccountName>().is_err());
        assert!("<jan>".parse::<PasskeyAccountName>().is_err());
        assert!("jan/vries".parse::<PasskeyAccountName>().is_err());
        assert!(
            "j.de-vries_1@kiesraad"
                .parse::<PasskeyAccountName>()
                .is_ok()
        );
        assert!("Zoë O'Neill".parse::<PasskeyAccountName>().is_ok());
    }

    #[test]
    fn label_is_trimmed_and_bounded() {
        let label: PasskeyLabel = " YubiKey 5 ".parse().expect("valid label");
        assert_eq!(label.as_str(), "YubiKey 5");

        assert!("   ".parse::<PasskeyLabel>().is_err());
        assert!("a\nb".parse::<PasskeyLabel>().is_err());
        assert!("x".repeat(65).parse::<PasskeyLabel>().is_err());
    }

    #[test]
    fn newtypes_serde_as_plain_strings() {
        let name: PasskeyAccountName = "Jan".parse().expect("valid name");
        assert_eq!(serde_json::to_string(&name).unwrap(), "\"Jan\"");
        let back: PasskeyAccountName = serde_json::from_str("\"  Jan \"").unwrap();
        assert_eq!(back, name);
        assert!(serde_json::from_str::<PasskeyAccountName>("\"J\"").is_err());

        let label: PasskeyLabel = "Key".parse().expect("valid label");
        assert_eq!(serde_json::to_string(&label).unwrap(), "\"Key\"");
        assert!(serde_json::from_str::<PasskeyLabel>("\"\"").is_err());
    }
}
