//! The identity behind a session: one variant per role.

use serde::{Deserialize, Serialize};

use crate::{CsbUser, ElectionConfig, Scope, StreamId};

/// Who a session belongs to. One variant per role, carrying exactly the state
/// that role needs, so an incomplete or mixed-role session cannot be
/// represented. Role changes require establishing a new session (see
/// [`crate::auth::session_extractor::establish_session`]), never mutating an
/// existing one across variants.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionUser {
    /// A political group, logged in via SAML (DigiD/TVS) or the dev login.
    PoliticalGroup {
        /// Stream derived from the login identity; fixed for the session.
        stream_id: StreamId,
        /// SAML `NameID` from the authenticating Assertion, needed for the
        /// `LogoutRequest` (eID §7.7.1); a placeholder for dev logins.
        saml_name_id: String,
        /// Election picked at `/select-election`; `None` until then.
        election: Option<ElectionConfig>,
    },
    /// A member of the central electoral committee (CSB), logged in with a
    /// security key (WebAuthn) or the dev login.
    CentralElectoralCommittee {
        /// The committee member, recorded on every CSB event for the audit log.
        user: CsbUser,
        /// Fixed at login from the configured default election.
        election: ElectionConfig,
        /// CSB stream whose paper documents are being corrected. While set,
        /// app routes serve that stream's paper-corrected data.
        paper_correction_stream_id: Option<StreamId>,
    },
}

impl SessionUser {
    /// Session-side view of the stream classifier, for logging and checks.
    pub fn scope(&self) -> Scope {
        match self {
            Self::PoliticalGroup { .. } => Scope::PoliticalGroup,
            Self::CentralElectoralCommittee { .. } => Scope::CentralElectoralCommittee,
        }
    }

    /// The election this session works on, if one has been picked.
    pub fn election(&self) -> Option<ElectionConfig> {
        match self {
            Self::PoliticalGroup { election, .. } => *election,
            Self::CentralElectoralCommittee { election, .. } => Some(*election),
        }
    }

    #[cfg(any(feature = "database", test))]
    pub(crate) fn tag(&self) -> &'static str {
        match self {
            Self::PoliticalGroup { .. } => "PoliticalGroup",
            Self::CentralElectoralCommittee { .. } => "CentralElectoralCommittee",
        }
    }
}

/// Redacts the SAML `NameID`, which identifies a person and has no place in
/// logs (log lines print the session identity via `Debug`).
impl std::fmt::Debug for SessionUser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PoliticalGroup {
                stream_id,
                election,
                ..
            } => f
                .debug_struct("PoliticalGroup")
                .field("stream_id", stream_id)
                .field("saml_name_id", &"***")
                .field("election", election)
                .finish(),
            Self::CentralElectoralCommittee {
                user,
                election,
                paper_correction_stream_id,
            } => f
                .debug_struct("CentralElectoralCommittee")
                .field("user", user)
                .field("election", election)
                .field("paper_correction_stream_id", paper_correction_stream_id)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn political_group() -> SessionUser {
        SessionUser::PoliticalGroup {
            stream_id: StreamId::new(),
            saml_name_id: "name-id-xyz".to_string(),
            election: Some(ElectionConfig::EK27),
        }
    }

    fn committee() -> SessionUser {
        SessionUser::CentralElectoralCommittee {
            user: CsbUser::new_test(),
            election: ElectionConfig::EK27,
            paper_correction_stream_id: None,
        }
    }

    #[test]
    fn scope_matches_variant() {
        assert_eq!(political_group().scope(), Scope::PoliticalGroup);
        assert_eq!(committee().scope(), Scope::CentralElectoralCommittee);
    }

    #[test]
    fn election_is_optional_only_for_political_groups() {
        let mut user = political_group();
        assert_eq!(user.election(), Some(ElectionConfig::EK27));
        if let SessionUser::PoliticalGroup { election, .. } = &mut user {
            *election = None;
        }
        assert_eq!(user.election(), None);

        assert_eq!(committee().election(), Some(ElectionConfig::EK27));
    }

    /// The serde external tag must match `tag()`: the database same-role guard
    /// (`identity ? $tag`) relies on it.
    #[test]
    fn serde_external_tag_matches_tag() {
        for user in [political_group(), committee()] {
            let value = serde_json::to_value(&user).expect("serialize");
            let keys: Vec<&String> = value.as_object().expect("object").keys().collect();
            assert_eq!(keys, [user.tag()]);
        }
    }

    #[test]
    fn serde_roundtrips() {
        for user in [political_group(), committee()] {
            let json = serde_json::to_string(&user).expect("serialize");
            let back: SessionUser = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, user);
        }
    }

    #[test]
    fn debug_redacts_saml_name_id() {
        let debug = format!("{:?}", political_group());
        assert!(!debug.contains("name-id-xyz"));
        assert!(debug.contains("***"));
    }
}
