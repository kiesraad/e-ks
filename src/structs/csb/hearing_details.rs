use chrono::{Local, NaiveDateTime};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct HearingDetails {
    pub date_time: NaiveDateTime,
    pub members: Vec<String>,
}

impl Default for HearingDetails {
    fn default() -> Self {
        Self {
            date_time: Local::now().naive_local(),
            members: Default::default(),
        }
    }
}
