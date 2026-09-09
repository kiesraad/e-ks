use std::str::FromStr;

use chrono::{NaiveDate, NaiveTime};
use serde::Deserialize;
use validate::Validate;

use crate::{
    constants::DEFAULT_DATE_FORMAT, csb::examination::structs::HearingDetails,
    form::ValidationError, structs::common::DATE_FORMAT_REGEX,
};

#[derive(Default, Clone)]
struct HearingFormTarget {
    date_of_hearing: DateOfHearing,
    time_of_hearing: TimeOfHearing,
    signer_0: String,
    signer_1: String,
    signer_2: String,
    signer_3: String,
    signer_4: String,
    signer_5: String,
    signer_6: String,
    signer_7: String,
    signer_8: String,
    signer_9: String,
}

impl From<HearingFormTarget> for HearingDetails {
    fn from(value: HearingFormTarget) -> Self {
        let members = [
            value.signer_0,
            value.signer_1,
            value.signer_2,
            value.signer_3,
            value.signer_4,
            value.signer_5,
            value.signer_6,
            value.signer_7,
            value.signer_8,
            value.signer_9,
        ]
        .into_iter()
        .filter_map(|m| {
            let m = m.trim().to_string();
            (!m.is_empty()).then_some(m)
        })
        .collect();

        let date_time = value.date_of_hearing.and_time(*value.time_of_hearing);

        Self { date_time, members }
    }
}

#[derive(Default, Clone)]
struct DateOfHearing(NaiveDate);

impl FromStr for DateOfHearing {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if !DATE_FORMAT_REGEX.is_match(value) {
            return Err(ValidationError::InvalidValue);
        }

        let naive_date = NaiveDate::parse_from_str(value, DEFAULT_DATE_FORMAT)
            .map_err(|_| ValidationError::InvalidValue)?;

        Ok(Self(naive_date))
    }
}

impl std::ops::Deref for DateOfHearing {
    type Target = chrono::NaiveDate;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Default, Clone)]
struct TimeOfHearing(NaiveTime);

impl FromStr for TimeOfHearing {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let time =
            NaiveTime::parse_from_str(value, "%H:%M").map_err(|_| ValidationError::InvalidValue)?;
        Ok(Self(time))
    }
}

impl std::ops::Deref for TimeOfHearing {
    type Target = chrono::NaiveTime;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Deserialize, Debug, Validate, Default)]
#[validate(target = "HearingFormTarget")]
#[serde(default)]
pub struct HearingForm {
    #[validate(parse = "DateOfHearing")]
    date_of_hearing: String,

    #[validate(parse = "TimeOfHearing")]
    time_of_hearing: String,

    signer_0: String,
    signer_1: String,
    signer_2: String,
    signer_3: String,
    signer_4: String,
    signer_5: String,
    signer_6: String,
    signer_7: String,
    signer_8: String,
    signer_9: String,
}
