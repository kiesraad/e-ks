use serde::{Deserialize, Serialize};

use crate::{
    OptionAsStrExt,
    structs::common::{InfoProblems, PotentialProblems, Problematic, Problems, Severity},
    utils::bag,
};

use super::{
    CountryCode, HouseNumber, HouseNumberAddition, InternationalPostalCode, Locality, PostalCode,
    StateOrProvince, StreetName, problematic::EmptyAddressProblems,
};

/// A Dutch postal address with optional components.
///
/// An address is considered complete when the street name, house number,
/// postal code, and locality are present.
#[derive(Default, Debug, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct DutchAddress {
    /// Street name (e.g. "Hoofdstraat").
    pub street_name: Option<StreetName>,
    /// House number (e.g. "123").
    pub house_number: Option<HouseNumber>,
    /// House number addition (e.g. "A" or "A1A").
    pub house_number_addition: Option<HouseNumberAddition>,
    /// City or town.
    pub locality: Option<Locality>,
    /// Dutch postal code.
    pub postal_code: Option<PostalCode>,
    /// Address found in the BAG.
    pub known_in_bag: Option<bool>,
}

impl DutchAddress {
    /// Returns `true` when all address parts are empty or `None`.
    pub fn is_empty(&self) -> bool {
        self.street_name.is_empty_or_none()
            && self.house_number.is_empty_or_none()
            && self.house_number_addition.is_empty_or_none()
            && self.postal_code.is_empty_or_none()
            && self.locality.is_empty_or_none()
    }

    /// Recompute [`known_in_bag`](Self::known_in_bag) from the current fields.
    ///
    /// When all of postal code, house number, street name and locality are
    /// present, the address is looked up in the BAG and the field is set to
    /// whether it matches. If any of those fields is missing the address
    /// cannot be checked, so the field is reset to `None`.
    pub fn update_is_known_in_bag(&mut self) {
        match (
            &self.postal_code,
            &self.house_number,
            &self.street_name,
            &self.locality,
        ) {
            (Some(postal_code), Some(house_number), Some(street_name), Some(locality)) => {
                let is_known_in_bag = bag::address_exists(
                    &postal_code.to_string(),
                    house_number.as_number(),
                    &street_name.to_string(),
                    &locality.to_string(),
                );

                self.known_in_bag = Some(is_known_in_bag);
            }
            _ => {
                self.known_in_bag = None;
            }
        }
    }

    /// Returns `true` when the address has been checked against the BAG and was
    /// not found. A `None` (unchecked) address is not considered unknown.
    pub fn is_unknown(&self) -> bool {
        self.known_in_bag == Some(false)
    }
}

impl Problematic<Severity> for DutchAddress {
    fn get_problems(&self, severity: Severity) -> Problems {
        let mut potential_problems = Vec::new();
        let mut info_problems = Vec::new();
        let mut address_problems = Vec::new();

        if self.street_name.is_empty_or_none() {
            address_problems.push(EmptyAddressProblems::StreetName);
        }

        if self.house_number.is_empty_or_none() {
            address_problems.push(EmptyAddressProblems::HouseNumber);
        }

        if self.postal_code.is_empty_or_none() {
            address_problems.push(EmptyAddressProblems::PostalCode);
        }

        if self.locality.is_empty_or_none() {
            address_problems.push(EmptyAddressProblems::Locality);
        }

        if !address_problems.is_empty() {
            if severity == Severity::Info {
                info_problems.push(InfoProblems::IncompleteAddress {
                    problems: address_problems,
                });
            } else {
                potential_problems.push(PotentialProblems::IncompleteAddress {
                    severity,
                    problems: address_problems,
                });
            }
        }
        if self.known_in_bag == Some(false) {
            potential_problems.push(PotentialProblems::UnknownAddress);
        }

        Problems {
            potential_problems,
            info_problems,
        }
    }
}

/// A postal address outside the Netherlands.
///
/// An address is considered complete when the street name, house number,
/// postal code, locality, and country are present.
#[derive(Default, Debug, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct InternationalAddress {
    /// Street name (e.g. "Downing Street").
    pub street_name: Option<StreetName>,
    /// House number (e.g. "10").
    pub house_number: Option<HouseNumber>,
    /// House number addition.
    pub house_number_addition: Option<HouseNumberAddition>,
    /// City or town.
    pub locality: Option<Locality>,
    /// State, province, or region.
    pub state_or_province: Option<StateOrProvince>,
    /// International postal code with relaxed validation.
    pub postal_code: Option<InternationalPostalCode>,
    /// ISO 3166-1 alpha-2 country code.
    pub country: Option<CountryCode>,
}

