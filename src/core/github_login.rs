//! Constrained newtype for a GitHub login name.

use std::{fmt::Display, str::FromStr};

use serde::{Deserialize, Serialize};

/// A GitHub login: one to 39 characters, letters, digits and hyphens, with
/// no hyphen at either end and none doubled, as GitHub itself requires.
///
/// Recorded next to the stable numeric id purely so the audit log reads well.
/// A login can be released and re-registered by someone else, so it never
/// identifies anyone on its own and the allowlist stays expressed in ids.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct GithubLogin(String);

const MAX_LENGTH: usize = 39;

impl TryFrom<String> for GithubLogin {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let valid_chars = value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        if value.is_empty()
            || value.len() > MAX_LENGTH
            || !valid_chars
            || value.starts_with('-')
            || value.ends_with('-')
            || value.contains("--")
        {
            return Err(format!("invalid GitHub login: {value:?}"));
        }
        Ok(Self(value))
    }
}

impl From<GithubLogin> for String {
    fn from(login: GithubLogin) -> Self {
        login.0
    }
}

impl FromStr for GithubLogin {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_from(value.to_string())
    }
}

impl Display for GithubLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_github_logins() {
        for login in ["octocat", "a", "mona-lisa", "User123", &"x".repeat(39)] {
            assert!(login.parse::<GithubLogin>().is_ok(), "{login}");
        }
        assert_eq!(
            "octocat".parse::<GithubLogin>().unwrap().to_string(),
            "octocat"
        );
    }

    #[test]
    fn rejects_what_github_rejects() {
        for login in [
            "",
            "-octocat",
            "octocat-",
            "mona--lisa",
            "mona lisa",
            "mona_lisa",
            "octo/cat",
            &"x".repeat(40),
        ] {
            assert!(login.parse::<GithubLogin>().is_err(), "{login:?}");
        }
    }

    #[test]
    fn serde_roundtrips_as_a_string_and_validates_on_read() {
        let login: GithubLogin = "octocat".parse().unwrap();
        assert_eq!(serde_json::to_string(&login).unwrap(), "\"octocat\"");
        let back: GithubLogin = serde_json::from_str("\"octocat\"").unwrap();
        assert_eq!(back, login);
        assert!(serde_json::from_str::<GithubLogin>("\"-nope\"").is_err());
    }
}
