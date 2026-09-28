//! Locale detection and formatting helpers for request handling.
//! Extracted from Accept-Language headers and used by Context and templates.

use serde::Deserialize;
use std::str::FromStr;

/// Supported UI locales for requests and templates.
#[derive(Default, Deserialize, Clone, Copy, Debug, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    En,
    #[default]
    Nl,
}

impl FromStr for Locale {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "en" => Ok(Locale::En),
            "nl" => Ok(Locale::Nl),
            _ => Err("invalid locale"),
        }
    }
}

impl Locale {
    pub fn as_str(&self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::Nl => "nl",
        }
    }

    pub fn as_usize(&self) -> usize {
        match self {
            Locale::En => 0,
            Locale::Nl => 1,
        }
    }
}

impl std::fmt::Display for Locale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use crate::Locale;

    #[test]
    fn converts_to_language_codes() {
        assert_eq!(Locale::En.as_str(), "en");
        assert_eq!(Locale::Nl.as_str(), "nl");
    }
}
