//! Locality (place / city name).
//!
//! Validation rules (via `FromStr`):
//! - Whitespace is trimmed; the value must be 1..=200 characters.
//! - Only Teletex characters are allowed.
//! - A misspelling is replaced by the official name (see
//!   [`correct_locality_name`]); both names of a Frisian locality are kept.
use crate::{
    form::{ValidationError, validate_length, validate_teletex_chars},
    transparent_string,
    utils::locality_aliases::correct_locality_name,
};

transparent_string! {
    pub struct Locality(String);
}

impl std::str::FromStr for Locality {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed_value = validate_length(value, 1, 200)?;
        validate_teletex_chars(&trimmed_value)?;

        let normalized =
            correct_locality_name(&trimmed_value).map_or(trimmed_value, str::to_string);

        Ok(Locality(normalized))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn keeps_official_name_unchanged() {
        let locality = Locality::from_str("Amsterdam").expect("locality");

        assert_eq!(locality.to_string(), "Amsterdam");
    }

    #[test]
    fn replaces_a_misspelling_with_the_official_name() {
        let locality = Locality::from_str("Den Haag").expect("locality");

        assert_eq!(locality.to_string(), "'s-Gravenhage");
    }

    #[test]
    fn keeps_both_names_of_a_frisian_locality() {
        for name in ["Berltsum", "Berlikum"] {
            let locality = Locality::from_str(name).expect("locality");

            assert_eq!(locality.to_string(), name);
        }

        // Beers (Land van Cuijk) is also the Dutch name of Bears in Fryslan,
        // so correcting it would move the candidate to another province.
        let locality = Locality::from_str("Beers").expect("locality");

        assert_eq!(locality.to_string(), "Beers");
    }

    #[test]
    fn rejects_too_short_values() {
        assert_eq!(
            Locality::from_str("  ").expect_err("empty locality"),
            ValidationError::ValueShouldNotBeEmpty
        );
    }
}
