use std::str::FromStr;

use chrono::{NaiveDate, NaiveTime};
use serde::Deserialize;
use validate::Validate;

use crate::{
    constants::DEFAULT_DATE_FORMAT,
    form::ValidationError,
    structs::{common::DATE_FORMAT_REGEX, csb::HearingDetails},
};

const TIME_FORMAT: &str = "%H:%M";

#[derive(Default, Clone)]
pub struct HearingDetailsFormTarget {
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

impl From<HearingDetailsFormTarget> for HearingDetails {
    fn from(value: HearingDetailsFormTarget) -> Self {
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

impl From<HearingDetails> for HearingDetailsForm {
    fn from(value: HearingDetails) -> Self {
        Self {
            date_of_hearing: DateOfHearing(value.date_time.date()).format(),
            time_of_hearing: TimeOfHearing(value.date_time.time()).format(),
            signer_0: value.members.get(0).cloned().unwrap_or_default(),
            signer_1: value.members.get(1).cloned().unwrap_or_default(),
            signer_2: value.members.get(2).cloned().unwrap_or_default(),
            signer_3: value.members.get(3).cloned().unwrap_or_default(),
            signer_4: value.members.get(4).cloned().unwrap_or_default(),
            signer_5: value.members.get(5).cloned().unwrap_or_default(),
            signer_6: value.members.get(6).cloned().unwrap_or_default(),
            signer_7: value.members.get(7).cloned().unwrap_or_default(),
            signer_8: value.members.get(8).cloned().unwrap_or_default(),
            signer_9: value.members.get(9).cloned().unwrap_or_default(),
        }
    }
}

#[derive(Default, Clone)]
struct DateOfHearing(NaiveDate);

impl DateOfHearing {
    fn format(&self) -> String {
        self.0.format(DEFAULT_DATE_FORMAT).to_string()
    }
}

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

impl TimeOfHearing {
    fn format(&self) -> String {
        self.0.format(TIME_FORMAT).to_string()
    }
}

impl FromStr for TimeOfHearing {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        NaiveTime::parse_from_str(value, TIME_FORMAT)
            .map_err(|_| ValidationError::InvalidValue)
            .map(Self)
    }
}

impl std::ops::Deref for TimeOfHearing {
    type Target = chrono::NaiveTime;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Deserialize, Debug, Validate, Default)]
#[validate(target = "HearingDetailsFormTarget")]
#[serde(default)]
pub struct HearingDetailsForm {
    #[validate(parse = "DateOfHearing")]
    pub date_of_hearing: String,

    #[validate(parse = "TimeOfHearing")]
    pub time_of_hearing: String,

    pub signer_0: String,
    pub signer_1: String,
    pub signer_2: String,
    pub signer_3: String,
    pub signer_4: String,
    pub signer_5: String,
    pub signer_6: String,
    pub signer_7: String,
    pub signer_8: String,
    pub signer_9: String,
}
