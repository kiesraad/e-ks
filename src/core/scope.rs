//! Authorization scope for sessions and streams.
//!
//! A scope distinguishes the kind of entity a session (and the streams it can
//! reach) belongs to. A political group only ever sees its own stream; the
//! central electoral committee (in Dutch: *centraal stembureau*, CSB) sees all
//! streams scoped to the committee.

use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The kind of entity a session or stream belongs to.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// A political group participating in an election. The default scope: a
    /// political group only has access to the single stream derived from its
    /// own identifier.
    #[default]
    PoliticalGroup,
    /// The central electoral committee (CSB). A committee member has access to
    /// every stream scoped to `ImportedByCsb`.
    CentralElectoralCommittee,
    /// A candidate-list package that was imported by the CSB from a political
    /// group's stream. One stream per import action.
    ImportedByCsb,
    /// A candidate-list package a political group handed in ahead of
    /// nomination day for the pre-submission check (*voorinlevering*), imported
    /// by the CSB to check it against the BRP. One stream per import action,
    /// kept apart from the [`Scope::ImportedByCsb`] streams of the
    /// examination.
    PreSubmittedToCsb,
}

impl Scope {
    /// Stable string representation persisted as a TEXT column in the database.
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::PoliticalGroup => "political_group",
            Scope::CentralElectoralCommittee => "central_electoral_committee",
            Scope::ImportedByCsb => "imported_by_csb",
            Scope::PreSubmittedToCsb => "pre_submitted_to_csb",
        }
    }
}

impl FromStr for Scope {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "political_group" => Ok(Scope::PoliticalGroup),
            "central_electoral_committee" => Ok(Scope::CentralElectoralCommittee),
            "imported_by_csb" => Ok(Scope::ImportedByCsb),
            "pre_submitted_to_csb" => Ok(Scope::PreSubmittedToCsb),
            _ => Err("invalid scope"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_political_group() {
        assert_eq!(Scope::default(), Scope::PoliticalGroup);
    }

    #[test]
    fn as_str_and_from_str_round_trip() {
        for scope in [
            Scope::PoliticalGroup,
            Scope::CentralElectoralCommittee,
            Scope::ImportedByCsb,
            Scope::PreSubmittedToCsb,
        ] {
            assert_eq!(Scope::from_str(scope.as_str()), Ok(scope));
        }
    }

    #[test]
    fn from_str_rejects_unknown() {
        assert!(Scope::from_str("something_else").is_err());
    }
}
