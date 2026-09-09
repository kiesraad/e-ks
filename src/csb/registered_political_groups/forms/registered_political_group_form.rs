use serde::Deserialize;
use validate::Validate;

use crate::structs::{
    common::Appellation,
    csb::{RegisteredPoliticalGroup, SeatCount, VoteCount},
};

/// Form backing the add and edit dialogs of a registered political group.
#[derive(Default, Deserialize, Debug, Validate)]
#[validate(target = "RegisteredPoliticalGroup")]
#[serde(default)]
pub struct RegisteredPoliticalGroupForm {
    #[validate(parse = "Appellation")]
    pub appellation: String,
    #[validate(parse = "VoteCount")]
    pub previous_votes: String,
    #[validate(parse = "SeatCount")]
    pub previous_seats: String,
}

impl From<RegisteredPoliticalGroup> for RegisteredPoliticalGroupForm {
    fn from(group: RegisteredPoliticalGroup) -> Self {
        RegisteredPoliticalGroupForm {
            appellation: group.appellation.to_string(),
            previous_votes: group.previous_votes.to_string(),
            previous_seats: group.previous_seats.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structs::csb::sample_registered_political_group;

    fn form(appellation: &str, votes: &str, seats: &str) -> RegisteredPoliticalGroupForm {
        RegisteredPoliticalGroupForm {
            appellation: appellation.to_string(),
            previous_votes: votes.to_string(),
            previous_seats: seats.to_string(),
        }
    }

    #[test]
    fn valid_form_creates_a_group_with_a_fresh_id() {
        let group = form(" De Partij ", "12345", "3")
            .validate_create()
            .expect("valid form");
        assert_eq!(group.appellation.to_string(), "De Partij");
        assert_eq!(group.previous_votes.value(), 12345);
        assert_eq!(group.previous_seats.value(), 3);
    }

    #[test]
    fn update_keeps_the_id_of_the_current_group() {
        let current = sample_registered_political_group("Oud", 1, 1);
        let updated = form("Nieuw", "2", "0")
            .validate_update(&current)
            .expect("valid form");
        assert_eq!(updated.id, current.id);
        assert_eq!(updated.appellation.to_string(), "Nieuw");
        assert!(!updated.is_numbered_on_votes());
    }

    #[test]
    fn every_invalid_field_is_reported() {
        let errors = form("", "twaalf", "-1")
            .validate_create()
            .expect_err("invalid form")
            .errors();
        let fields: Vec<_> = errors.iter().map(|(field, _)| field.as_str()).collect();
        assert_eq!(fields, ["appellation", "previous_votes", "previous_seats"]);
    }

    #[test]
    fn form_round_trips_a_group() {
        let group = sample_registered_political_group("Partij", 98765, 4);
        let form = RegisteredPoliticalGroupForm::from(group);
        assert_eq!(form.appellation, "Partij");
        assert_eq!(form.previous_votes, "98765");
        assert_eq!(form.previous_seats, "4");
    }
}
