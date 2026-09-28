use std::str::FromStr;

use chrono::{Local, NaiveDateTime};
use serde::{Deserialize, Serialize, Serializer};

use crate::form::ValidationError;

const I1: &str = "i1";
const I4: &str = "i4";

/// The proces-verbaal a hearing belongs to. Both models report on a hearing of
/// the central electoral committee, but on different ones: the I 1 on the
/// hearing about the examination of the lists, the I 4 on the hearing in which
/// the lists are established. They are recorded separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HearingModel {
    I1,
    I4,
}

impl HearingModel {
    fn as_str(self) -> &'static str {
        match self {
            HearingModel::I1 => I1,
            HearingModel::I4 => I4,
        }
    }

    pub fn is_i1(self) -> bool {
        matches!(self, HearingModel::I1)
    }
}

impl FromStr for HearingModel {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            I1 => Ok(HearingModel::I1),
            I4 => Ok(HearingModel::I4),
            _ => Err(ValidationError::InvalidValue),
        }
    }
}

impl std::fmt::Display for HearingModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// Serialized as a plain string, both to survive the axum path deserializer
// (which drives every field through `deserialize_str`) and to keep the event
// payload readable; the two halves have to agree, so neither is derived.
impl Serialize for HearingModel {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for HearingModel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

/// The moment of a hearing and the committee members who sign its report, as
/// entered by the committee. Overrides what the election configuration says
/// about the session; see [`HearingModel`] for the two hearings this covers.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct HearingDetails {
    pub date_time: NaiveDateTime,
    pub chair: String,
    pub members: Vec<String>,
}

impl Default for HearingDetails {
    fn default() -> Self {
        Self {
            date_time: Local::now().naive_local(),
            chair: String::new(),
            members: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hearing_details_gets_current_time() {
        let hearing_details = HearingDetails::default();
        let now = Local::now().naive_local();

        assert!(
            // Give a little leeway
            now.signed_duration_since(hearing_details.date_time)
                .num_milliseconds()
                < 20
        );
    }

    #[test]
    fn hearing_model_parses_from_its_path_segment() {
        assert_eq!("i1".parse(), Ok(HearingModel::I1));
        assert_eq!("i4".parse(), Ok(HearingModel::I4));
        assert_eq!(
            "i2".parse::<HearingModel>(),
            Err(ValidationError::InvalidValue)
        );
    }

    #[test]
    fn hearing_model_displays_as_its_path_segment() {
        assert_eq!(HearingModel::I1.to_string(), "i1");
        assert_eq!(HearingModel::I4.to_string(), "i4");
    }
}
