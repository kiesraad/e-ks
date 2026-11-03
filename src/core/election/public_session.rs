use chrono::NaiveDateTime;
use serde::{Serialize, Serializer, ser::SerializeStruct};

use crate::core::constants::{DEFAULT_DATE_FORMAT, DEFAULT_TIME_FORMAT};

/// The derived `Default` uses the Unix epoch as datetime.
#[derive(Debug, Clone, Copy, Default)]
pub struct PublicSession {
    pub location: &'static str,
    pub street_name: &'static str,
    pub house_number: usize,
    pub postal_code: &'static str,
    pub datetime: NaiveDateTime,
    pub chair: &'static str,
    pub members: &'static [&'static str],
}

impl PublicSession {
    pub fn formatted_date(&self) -> String {
        self.datetime.format(DEFAULT_DATE_FORMAT).to_string()
    }

    pub fn formatted_time(&self) -> String {
        self.datetime.format(DEFAULT_TIME_FORMAT).to_string()
    }
}

impl Serialize for PublicSession {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("PublicSession", 5)?;
        s.serialize_field("location", self.location)?;
        s.serialize_field("date", &self.formatted_date())?;
        s.serialize_field("time", &self.formatted_time())?;
        s.serialize_field("chair", self.chair)?;
        s.serialize_field("members", self.members)?;
        s.end()
    }
}
