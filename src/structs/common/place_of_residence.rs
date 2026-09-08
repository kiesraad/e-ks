//! Place of residence (the locality a person lives in).
//!
//! Unlike [`Locality`](super::Locality), a place of residence records whether
//! the (normalized) name was found in the BAG, so callers can flag values that
//! do not correspond to a known Dutch locality without rejecting them outright.
//!
//! Validation rules (via `FromStr`):
//! - Whitespace is trimmed; the value must be 1..=200 characters.
//! - Only Teletex characters are allowed.
//! - A misspelling is replaced by the official name (see
//!   [`correct_locality_name`]); both names of a Frisian locality are kept.
//! - The normalized name is looked up in the BAG to pick the variant.
use std::ops::Deref;

use serde::{Deserialize, Serialize};

use crate::{
    form::{ValidationError, validate_length, validate_teletex_chars},
    utils::{bag, locality_aliases::correct_locality_name},
};

/// Localities (Kralendijk, Rincon) and municipalities (Bonaire, Saba, Sint
/// Eustatius) of the Caribbean Netherlands. Residents of these places have
/// country code NL but need an authorised person (gemachtigde) instead of a
/// Dutch correspondence address. Mirrors bagatel's private `CN_LOCALITIES`
/// and `CN_MUNICIPALITIES` (bagatel 0.8.3).
pub const CARIBBEAN_NL_PLACES: &[&str] =
    &["Kralendijk", "Rincon", "Bonaire", "Saba", "Sint Eustatius"];

/// A validated place of residence, tagged by whether it exists in the BAG.
///
/// Both variants wrap the same kind of normalized locality name; the
/// distinction only records the outcome of the BAG lookup performed while
/// parsing. Use [`Display`](std::fmt::Display), [`Deref`] or [`AsRef`] to read
/// the underlying name regardless of variant.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, Hash)]
pub enum PlaceOfResidence {
    /// The name matches a locality known in the BAG.
    Known(String),
    /// A syntactically valid name with no matching BAG locality.
    Unknown(String),
}

impl std::fmt::Display for PlaceOfResidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlaceOfResidence::Known(name) | PlaceOfResidence::Unknown(name) => write!(f, "{name}"),
        }
    }
}

impl Deref for PlaceOfResidence {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        match self {
            PlaceOfResidence::Known(name) | PlaceOfResidence::Unknown(name) => name,
        }
    }
}

impl AsRef<str> for PlaceOfResidence {
    fn as_ref(&self) -> &str {
        match self {
            PlaceOfResidence::Known(name) | PlaceOfResidence::Unknown(name) => name.as_str(),
        }
    }
}

impl std::str::FromStr for PlaceOfResidence {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed_value = validate_length(value, 1, 200)?;
        validate_teletex_chars(&trimmed_value)?;

        let normalized =
            correct_locality_name(&trimmed_value).map_or(trimmed_value, str::to_string);

        if bag::locality_exists(&normalized, true, true) {
            Ok(PlaceOfResidence::Known(normalized))
        } else {
            Ok(PlaceOfResidence::Unknown(normalized))
        }
    }
}

impl PlaceOfResidence {
    pub fn is_unknown_opt(por: &Option<Self>) -> bool {
        matches!(por, Some(PlaceOfResidence::Unknown(_)))
    }

    /// Whether this place lies in the Caribbean Netherlands (see
    /// [`CARIBBEAN_NL_PLACES`]). Case-insensitive, so a manually typed
    /// [`Unknown`](PlaceOfResidence::Unknown) variant like "kralendijk" still
    /// matches.
    pub fn is_caribbean_nl(&self) -> bool {
        CARIBBEAN_NL_PLACES
            .iter()
            .any(|place| place.eq_ignore_ascii_case(self.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_of_residence_can_be_single_character() {
        "E".parse::<PlaceOfResidence>()
            .expect("names of length one should be allowed");
    }

    #[test]
    fn caribbean_nl_places_are_detected_case_insensitively() {
        for place in CARIBBEAN_NL_PLACES {
            assert!(PlaceOfResidence::Known(place.to_string()).is_caribbean_nl());
            assert!(PlaceOfResidence::Unknown(place.to_lowercase()).is_caribbean_nl());
        }

        assert!(!PlaceOfResidence::Known("Amsterdam".to_string()).is_caribbean_nl());
    }
}
