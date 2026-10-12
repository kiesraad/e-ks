use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::form::ValidationError;

/// BSN value stored as a secret to reduce accidental exposure.
///
/// Validation (via `FromStr`) is strict and deterministic:
/// - Input is trimmed and must be non-empty.
/// - Length must be exactly 9 characters.
/// - All characters must be digits.
/// - It must pass the 11-test checksum (weighted sum from the right:
///   weights 2..=9 for the first 8 digits, and -1 for the last digit;
///   the total must be divisible by 11).
///
/// We store BSNs as `SecretString` as a defense-in-depth measure: Debug/Display
/// are redacted and callers must explicitly expose the value. This is not
/// watertight security. The value still exists in memory and can be exposed
/// or serialized when needed, so treat it as sensitive throughout the codebase.
#[derive(Default, Clone)]
pub struct Bsn(SecretString);

impl Bsn {
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }

    pub fn to_exposed_string(&self) -> String {
        self.expose().to_string()
    }
}

impl std::fmt::Debug for Bsn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Bsn([REDACTED])")
    }
}

impl std::fmt::Display for Bsn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl PartialEq for Bsn {
    fn eq(&self, other: &Self) -> bool {
        self.expose() == other.expose()
    }
}

impl Eq for Bsn {}

impl std::hash::Hash for Bsn {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.expose().hash(state);
    }
}

impl PartialOrd for Bsn {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Bsn {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.expose().cmp(other.expose())
    }
}

impl FromStr for Bsn {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed_value = value.trim();

        if trimmed_value.is_empty() {
            return Err(ValidationError::ValueShouldNotBeEmpty);
        }

        if trimmed_value.len() < 9 {
            return Err(ValidationError::ValueTooShort(trimmed_value.len(), 9));
        }

        if trimmed_value.len() > 9 {
            return Err(ValidationError::ValueTooLong(trimmed_value.len(), 9));
        }

        let mut checksum = 0;
        for (i, digit) in trimmed_value.chars().rev().enumerate() {
            checksum += (if i == 0 { -1 } else { i as i32 + 1 })
                * (digit.to_digit(10).ok_or(ValidationError::InvalidValue)?) as i32;
        }

        if checksum % 11 != 0 {
            return Err(ValidationError::InvalidChecksum);
        }

        Ok(Bsn(SecretString::from(trimmed_value.to_string())))
    }
}

impl Serialize for Bsn {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.expose())
    }
}

impl<'de> Deserialize<'de> for Bsn {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

pub const BSN_NONE_CONFIRMATION: &str = "none-confirmed";

/// This enum represents the state of a person's BSN (Dutch citizen service number):
/// - `Bsn`: A BSN has been provided and confirmed valid.
/// - `NoneConfirmed`: It is confirmed that this person does not have a BSN.
#[derive(Debug, Serialize, Eq, PartialEq, Deserialize, Clone)]
pub enum BsnOrNoneConfirmed {
    /// BSN provided and confirmed valid
    Bsn(Bsn),
    /// It is confirmed that this person does not have a BSN
    NoneConfirmed,
}

impl FromStr for BsnOrNoneConfirmed {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.trim() == BSN_NONE_CONFIRMATION {
            Ok(BsnOrNoneConfirmed::NoneConfirmed)
        } else {
            Ok(BsnOrNoneConfirmed::Bsn(value.parse()?))
        }
    }
}

impl BsnOrNoneConfirmed {
    pub fn to_exposed_string(&self) -> String {
        match self {
            BsnOrNoneConfirmed::NoneConfirmed => BSN_NONE_CONFIRMATION.to_string(),
            BsnOrNoneConfirmed::Bsn(b) => b.to_exposed_string(),
        }
    }
}
