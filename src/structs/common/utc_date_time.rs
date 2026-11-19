use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UtcDateTime(jiff::Timestamp);

impl std::ops::Deref for UtcDateTime {
    type Target = jiff::Timestamp;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::fmt::Display for UtcDateTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Default for UtcDateTime {
    fn default() -> Self {
        Self(jiff::Timestamp::now())
    }
}

impl From<jiff::Timestamp> for UtcDateTime {
    fn from(value: jiff::Timestamp) -> Self {
        Self(value)
    }
}

impl From<UtcDateTime> for jiff::Timestamp {
    fn from(value: UtcDateTime) -> Self {
        value.0
    }
}

impl UtcDateTime {
    pub fn now() -> Self {
        Self::default()
    }

    /// The calendar date of this moment in UTC.
    pub fn date(&self) -> jiff::civil::Date {
        jiff::tz::Offset::UTC.to_datetime(self.0).date()
    }
}
