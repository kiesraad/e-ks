//! Constrained newtype for the username of a committee member.

use std::{fmt::Display, str::FromStr};

use serde::{Deserialize, Serialize};

/// Username of a committee member, as configured in `CSB_WEBAUTHN_USERS` and
/// recorded on every CSB event. One to 64 ASCII letters, digits, dots,
/// hyphens, underscores or `@`, so a name survives the comma-separated
/// environment variable, a form field and the audit log without escaping.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CsbUsername(String);

const MAX_LEN: usize = 64;

impl CsbUsername {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CsbUsername {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() || value.len() > MAX_LEN {
            return Err(format!("username must be 1 to {MAX_LEN} characters"));
        }
        if !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'@'))
        {
            return Err(format!(
                "username {value:?} may only contain ASCII letters, digits, '.', '-', '_' or '@'"
            ));
        }
        Ok(Self(value))
    }
}

impl From<CsbUsername> for String {
    fn from(username: CsbUsername) -> Self {
        username.0
    }
}

impl FromStr for CsbUsername {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_string().try_into()
    }
}

impl Display for CsbUsername {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_names() {
        for name in ["alice", "a.de-vries_2", "voorzitter@kiesraad.nl", "A"] {
            let username: CsbUsername = name.parse().expect("valid username");
            assert_eq!(username.to_string(), name);
        }
        assert!("a".repeat(MAX_LEN).parse::<CsbUsername>().is_ok());
    }

    #[test]
    fn rejects_empty_long_and_unusual_names() {
        assert!("".parse::<CsbUsername>().is_err());
        assert!("a".repeat(MAX_LEN + 1).parse::<CsbUsername>().is_err());
        for name in ["alice,bob", "alice:key", "al ice", "ali<ce", "élise"] {
            assert!(name.parse::<CsbUsername>().is_err(), "{name}");
        }
    }

    #[test]
    fn serde_roundtrips_as_plain_string() {
        let username: CsbUsername = "alice".parse().expect("valid username");
        let json = serde_json::to_string(&username).expect("serialize");
        assert_eq!(json, "\"alice\"");
        let back: CsbUsername = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, username);
        assert!(serde_json::from_str::<CsbUsername>("\"a b\"").is_err());
    }
}
