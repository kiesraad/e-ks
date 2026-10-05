//! The authenticated identity behind a CSB (committee) session.

use serde::{Deserialize, Serialize};

use crate::{GithubUserId, Locale, PasskeyAccountId, PasskeyAccountName, trans};

/// The committee member behind a CSB session, recorded on every CSB event so
/// the audit log can show who triggered it.
///
/// Deliberately an enum over login methods rather than a single id: future
/// login methods add a variant here, and the events referencing the user
/// stay unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CsbUser {
    /// Dev-login bypass, with no identity beyond the login method itself.
    #[cfg(any(feature = "dev-features", test))]
    Developer,
    /// GitHub OAuth login, identified by the account's numeric id.
    Github { user_id: GithubUserId },
    /// Passkey (WebAuthn) login, identified by the passkey account the
    /// credential belongs to; the name is kept so the audit log reads well.
    Passkey {
        account_id: PasskeyAccountId,
        name: PasskeyAccountName,
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
            CsbUser::Github { user_id } => {
                format!("{} {user_id}", trans!("audit_log.user.github", locale))
            }
            CsbUser::Passkey { name, .. } => {
                format!("{} {name}", trans!("audit_log.user.passkey", locale))
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
        };
        assert_eq!(github.describe(Locale::En), "GitHub user 583231");
        assert_eq!(github.describe(Locale::Nl), "GitHub-gebruiker 583231");

        assert_eq!(CsbUser::Developer.describe(Locale::En), "Developer");

        let passkey = CsbUser::Passkey {
            account_id: PasskeyAccountId::new(),
            name: "Jan de Vries".parse().expect("valid name"),
        };
        assert_eq!(passkey.describe(Locale::En), "Passkey account Jan de Vries");
        assert_eq!(passkey.describe(Locale::Nl), "Passkey-account Jan de Vries");
    }

    #[test]
    fn serde_roundtrips() {
        let user = CsbUser::Github {
            user_id: "42".parse().expect("valid id"),
        };
        let json = serde_json::to_string(&user).expect("serialize");
        let back: CsbUser = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, user);

        let user = CsbUser::Passkey {
            account_id: PasskeyAccountId::new(),
            name: "Jan de Vries".parse().expect("valid name"),
        };
        let json = serde_json::to_string(&user).expect("serialize");
        let back: CsbUser = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, user);
    }
}
