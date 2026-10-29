use serde::Deserialize;
use validate::Validate;

use crate::{
    OptionStringExt,
    structs::{
        common::{Appellation, PreviousElectionResults},
        csb::RegisteredPoliticalGroup,
        political_groups::PoliticalGroup,
    },
};

#[derive(Default, Deserialize, Debug, Validate)]
#[validate(target = "PoliticalGroup")]
#[serde(default)]
pub struct PoliticalGroupForm {
    /// The id of the committee registration picked, empty when the group
    /// enters its information by hand. Bound to the group by the handler,
    /// which checks the id against the committee's registrations.
    #[validate(ignore)]
    pub registered_political_group: String,
    #[validate(parse = "PreviousElectionResults", optional)]
    pub previous_election_results: String,
    #[validate(parse = "Appellation", optional)]
    pub appellation: String,
}

impl PoliticalGroupForm {
    /// Take the values a registration decides over from `registration`: the
    /// fields are read-only once a registration is picked, so what was
    /// submitted for them is not trusted.
    pub fn with_registration(mut self, registration: &RegisteredPoliticalGroup) -> Self {
        self.registered_political_group = registration.id.to_string();
        self.appellation = registration.appellation.to_string();
        self.previous_election_results = registration.previous_election_results().to_string();
        self
    }
}

impl From<PoliticalGroup> for PoliticalGroupForm {
    fn from(value: PoliticalGroup) -> Self {
        PoliticalGroupForm {
            registered_political_group: String::new(),
            previous_election_results: value
                .previous_election_results
                .map(|r| r.to_string())
                .unwrap_or_default(),
            appellation: value.appellation.to_string_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structs::csb::sample_registered_political_group;

    #[test]
    fn a_registration_overrides_the_submitted_values() {
        let registration = sample_registered_political_group("De Partij", 1000, 16);
        let form = PoliticalGroupForm {
            registered_political_group: "tampered".to_string(),
            previous_election_results: "zero_seats".to_string(),
            appellation: "Andere Partij".to_string(),
        }
        .with_registration(&registration);

        assert_eq!(form.registered_political_group, registration.id.to_string());
        assert_eq!(form.appellation, "De Partij");
        assert_eq!(form.previous_election_results, "sixteen_or_more_seats");

        let group = form.validate_create().expect("valid form");
        assert_eq!(group.appellation.unwrap().to_string(), "De Partij");
        assert_eq!(
            group.previous_election_results,
            Some(PreviousElectionResults::SixteenOrMoreSeats)
        );
    }
}