impl InternationalAddress {
    /// Returns `true` when all address parts are empty or `None`.
    pub fn is_empty(&self) -> bool {
        self.street_name.is_empty_or_none()
            && self.house_number.is_empty_or_none()
            && self.house_number_addition.is_empty_or_none()
            && self.postal_code.is_empty_or_none()
            && self.locality.is_empty_or_none()
            && self.state_or_province.is_empty_or_none()
            && self.country.is_empty_or_none()
    }
}

impl Problematic<Severity> for InternationalAddress {
    fn get_problems(&self, severity: Severity) -> Problems {
        let mut problems = Vec::new();

        if self.street_name.is_empty_or_none() {
            problems.push(EmptyAddressProblems::StreetName);
        }
        if self.house_number.is_empty_or_none() {
            problems.push(EmptyAddressProblems::HouseNumber);
        }

        if self.postal_code.is_empty_or_none() {
            problems.push(EmptyAddressProblems::PostalCode);
        }

        if self.locality.is_empty_or_none() {
            problems.push(EmptyAddressProblems::Locality);
        }

        if self.country.is_empty_or_none() {
            problems.push(EmptyAddressProblems::Country);
        }

        Problems {
            potential_problems: if problems.is_empty() {
                Vec::new()
            } else {
                vec![PotentialProblems::IncompleteAddress { severity, problems }]
            },
            info_problems: Vec::new(),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub enum Address {
    Dutch(DutchAddress),
    International(InternationalAddress),
}

impl Default for Address {
    fn default() -> Self {
        Address::Dutch(DutchAddress::default())
    }
}

impl Address {
    /// Recompute the BAG match for a Dutch address. International addresses
    /// have no BAG equivalent, so this is a no-op for them.
    pub fn update_is_known_in_bag(&mut self) {
        if let Address::Dutch(address) = self {
            address.update_is_known_in_bag();
        }
    }

    /// Returns `true` for a Dutch address that was checked against the BAG and
    /// not found. International addresses have no BAG equivalent and are never
    /// considered unknown.
    pub fn is_unknown(&self) -> bool {
        match self {
            Address::Dutch(address) => address.is_unknown(),
            Address::International(_) => false,
        }
    }

    pub fn as_dutch(&self) -> Option<&DutchAddress> {
        match self {
            Address::Dutch(address) => Some(address),
            _ => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Address::Dutch(address) => address.is_empty(),
            Address::International(address) => address.is_empty(),
        }
    }

    pub fn postal_code(&self) -> Option<String> {
        match self {
            Address::Dutch(address) => address.postal_code.as_ref().map(ToString::to_string),
            Address::International(address) => {
                address.postal_code.as_ref().map(ToString::to_string)
            }
        }
    }

    pub fn locality(&self) -> Option<String> {
        match self {
            Address::Dutch(address) => address.locality.as_ref().map(ToString::to_string),
            Address::International(address) => address.locality.as_ref().map(ToString::to_string),
        }
    }

    pub fn country(&self) -> Option<String> {
        match self {
            Address::Dutch(_) => None,
            Address::International(address) => address.country.as_ref().map(ToString::to_string),
        }
    }

    pub fn state_or_province(&self) -> Option<String> {
        match self {
            Address::Dutch(_) => None,
            Address::International(address) => {
                address.state_or_province.as_ref().map(ToString::to_string)
            }
        }
    }

    pub fn street_name(&self) -> Option<String> {
        match self {
            Address::Dutch(address) => address.street_name.as_ref().map(ToString::to_string),
            Address::International(address) => {
                address.street_name.as_ref().map(ToString::to_string)
            }
        }
    }

    /// The house number with its addition appended, e.g. "5A".
    pub fn house_number(&self) -> Option<String> {
        let (house_number, addition) = match self {
            Address::Dutch(address) => (
                address.house_number.as_ref(),
                address.house_number_addition.as_ref(),
            ),
            Address::International(address) => (
                address.house_number.as_ref(),
                address.house_number_addition.as_ref(),
            ),
        };

        house_number.map(|house_number| match addition {
            Some(addition) => format!("{house_number}{addition}"),
            None => house_number.to_string(),
        })
    }

    /// The street name, house number, and house number addition, e.g. "Main Street 10A".
    ///
    /// Returns `None` if the street name or house number are `None`.
    pub fn address_line_1(&self) -> Option<String> {
        let (street_name, house_number, house_number_addition) = match self {
            Address::Dutch(address) => (
                address.street_name.as_ref(),
                address.house_number.as_ref(),
                address.house_number_addition.as_ref(),
            ),
            Address::International(address) => (
                address.street_name.as_ref(),
                address.house_number.as_ref(),
                address.house_number_addition.as_ref(),
            ),
        };

        match (street_name, house_number, house_number_addition) {
            (Some(street_name), Some(house_number), Some(house_number_addition)) => Some(format!(
                "{street_name} {house_number}{house_number_addition}"
            )),
            (Some(street_name), Some(house_number), None) => {
                Some(format!("{street_name} {house_number}"))
            }
            _ => None,
        }
    }

    /// The locality, postal code, state/province, and country.
    ///
    /// Returns `None` if the locality or postal code are `None`.
    pub fn address_line_2(&self) -> Option<String> {
        match (
            self.locality(),
            self.postal_code(),
            self.state_or_province(),
            self.country(),
        ) {
            (Some(locality), Some(postal_code), Some(state_or_province), Some(country)) => Some(
                format!("{postal_code} {locality}, {state_or_province} ({country})"),
            ),
            (Some(locality), Some(postal_code), Some(state_or_province), None) => {
                Some(format!("{postal_code} {locality}, {state_or_province}"))
            }
            (Some(locality), Some(postal_code), None, Some(country)) => {
                Some(format!("{postal_code} {locality} ({country})"))
            }
            (Some(locality), Some(postal_code), None, None) => {
                Some(format!("{postal_code} {locality}"))
            }
            _ => None,
        }
    }

    pub fn is_dutch(&self) -> bool {
        matches!(self, Self::Dutch(_))
    }
}

impl Problematic<Severity> for Address {
    fn get_problems(&self, severity: Severity) -> Problems {
        match self {
            Address::Dutch(address) => address.get_problems(severity),
            Address::International(address) => address.get_problems(severity),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::structs::common::HasSeverity;

    use super::*;
    use std::str::FromStr;

    fn sample_address() -> DutchAddress {
        DutchAddress {
            street_name: Some(StreetName::from_str("Hoofdstraat").unwrap()),
            house_number: Some(HouseNumber::from_str("123").unwrap()),
            house_number_addition: Some(HouseNumberAddition::from_str("a").unwrap()),
            locality: Some(Locality::from_str("Amsterdam").unwrap()),
            postal_code: Some(PostalCode::from_str("1234 AB").unwrap()),
            known_in_bag: Some(true),
        }
    }

    #[test]
    fn is_all_good_when_required_parts_present() {
        let mut address = sample_address();
        address.house_number_addition = None;

        assert!(address.get_problems(Severity::Info).is_all_good());
    }

    #[test]
    fn is_not_all_good_when_any_required_part_missing() {
        let mut address = sample_address();
        address.locality = None;

        assert!(!address.get_problems(Severity::Info).is_all_good());
    }

    /// Places the BAG names like the Dutch alias of a Frisian village.
    #[test]
    fn ambiguously_named_places_survive_a_bag_round_trip() {
        for (postal_code, street_name, locality) in [
            ("1794AA", "Mulderstraat", "Oosterend"),
            ("5437AA", "Molenstraat", "Beers"),
            ("5807AE", "Ooster Thienweg", "Oostrum"),
            ("9152AA", "Poelewei", "Waaxens"),
        ] {
            let mut address = DutchAddress {
                street_name: Some(StreetName::from_str(street_name).unwrap()),
                house_number: Some(HouseNumber::from_str("1").unwrap()),
                house_number_addition: None,
                locality: Some(Locality::from_str(locality).unwrap()),
                postal_code: Some(PostalCode::from_str(postal_code).unwrap()),
                known_in_bag: None,
            };

            assert_eq!(
                address
                    .locality
                    .as_ref()
                    .map(ToString::to_string)
                    .as_deref(),
                Some(locality),
                "{locality} is an official locality and must be kept as it is"
            );

            address.update_is_known_in_bag();
            assert_eq!(
                address.known_in_bag,
                Some(true),
                "{street_name} 1, {postal_code} {locality} should be found in the BAG"
            );
        }
    }

    #[test]
    fn a_locality_in_either_language_is_found_in_the_bag() {
        for locality in ["Berltsum", "Berlikum"] {
            let mut address = DutchAddress {
                street_name: Some(StreetName::from_str("Bûterhoeke").unwrap()),
                house_number: Some(HouseNumber::from_str("1").unwrap()),
                house_number_addition: None,
                locality: Some(Locality::from_str(locality).unwrap()),
                postal_code: Some(PostalCode::from_str("9041AA").unwrap()),
                known_in_bag: None,
            };

            assert_eq!(
                address
                    .locality
                    .as_ref()
                    .map(ToString::to_string)
                    .as_deref(),
                Some(locality),
                "{locality} is a name of the place and must be kept as it is"
            );

            address.update_is_known_in_bag();
            assert_eq!(
                address.known_in_bag,
                Some(true),
                "Bûterhoeke 1, 9041AA {locality} should be found in the BAG"
            );
        }
    }

    #[test]
    fn is_empty_when_all_parts_absent_or_empty() {
        let address = DutchAddress {
            street_name: Some(StreetName::default()),
            house_number: Some(HouseNumber::default()),
            house_number_addition: Some(HouseNumberAddition::default()),
            locality: Some(Locality::default()),
            postal_code: Some(PostalCode::default()),
            known_in_bag: None,
        };

        assert!(address.is_empty());
    }

    #[test]
    fn is_not_empty_when_any_part_has_value() {
        let address = DutchAddress {
            street_name: Some(StreetName::from_str("Kerkstraat").unwrap()),
            ..DutchAddress::default()
        };

        assert!(!address.is_empty());
    }

    #[test]
    fn address_line_1_with_addition() {
        let address = sample_address();

        assert_eq!(
            Some("Hoofdstraat 123a".to_string()),
            Address::Dutch(address).address_line_1()
        );
    }

    #[test]
    fn address_line_1_without_addition() {
        let mut address = sample_address();
        address.house_number_addition = None;

        assert_eq!(
            Some("Hoofdstraat 123".to_string()),
            Address::Dutch(address).address_line_1()
        );
    }

    #[test]
    fn address_line_1_requires_street_and_house_number() {
        let mut address = sample_address();
        address.street_name = None;

        assert_eq!(None, Address::Dutch(address).address_line_1());

        address = sample_address();
        address.house_number = None;

        assert_eq!(None, Address::Dutch(address).address_line_1());
    }

    #[test]
    fn street_name_returns_value_for_both_variants() {
        assert_eq!(
            Some("Hoofdstraat".to_string()),
            Address::Dutch(sample_address()).street_name()
        );
        assert_eq!(
            Some("Downing Street".to_string()),
            Address::International(sample_international_address()).street_name()
        );
    }

    #[test]
    fn street_name_is_none_when_absent() {
        let mut address = sample_address();
        address.street_name = None;

        assert_eq!(None, Address::Dutch(address).street_name());
    }

    #[test]
    fn house_number_appends_addition() {
        assert_eq!(
            Some("123a".to_string()),
            Address::Dutch(sample_address()).house_number()
        );
    }

    #[test]
    fn house_number_without_addition() {
        let mut address = sample_address();
        address.house_number_addition = None;

        assert_eq!(
            Some("123".to_string()),
            Address::Dutch(address).house_number()
        );

        // The international sample has no addition either.
        assert_eq!(
            Some("10".to_string()),
            Address::International(sample_international_address()).house_number()
        );
    }

    #[test]
    fn house_number_is_none_when_absent() {
        let mut address = sample_address();
        address.house_number = None;

        assert_eq!(None, Address::Dutch(address).house_number());
    }

    fn sample_international_address() -> InternationalAddress {
        InternationalAddress {
            street_name: Some(StreetName::from_str("Downing Street").unwrap()),
            house_number: Some(HouseNumber::from_str("10").unwrap()),
            house_number_addition: None,
            locality: Some(Locality::from_str("London").unwrap()),
            state_or_province: Some(StateOrProvince::from_str("Greater London").unwrap()),
            postal_code: Some(InternationalPostalCode::from_str("SW1A 2AA").unwrap()),
            country: Some(CountryCode::from_str("GB").unwrap()),
        }
    }

    #[test]
    fn international_address_is_all_good_when_required_parts_present() {
        assert!(
            sample_international_address()
                .get_problems(Severity::Info)
                .is_all_good()
        );
    }

    #[test]
    fn international_address_requires_country_for_completeness() {
        let mut address = sample_international_address();
        address.country = None;

        assert!(!address.get_problems(Severity::Info).is_all_good());
    }

    #[test]
    fn international_address_adds_country_warning_when_country_missing() {
        let mut address = sample_international_address();
        address.country = None;

        let problems = address.get_problems(Severity::Error);

        assert_eq!(
            vec![PotentialProblems::IncompleteAddress {
                severity: Severity::Error,
                problems: vec![EmptyAddressProblems::Country]
            }],
            problems.potential_problems
        );
        assert!(problems.info_problems.is_empty());
    }

    #[test]
    fn international_address_is_empty_when_all_parts_absent_or_empty() {
        let address = InternationalAddress {
            street_name: Some(StreetName::default()),
            house_number: Some(HouseNumber::default()),
            house_number_addition: Some(HouseNumberAddition::default()),
            locality: Some(Locality::default()),
            state_or_province: Some(StateOrProvince::default()),
            postal_code: Some(InternationalPostalCode::default()),
            country: Some(CountryCode::default()),
        };

        assert!(address.is_empty());
    }

    #[test]
    fn international_address_line_2_includes_state_or_province_when_present() {
        assert_eq!(
            Some("SW1A 2AA London, Greater London (GB)".to_string()),
            Address::International(sample_international_address()).address_line_2()
        );
    }
}
