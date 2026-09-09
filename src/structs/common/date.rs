use chrono::NaiveDate;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{str::FromStr, sync::LazyLock};

use crate::{ElectionConfig, constants::DEFAULT_DATE_FORMAT, form::ValidationError};

pub static DATE_FORMAT_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d{1,2}-\d{1,2}-\d{4}$").expect("valid date regex"));

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, Hash)]
#[serde(transparent)]
pub struct DateOfBirth(NaiveDate);

impl std::ops::Deref for DateOfBirth {
    type Target = chrono::NaiveDate;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::fmt::Display for DateOfBirth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for DateOfBirth {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if !DATE_FORMAT_REGEX.is_match(value) {
            return Err(ValidationError::InvalidValue);
        }

        let naive_date = NaiveDate::parse_from_str(value, DEFAULT_DATE_FORMAT)
            .map_err(|_| ValidationError::InvalidValue)?;

        if naive_date > chrono::Utc::now().date_naive() {
            return Err(ValidationError::DateInFuture);
        }

        Ok(DateOfBirth(naive_date))
    }
}

impl From<NaiveDate> for DateOfBirth {
    fn from(value: NaiveDate) -> Self {
        Self(value)
    }
}

impl From<DateOfBirth> for NaiveDate {
    fn from(value: DateOfBirth) -> Self {
        value.0
    }
}

impl DateOfBirth {
    /// Age threshold (years) above which a date of birth triggers a data-quality warning
    pub const WARN_AGE: u32 = 110;

    pub fn is_very_old(&self) -> bool {
        chrono::Utc::now()
            .date_naive()
            .years_since(self.0)
            .is_some_and(|y| y >= Self::WARN_AGE)
    }

    pub fn is_too_young(&self, election: &ElectionConfig) -> bool {
        self.0 > election.eligible_date_of_birth()
    }

    pub fn format_option(date: &Option<Self>) -> String {
        date.as_ref()
            .map(|date| date.0.format(DEFAULT_DATE_FORMAT).to_string())
            .unwrap_or("-".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_of_birth_cannot_be_in_the_future() {
        assert!(matches!(
            "01-01-9999".parse::<DateOfBirth>(),
            Err(ValidationError::DateInFuture),
        ));

        assert!("06-04-2001".parse::<DateOfBirth>().is_ok());
    }

    #[test]
    fn format() {
        assert!("12-12-0009".parse::<DateOfBirth>().is_ok());
        assert!("12-12-1909".parse::<DateOfBirth>().is_ok());
        assert!(matches!(
            "12-12-09".parse::<DateOfBirth>(),
            Err(ValidationError::InvalidValue)
        ));
        assert!(matches!(
            "12-12-9".parse::<DateOfBirth>(),
            Err(ValidationError::InvalidValue)
        ));
    }

    #[test]
    fn format_option_test() {
        assert_eq!(
            DateOfBirth::format_option(&Some(DateOfBirth(
                NaiveDate::from_ymd_opt(2000, 2, 3).unwrap()
            ))),
            "03-02-2000".to_string()
        );

        assert_eq!(
            DateOfBirth::format_option(&Some(DateOfBirth(
                NaiveDate::from_ymd_opt(2000, 12, 13).unwrap()
            ))),
            "13-12-2000".to_string()
        );
    }
}
