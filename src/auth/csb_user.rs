//! The authenticated identity behind a CSB (committee) session.

use serde::{Deserialize, Serialize};

use crate::{GithubLogin, GithubUserId, Locale, trans};

/// The committee member behind a CSB session, recorded on every CSB event so
/// the audit log can show who triggered it.
///
/// Deliberately an enum over login methods rather than a single id: future
/// login methods add a variant here, and the events referencing the user
/// stay unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CsbUser {
    /// Dev-login bypass, with no identity beyond the login method itself.
    #[cfg(any(feature = "dev-features", test))]
    Developer,
    /// GitHub OAuth login, identified by the account's numeric id.
    Github {
        user_id: GithubUserId,
        /// The account's login when they signed in, so the audit log reads
        /// well; absent on events recorded before it was kept.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        login: Option<GithubLogin>,
    },
}

/// Implemented by the CSB store events, which all record the committee member
/// that triggered them. Lets the audit log render the user generically.
pub trait HasCsbUser {
    fn csb_user(&self) -> &CsbUser;
}

impl CsbUser {
    /// Human-readable label shown in the audit log.
    pub fn describe(&self, locale: Locale) -> String {
        match self {
            #[cfg(any(feature = "dev-features", test))]
            CsbUser::Developer => trans!("audit_log.user.developer", locale),
            CsbUser::Github {
                user_id,
                login: Some(login),
            } => {
                format!(
                    "{} {login} ({user_id})",
                    trans!("audit_log.user.github", locale)
                )
            }
            CsbUser::Github {
                user_id,
                login: None,
            } => {
                format!("{} {user_id}", trans!("audit_log.user.github", locale))
            }
        }
    }

    #[cfg(test)]
    pub fn new_test() -> Self {
        CsbUser::Developer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_shows_login_method_and_identity() {
        let github = CsbUser::Github {
            user_id: "583231".parse().expect("valid id"),
            login: None,
        };
        assert_eq!(github.describe(Locale::En), "GitHub user 583231");
        assert_eq!(github.describe(Locale::Nl), "GitHub-gebruiker 583231");

        let named = CsbUser::Github {
            user_id: "583231".parse().expect("valid id"),
            login: Some("octocat".parse().expect("valid login")),
        };
        assert_eq!(named.describe(Locale::En), "GitHub user octocat (583231)");
        assert_eq!(
            named.describe(Locale::Nl),
            "GitHub-gebruiker octocat (583231)"
        );

        assert_eq!(CsbUser::Developer.describe(Locale::En), "Developer");
    }

    #[test]
    fn serde_roundtrips() {
        let user = CsbUser::Github {
            user_id: "42".parse().expect("valid id"),
            login: Some("octocat".parse().expect("valid login")),
        };
        let json = serde_json::to_string(&user).expect("serialize");
        let back: CsbUser = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, user);
    }

    /// Events recorded before the login was kept carry the id alone.
    #[test]
    fn reads_a_user_recorded_without_a_login() {
        let back: CsbUser =
            serde_json::from_str(r#"{"Github":{"user_id":42}}"#).expect("deserialize");
        assert_eq!(
            back,
            CsbUser::Github {
                user_id: "42".parse().expect("valid id"),
                login: None,
            }
        );
    }
}
