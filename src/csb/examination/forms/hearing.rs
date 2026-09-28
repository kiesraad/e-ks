use std::str::FromStr;

use chrono::{NaiveDate, NaiveTime};
use serde::Deserialize;
use validate::Validate;

use crate::{
    constants::{DEFAULT_DATE_FORMAT, DEFAULT_TIME_FORMAT},
    form::ValidationError,
    structs::{common::DATE_FORMAT_REGEX, csb::HearingDetails},
};

/// Number of signer inputs rendered on the form.
const SIGNER_COUNT: usize = 10;
const SIGNERS_PER_ROW: usize = 2;

#[derive(Default, Clone)]
pub struct HearingDetailsFormTarget {
    date_of_hearing: DateOfHearing,
    time_of_hearing: TimeOfHearing,
    chair: String,
    signers: Vec<String>,
}

impl From<HearingDetailsFormTarget> for HearingDetails {
    fn from(value: HearingDetailsFormTarget) -> Self {
        let members = value
            .signers
            .into_iter()
            .filter_map(|m| {
                let m = m.trim().to_string();
                (!m.is_empty()).then_some(m)
            })
            .collect();

        let date_time = value.date_of_hearing.and_time(*value.time_of_hearing);

        Self {
            date_time,
            chair: value.chair.trim().to_string(),
            members,
        }
    }
}

impl From<HearingDetails> for HearingDetailsForm {
    fn from(value: HearingDetails) -> Self {
        Self {
            date_of_hearing: DateOfHearing(value.date_time.date()).format(),
            time_of_hearing: TimeOfHearing(value.date_time.time()).format(),
            chair: value.chair,
            signers: value.members,
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
        self.0.format(DEFAULT_TIME_FORMAT).to_string()
    }
}

impl FromStr for TimeOfHearing {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        NaiveTime::parse_from_str(value, DEFAULT_TIME_FORMAT)
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

    pub chair: String,
    pub signers: Vec<String>,
}

impl HearingDetailsForm {
    pub fn is_invalid_date(&self) -> bool {
        DateOfHearing::from_str(&self.date_of_hearing).is_err()
    }

    pub fn is_invalid_time(&self) -> bool {
        TimeOfHearing::from_str(&self.time_of_hearing).is_err()
    }

    /// Signer inputs grouped per form row, padded to [`SIGNER_COUNT`] so the
    /// page always renders the full set of fields.
    pub fn signer_rows(&self) -> Vec<Vec<(usize, String)>> {
        let mut signers = self.signers.clone();
        signers.resize(signers.len().max(SIGNER_COUNT), String::new());

        signers
            .into_iter()
            .enumerate()
            .collect::<Vec<_>>()
            .chunks(SIGNERS_PER_ROW)
            .map(<[(usize, String)]>::to_vec)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use axum::extract::FromRequest;

    use super::*;

    fn valid_form() -> HearingDetailsForm {
        HearingDetailsForm {
            date_of_hearing: "31-12-1999".to_string(),
            time_of_hearing: "12:34".to_string(),
            chair: "Vera Voorzitter".to_string(),
            signers: signers(&["Jan Klaassen", "Malle Babbe"]),
        }
    }

    /// The page always posts [`SIGNER_COUNT`] values, blank ones included.
    fn signers(filled: &[&str]) -> Vec<String> {
        let mut signers: Vec<String> = filled.iter().copied().map(String::from).collect();
        signers.resize(SIGNER_COUNT, String::new());
        signers
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
        assert_eq!(hearing_details.chair, "Vera Voorzitter");
        assert_eq!(
            hearing_details.members,
            vec!["Jan Klaassen".to_string(), "Malle Babbe".to_string()]
        );
    }

    #[test]
    fn validate_create_trims_and_drops_blank_signers() {
        let form = HearingDetailsForm {
            chair: "  Vera Voorzitter  ".to_string(),
            signers: signers(&["  Jan Klaassen  ", "   "]),
            ..valid_form()
        };

        let target = form.validate_create().expect("valid form");
        let hearing_details = HearingDetails::from(target);

        assert_eq!(hearing_details.chair, "Vera Voorzitter");
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
            chair: "Vera Voorzitter".to_string(),
            members: vec!["Jan Klaassen".to_string(), "Malle Babbe".to_string()],
        };

        let form = HearingDetailsForm::from(hearing_details);

        assert_eq!(form.date_of_hearing, "31-12-1999");
        assert_eq!(form.time_of_hearing, "12:34");
        assert_eq!(form.chair, "Vera Voorzitter");
        assert_eq!(
            form.signers,
            vec!["Jan Klaassen".to_string(), "Malle Babbe".to_string()]
        );
    }

    #[test]
    fn signer_rows_pads_to_the_full_set_of_inputs() {
        let form = HearingDetailsForm {
            signers: vec!["Jan Klaassen".to_string()],
            ..valid_form()
        };

        let rows = form.signer_rows();

        assert_eq!(rows.len(), SIGNER_COUNT / SIGNERS_PER_ROW);
        assert_eq!(
            rows[0],
            vec![(0, "Jan Klaassen".to_string()), (1, "".into())]
        );
        assert_eq!(rows[4], vec![(8, "".to_string()), (9, "".into())]);
    }

    #[test]
    fn signer_rows_keeps_signers_beyond_the_rendered_count() {
        let form = HearingDetailsForm {
            signers: (0..SIGNER_COUNT + 1).map(|i| i.to_string()).collect(),
            ..valid_form()
        };

        let rows = form.signer_rows();

        assert_eq!(rows.len(), SIGNER_COUNT / SIGNERS_PER_ROW + 1);
        assert_eq!(rows[5], vec![(10, "10".to_string())]);
    }

    #[tokio::test]
    async fn form_deserializes_repeated_signer_fields() {
        let request = axum::extract::Request::builder()
            .method("POST")
            .uri("/csb/examination/hearing-details")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(axum::body::Body::from(
                "date_of_hearing=31-12-1999&time_of_hearing=12%3A34\
                 &chair=Vera+Voorzitter\
                 &signers=Jan+Klaassen&signers=&signers=Malle+Babbe",
            ))
            .expect("request");

        let crate::Form(form) = crate::Form::<HearingDetailsForm>::from_request(request, &())
            .await
            .expect("form body");

        assert_eq!(form.date_of_hearing, "31-12-1999");
        assert_eq!(form.chair, "Vera Voorzitter");
        assert_eq!(form.signers, vec!["Jan Klaassen", "", "Malle Babbe"]);
    }
}
