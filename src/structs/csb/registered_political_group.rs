//! A political group as registered by the central electoral committee for the
//! numbering of the candidate lists (Kieswet Art. I 14): its registered
//! appellation and its result at the previous election of the same body.
//!
//! The lists of political groups that obtained one or more seats at that
//! election are numbered first, in the order of the number of votes cast on
//! their lists. The remaining lists are numbered by lot (Art. I 15).

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{form::ValidationError, id_newtype, structs::common::Appellation};

id_newtype!(pub struct RegisteredPoliticalGroupId);

/// Define non-negative integer newtypes parsed from a form field: trimmed,
/// digits only.
macro_rules! count_newtypes {
    ($($(#[$meta:meta])* $vis:vis struct $name:ident($int:ty);)*) => {
        $(
            $(#[$meta])*
            #[derive(
                Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
                Serialize, Deserialize,
            )]
            #[serde(transparent)]
            $vis struct $name($int);

            impl $name {
                pub fn value(self) -> $int {
                    self.0
                }
            }

            impl From<$int> for $name {
                fn from(value: $int) -> Self {
                    Self(value)
                }
            }

            impl FromStr for $name {
                type Err = ValidationError;

                fn from_str(value: &str) -> Result<Self, Self::Err> {
                    let trimmed = value.trim();
                    if trimmed.is_empty() {
                        return Err(ValidationError::ValueShouldNotBeEmpty);
                    }
                    if !trimmed.bytes().all(|b| b.is_ascii_digit()) {
                        return Err(ValidationError::InvalidValue);
                    }
                    trimmed
                        .parse::<$int>()
                        .map(Self)
                        .map_err(|_| ValidationError::InvalidValue)
                }
            }

            impl std::fmt::Display for $name {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    self.0.fmt(f)
                }
            }
        )*
    };
}

count_newtypes! {
    /// Number of votes cast on a political group's lists at an election.
    pub struct VoteCount(u64);
    /// Number of seats a political group obtained at an election.
    pub struct SeatCount(u32);
}

/// A registered political group with its result at the previous election of
/// the same representative body, recorded per election on the CSB main stream.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisteredPoliticalGroup {
    pub id: RegisteredPoliticalGroupId,
    /// The appellation registered with the central electoral committee.
    pub appellation: Appellation,
    /// Votes cast on the group's lists at the previous election.
    pub previous_votes: VoteCount,
    /// Seats the group obtained at the previous election.
    pub previous_seats: SeatCount,
}

impl RegisteredPoliticalGroup {
    /// Whether the group's list is numbered on its previous votes (Kieswet
    /// Art. I 14): only groups that obtained one or more seats are.
    pub fn is_numbered_on_votes(&self) -> bool {
        self.previous_seats.value() > 0
    }

    /// Whether `appellation` is this group's, ignoring case.
    pub fn has_appellation(&self, appellation: &Appellation) -> bool {
        self.appellation.to_lowercase() == appellation.to_lowercase()
    }

    /// The order the lists are numbered in on votes: most votes first, ties
    /// (decided by lot during the session) alphabetically for a stable listing.
    pub fn numbering_order(&self, other: &Self) -> std::cmp::Ordering {
        other
            .previous_votes
            .cmp(&self.previous_votes)
            .then_with(|| self.appellation.cmp(&other.appellation))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn sample_registered_political_group(
        appellation: &str,
        votes: u64,
        seats: u32,
    ) -> RegisteredPoliticalGroup {
        RegisteredPoliticalGroup {
            id: RegisteredPoliticalGroupId::new(),
            appellation: appellation.parse().expect("valid appellation"),
            previous_votes: VoteCount::from(votes),
            previous_seats: SeatCount::from(seats),
        }
    }

    #[test]
    fn counts_parse_trimmed_digits_only() {
        assert_eq!(VoteCount::from_str(" 12345 "), Ok(VoteCount(12345)));
        assert_eq!(SeatCount::from_str("0"), Ok(SeatCount(0)));
        assert_eq!(
            VoteCount::from_str(""),
            Err(ValidationError::ValueShouldNotBeEmpty)
        );
        assert_eq!(
            VoteCount::from_str("   "),
            Err(ValidationError::ValueShouldNotBeEmpty)
        );
        assert_eq!(
            VoteCount::from_str("-1"),
            Err(ValidationError::InvalidValue)
        );
        assert_eq!(
            VoteCount::from_str("+1"),
            Err(ValidationError::InvalidValue)
        );
        assert_eq!(
            VoteCount::from_str("1.000"),
            Err(ValidationError::InvalidValue)
        );
        assert_eq!(
            SeatCount::from_str("99999999999"),
            Err(ValidationError::InvalidValue)
        );
    }

    #[test]
    fn counts_display_and_serialize_transparently() {
        assert_eq!(VoteCount(1234).to_string(), "1234");
        assert_eq!(serde_json::to_string(&SeatCount(3)).unwrap(), "3");
        let votes: VoteCount = serde_json::from_str("42").unwrap();
        assert_eq!(votes.value(), 42);
    }

    #[test]
    fn only_seated_groups_are_numbered_on_votes() {
        assert!(sample_registered_political_group("A", 1000, 1).is_numbered_on_votes());
        assert!(!sample_registered_political_group("B", 1000, 0).is_numbered_on_votes());
    }

    #[test]
    fn appellations_match_ignoring_case() {
        let group = sample_registered_political_group("De Tegen Partij", 1, 1);
        assert!(group.has_appellation(&"de tegen partij".parse().unwrap()));
        assert!(!group.has_appellation(&"De Voor Partij".parse().unwrap()));
    }

    #[test]
    fn numbering_order_is_most_votes_first_then_alphabetical() {
        let mut groups = [
            sample_registered_political_group("Beta", 100, 1),
            sample_registered_political_group("Alpha", 100, 1),
            sample_registered_political_group("Gamma", 300, 2),
        ];
        groups.sort_by(RegisteredPoliticalGroup::numbering_order);
        let names: Vec<_> = groups.iter().map(|g| g.appellation.to_string()).collect();
        assert_eq!(names, ["Gamma", "Alpha", "Beta"]);
    }
}
