//! A political group as registered by the central electoral committee for the
//! numbering of the candidate lists (Kieswet Art. I 14): its registered
//! appellation and its result at the previous election of the same body.
//!
//! The lists of political groups that obtained one or more seats at that
//! election are numbered first, in the order of the number of votes cast on
//! their lists. The remaining lists are numbered by lot (Art. I 15). A
//! combined list is numbered on the registrations of its parties together,
//! see [`PreviousResult`].

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{
    form::ValidationError,
    id_newtype,
    structs::{
        audit_log::{AuditLeaf, AuditValue, audit_fields},
        common::{Appellation, LegalName, PreviousElectionResults},
    },
};

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

            impl AuditLeaf for $name {
                fn audit_value(&self) -> AuditValue {
                    AuditValue::text(self.0)
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
    /// The group's statutory name, as recorded in its articles of association
    /// and at the chamber of commerce (Kieswet Art. G 1, eerste lid).
    #[serde(default)]
    pub legal_name: LegalName,
    /// Votes cast on the group's lists at the previous election.
    pub previous_votes: VoteCount,
    /// Seats the group obtained at the previous election.
    pub previous_seats: SeatCount,
}

audit_fields!(RegisteredPoliticalGroup {
    id: skip,
    appellation: leaf(Appellation),
    legal_name: leaf(LegalName),
    previous_votes: leaf(PreviousVotes),
    previous_seats: leaf(PreviousSeats),
});

impl RegisteredPoliticalGroup {
    /// Whether the group obtained one or more seats at the previous election.
    pub fn was_previously_seated(&self) -> bool {
        self.previous_seats.value() > 0
    }

    /// Whether the group's list is numbered on its previous votes (Kieswet
    /// Art. I 14): only groups that obtained one or more seats are.
    pub fn is_numbered_on_votes(&self) -> bool {
        self.was_previously_seated()
    }

    /// The previous election result as the political group answers it on its
    /// general information: the bracket its seat count falls in.
    pub fn previous_election_results(&self) -> PreviousElectionResults {
        match self.previous_seats.value() {
            0 => PreviousElectionResults::ZeroSeats,
            1..=15 => PreviousElectionResults::OneToFifteenSeats,
            _ => PreviousElectionResults::SixteenOrMoreSeats,
        }
    }

    /// Whether `appellation` is this group's, ignoring case.
    pub fn has_appellation(&self, appellation: &Appellation) -> bool {
        self.appellation.to_lowercase() == appellation.to_lowercase()
    }

    /// Whether `legal_name` is this group's statutory name, ignoring case. A
    /// registration without a legal name matches none.
    pub fn has_legal_name(&self, legal_name: &LegalName) -> bool {
        !self.legal_name.is_empty() && self.legal_name.to_lowercase() == legal_name.to_lowercase()
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

/// The result at the previous election a candidate list is numbered on
/// (Kieswet Art. I 14) and exempted from declarations of support on (Art. H 4,
/// achtste lid): that of the list's registration, or for a combined list that
/// of the registrations of its parties together.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PreviousResult {
    /// The votes the list is numbered on. For a combined list these are the
    /// votes of the parties that obtained seats added up (Art. I 14, tweede
    /// lid); a combination without a seat, numbered by lot, carries the votes
    /// of all its parties for the record.
    pub votes: VoteCount,
    /// The seats the parties behind the list obtained together.
    pub seats: SeatCount,
}

impl PreviousResult {
    /// The result of the parties behind one list together; `None` without a
    /// registration.
    pub fn combined<'a>(
        registrations: impl IntoIterator<Item = &'a RegisteredPoliticalGroup>,
    ) -> Option<Self> {
        let registrations: Vec<_> = registrations.into_iter().collect();
        if registrations.is_empty() {
            return None;
        }
        let seats: u32 = registrations.iter().map(|r| r.previous_seats.value()).sum();
        let votes: u64 = registrations
            .iter()
            .filter(|r| seats == 0 || r.was_previously_seated())
            .map(|r| r.previous_votes.value())
            .sum();
        Some(Self {
            votes: VoteCount::from(votes),
            seats: SeatCount::from(seats),
        })
    }

    /// Whether the parties behind the list obtained one or more seats, which
    /// numbers the list on votes and exempts it from declarations of support.
    pub fn is_seated(self) -> bool {
        self.seats.value() > 0
    }
}

impl From<&RegisteredPoliticalGroup> for PreviousResult {
    fn from(registration: &RegisteredPoliticalGroup) -> Self {
        Self {
            votes: registration.previous_votes,
            seats: registration.previous_seats,
        }
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
            legal_name: format!("Vereniging {appellation}")
                .parse()
                .expect("valid legal name"),
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
    fn a_group_with_a_seat_was_previously_seated() {
        assert!(sample_registered_political_group("A", 1000, 1).was_previously_seated());
        assert!(!sample_registered_political_group("B", 1000, 0).was_previously_seated());
    }

    #[test]
    fn previous_election_results_follow_the_seat_brackets() {
        let results =
            |seats| sample_registered_political_group("A", 1, seats).previous_election_results();
        assert_eq!(results(0), PreviousElectionResults::ZeroSeats);
        assert_eq!(results(1), PreviousElectionResults::OneToFifteenSeats);
        assert_eq!(results(15), PreviousElectionResults::OneToFifteenSeats);
        assert_eq!(results(16), PreviousElectionResults::SixteenOrMoreSeats);
    }

    #[test]
    fn a_registration_without_a_legal_name_still_deserialises() {
        let group: RegisteredPoliticalGroup = serde_json::from_value(serde_json::json!({
            "id": uuid::Uuid::nil(),
            "appellation": "Partij",
            "previous_votes": 1,
            "previous_seats": 1,
        }))
        .unwrap();
        assert_eq!(group.legal_name.to_string(), "");
    }

    #[test]
    fn appellations_match_ignoring_case() {
        let group = sample_registered_political_group("De Tegen Partij", 1, 1);
        assert!(group.has_appellation(&"de tegen partij".parse().unwrap()));
        assert!(!group.has_appellation(&"De Voor Partij".parse().unwrap()));
    }

    #[test]
    fn legal_names_match_ignoring_case() {
        let group = sample_registered_political_group("De Tegen Partij", 1, 1);
        assert!(group.has_legal_name(&"vereniging de tegen partij".parse().unwrap()));
        assert!(!group.has_legal_name(&"Vereniging De Voor Partij".parse().unwrap()));

        let without = RegisteredPoliticalGroup {
            legal_name: LegalName::default(),
            ..group
        };
        assert!(!without.has_legal_name(&"Vereniging De Tegen Partij".parse().unwrap()));
    }

    #[test]
    fn a_combined_result_counts_the_votes_of_the_seated_parties_only() {
        let a = sample_registered_political_group("A", 100, 0);
        let b = sample_registered_political_group("B", 50, 2);
        let c = sample_registered_political_group("C", 30, 1);
        let result = |registrations: &[&RegisteredPoliticalGroup]| {
            PreviousResult::combined(registrations.iter().copied())
        };

        assert_eq!(result(&[]), None);
        assert_eq!(result(&[&a]), Some(PreviousResult::from(&a)));
        assert_eq!(
            PreviousResult::from(&a),
            PreviousResult {
                votes: VoteCount(100),
                seats: SeatCount(0)
            }
        );
        // Both seated: votes added up.
        assert_eq!(
            result(&[&b, &c]),
            Some(PreviousResult {
                votes: VoteCount(80),
                seats: SeatCount(3)
            })
        );
        // The party with the most votes has no seat: its votes do not count,
        // the combination is still seated.
        assert_eq!(
            result(&[&a, &b]),
            Some(PreviousResult {
                votes: VoteCount(50),
                seats: SeatCount(2)
            })
        );
        assert!(result(&[&a, &b]).unwrap().is_seated());
        // No party seated: numbered by lot, all votes kept for the record.
        let d = sample_registered_political_group("D", 10, 0);
        assert_eq!(
            result(&[&a, &d]),
            Some(PreviousResult {
                votes: VoteCount(110),
                seats: SeatCount(0)
            })
        );
        assert!(!result(&[&a, &d]).unwrap().is_seated());
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
