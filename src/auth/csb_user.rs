//! The authenticated identity behind a CSB (committee) session.

use serde::{Deserialize, Serialize};

use crate::{CsbUsername, Locale, trans};

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
    /// Security-key (WebAuthn) login, identified by the configured username.
    SecurityKey { username: CsbUsername },
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
            CsbUser::SecurityKey { username } => {
                format!(
                    "{} {username}",
                    trans!("audit_log.user.committee_member", locale)
                )
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
        let member = CsbUser::SecurityKey {
            username: "alice".parse().expect("valid username"),
        };
        assert_eq!(member.describe(Locale::En), "Committee member alice");
        assert_eq!(member.describe(Locale::Nl), "CSB-lid alice");

        assert_eq!(CsbUser::Developer.describe(Locale::En), "Developer");
    }

    #[test]
    fn serde_roundtrips() {
        let user = CsbUser::SecurityKey {
            username: "alice".parse().expect("valid username"),
        };
        let json = serde_json::to_string(&user).expect("serialize");
        let back: CsbUser = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, user);
    }
}
