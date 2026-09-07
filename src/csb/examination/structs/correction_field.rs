use serde::Deserialize;

use crate::structs::{brp::BrpCheckedField, common::DateOfBirth, persons::Person};

/// Which personal-data field of a candidate a correction dialog operates on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CandidateCorrectionField {
    Initials,
    LastNamePrefix,
    LastName,
    DateOfBirth,
    PlaceOfResidence,
}

impl CandidateCorrectionField {
    pub fn label(self, locale: crate::Locale) -> String {
        match self {
            Self::Initials => crate::trans!("person.fields.initials", locale),
            Self::LastNamePrefix => crate::trans!("person.fields.last_name_prefix", locale),
            Self::LastName => crate::trans!("person.fields.last_name", locale),
            Self::DateOfBirth => crate::trans!("person.fields.date_of_birth", locale),
            Self::PlaceOfResidence => crate::trans!("person.fields.place_of_residence", locale),
        }
    }

    /// The BRP field this dialog corrects, so what the BRP holds can be
    /// offered while correcting.
    pub fn brp_field(self) -> BrpCheckedField {
        match self {
            Self::Initials => BrpCheckedField::Initials,
            Self::LastNamePrefix => BrpCheckedField::LastNamePrefix,
            Self::LastName => BrpCheckedField::LastName,
            Self::DateOfBirth => BrpCheckedField::DateOfBirth,
            Self::PlaceOfResidence => BrpCheckedField::PlaceOfResidence,
        }
    }

    /// Extract the string representation of this field from a person, using
    /// the same formatting as the examination pages and the correction overlay.
    pub fn extract(self, person: &Person) -> String {
        match self {
            Self::Initials => person.name.initials.to_string(),
            Self::LastNamePrefix => person
                .name
                .last_name_prefix
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
            Self::LastName => person.name.last_name.to_string(),
            Self::DateOfBirth => DateOfBirth::format_option(&person.personal_data.date_of_birth),
            Self::PlaceOfResidence => person
                .personal_data
                .place_of_residence
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
        }
    }
}

impl std::str::FromStr for CandidateCorrectionField {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "initials" => Ok(Self::Initials),
            "last-name-prefix" => Ok(Self::LastNamePrefix),
            "last-name" => Ok(Self::LastName),
            "date-of-birth" => Ok(Self::DateOfBirth),
            "place-of-residence" => Ok(Self::PlaceOfResidence),
            _ => Err("unknown correction field"),
        }
    }
}

impl std::fmt::Display for CandidateCorrectionField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Initials => write!(f, "initials"),
            Self::LastNamePrefix => write!(f, "last-name-prefix"),
            Self::LastName => write!(f, "last-name"),
            Self::DateOfBirth => write!(f, "date-of-birth"),
            Self::PlaceOfResidence => write!(f, "place-of-residence"),
        }
    }
}

impl<'de> Deserialize<'de> for CandidateCorrectionField {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}
