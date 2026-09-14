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
            signer_0: value.members.first().cloned().unwrap_or_default(),
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

impl HearingDetailsForm {
    pub fn is_invalid_date(&self) -> bool {
        DateOfHearing::from_str(&self.date_of_hearing).is_err()
    }

    pub fn is_invalid_time(&self) -> bool {
        TimeOfHearing::from_str(&self.time_of_hearing).is_err()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_form() -> HearingDetailsForm {
        HearingDetailsForm {
            date_of_hearing: "31-12-1999".to_string(),
            time_of_hearing: "12:34".to_string(),
            signer_0: "Jan Klaassen".to_string(),
            signer_1: "Malle Babbe".to_string(),
            signer_2: String::new(),
            signer_3: String::new(),
            signer_4: String::new(),
            signer_5: String::new(),
            signer_6: String::new(),
            signer_7: String::new(),
            signer_8: String::new(),
            signer_9: String::new(),
        }
    }

    #[test]
    fn date_of_hearing_parses_valid_date() {
        assert!("31-12-1999".parse::<DateOfHearing>().is_ok());
    }

    #[test]
    fn date_of_hearing_rejects_wrong_format() {
        assert!(matches!(
            "1999-12-31".parse::<DateOfHearing>(),
            Err(ValidationError::InvalidValue)
        ));
        assert!(matches!(
            "31-12-99".parse::<DateOfHearing>(),
            Err(ValidationError::InvalidValue)
        ));
    }

    #[test]
    fn date_of_hearing_rejects_nonexistent_date() {
        // Matches the day-month-year regex, but April only has 30 days.
        assert!(matches!(
            "31-04-2020".parse::<DateOfHearing>(),
            Err(ValidationError::InvalidValue)
        ));
    }

    #[test]
    fn time_of_hearing_parses_valid_time() {
        assert!("12:34".parse::<TimeOfHearing>().is_ok());
    }

    #[test]
    fn time_of_hearing_rejects_invalid_time() {
        assert!(matches!(
            "25:00".parse::<TimeOfHearing>(),
            Err(ValidationError::InvalidValue)
        ));
        assert!(matches!(
            "12:60".parse::<TimeOfHearing>(),
            Err(ValidationError::InvalidValue)
        ));
        assert!(matches!(
            "1234".parse::<TimeOfHearing>(),
            Err(ValidationError::InvalidValue)
        ));
    }

    #[test]
    fn is_invalid_date_reflects_parse_result() {
        let mut form = valid_form();
        assert!(!form.is_invalid_date());

        form.date_of_hearing = "not-a-date".to_string();
        assert!(form.is_invalid_date());
    }

    #[test]
    fn is_invalid_time_reflects_parse_result() {
        let mut form = valid_form();
        assert!(!form.is_invalid_time());

        form.time_of_hearing = "not-a-time".to_string();
        assert!(form.is_invalid_time());
    }

    #[test]
    fn validate_create_succeeds_and_converts_to_hearing_details() {
        let target = valid_form().validate_create().expect("valid form");
        let hearing_details = HearingDetails::from(target);

        assert_eq!(
            hearing_details
                .date_time
                .format("%Y-%m-%d %H:%M")
                .to_string(),
            "1999-12-31 12:34"
        );
        assert_eq!(
            hearing_details.members,
            vec!["Jan Klaassen".to_string(), "Malle Babbe".to_string()]
        );
    }

    #[test]
    fn validate_create_trims_and_drops_blank_signers() {
        let form = HearingDetailsForm {
            signer_0: "  Jan Klaassen  ".to_string(),
            signer_1: "   ".to_string(),
            ..valid_form()
        };

        let target = form.validate_create().expect("valid form");
        let hearing_details = HearingDetails::from(target);

        assert_eq!(hearing_details.members, vec!["Jan Klaassen".to_string()]);
    }

    #[test]
    fn validate_create_rejects_blank_date_and_time() {
        let form = HearingDetailsForm {
            date_of_hearing: String::new(),
            time_of_hearing: String::new(),
            ..valid_form()
        };

        let Err(form_data) = form.validate_create() else {
            panic!("expected validation errors for blank date/time");
        };
        let errors = form_data.errors();
        assert!(errors.contains(&(
            "date_of_hearing".to_string(),
            ValidationError::ValueShouldNotBeEmpty
        )));
        assert!(errors.contains(&(
            "time_of_hearing".to_string(),
            ValidationError::ValueShouldNotBeEmpty
        )));
    }

    #[test]
    fn validate_create_rejects_invalid_date_and_time() {
        let form = HearingDetailsForm {
            date_of_hearing: "not-a-date".to_string(),
            time_of_hearing: "not-a-time".to_string(),
            ..valid_form()
        };

        let Err(form_data) = form.validate_create() else {
            panic!("expected validation errors for invalid date/time");
        };
        let errors = form_data.errors();
        assert!(errors.contains(&("date_of_hearing".to_string(), ValidationError::InvalidValue)));
        assert!(errors.contains(&("time_of_hearing".to_string(), ValidationError::InvalidValue)));
    }

    #[test]
    fn from_hearing_details_formats_date_and_time_and_fills_signers() {
        let hearing_details = HearingDetails {
            date_time: NaiveDate::from_ymd_opt(1999, 12, 31)
                .unwrap()
                .and_hms_opt(12, 34, 0)
                .unwrap(),
            members: vec!["Jan Klaassen".to_string(), "Malle Babbe".to_string()],
        };

        let form = HearingDetailsForm::from(hearing_details);

        assert_eq!(form.date_of_hearing, "31-12-1999");
        assert_eq!(form.time_of_hearing, "12:34");
        assert_eq!(form.signer_0, "Jan Klaassen");
        assert_eq!(form.signer_1, "Malle Babbe");
        assert_eq!(form.signer_2, "");
        assert_eq!(form.signer_9, "");
    }
}
