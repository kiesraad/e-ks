use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{
    form::ValidationError,
    structs::audit_log::{AuditLeaf, AuditValue},
};

/// In the BAG a house number is a natural number from 1 to 99999.
///
/// See <https://catalogus.kadaster.nl/bag/nl/page/Huisnummer>
const MIN_HOUSE_NUMBER: u32 = 1;
const MAX_HOUSE_NUMBER: u32 = 99_999;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct HouseNumber(u32);

impl HouseNumber {
    /// Return the house number as an integer.
    pub fn as_number(&self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for HouseNumber {
    type Error = ValidationError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if (MIN_HOUSE_NUMBER..=MAX_HOUSE_NUMBER).contains(&value) {
            Ok(HouseNumber(value))
        } else {
            Err(ValidationError::InvalidValue)
        }
    }
}

impl From<HouseNumber> for u32 {
    fn from(value: HouseNumber) -> Self {
        value.0
    }
}

impl FromStr for HouseNumber {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed_value = value.trim();

        if trimmed_value.is_empty() {
            return Err(ValidationError::ValueShouldNotBeEmpty);
        }

        if !trimmed_value.chars().all(|c| c.is_ascii_digit()) {
            return Err(ValidationError::InvalidValue);
        }

        trimmed_value
            .parse::<u32>()
            .map_err(|_| ValidationError::InvalidValue)?
            .try_into()
    }
}

impl std::fmt::Display for HouseNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl AuditLeaf for HouseNumber {
    fn audit_value(&self) -> AuditValue {
        AuditValue::text(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numbers_in_the_bag_range() {
        assert_eq!(1, "1".parse::<HouseNumber>().unwrap().as_number());
        assert_eq!(12, " 12 ".parse::<HouseNumber>().unwrap().as_number());
        assert_eq!(7, "007".parse::<HouseNumber>().unwrap().as_number());
        assert_eq!(99_999, "99999".parse::<HouseNumber>().unwrap().as_number());
    }

    #[test]
    fn rejects_invalid_input() {
        assert_eq!(
            Err(ValidationError::ValueShouldNotBeEmpty),
            " ".parse::<HouseNumber>()
        );
        assert_eq!(
            Err(ValidationError::InvalidValue),
            "0".parse::<HouseNumber>()
        );
        assert_eq!(
            Err(ValidationError::InvalidValue),
            "12a".parse::<HouseNumber>()
        );
        assert_eq!(
            Err(ValidationError::InvalidValue),
            "-1".parse::<HouseNumber>()
        );
        assert_eq!(
            Err(ValidationError::InvalidValue),
            "100000".parse::<HouseNumber>()
        );
    }

    #[test]
    fn serializes_as_an_integer() {
        let house_number: HouseNumber = "12".parse().unwrap();

        assert_eq!("12", serde_json::to_string(&house_number).unwrap());
        assert_eq!(
            house_number,
            serde_json::from_str::<HouseNumber>("12").unwrap()
        );
        assert!(serde_json::from_str::<HouseNumber>("0").is_err());
        assert!(serde_json::from_str::<HouseNumber>("100000").is_err());
    }
}
