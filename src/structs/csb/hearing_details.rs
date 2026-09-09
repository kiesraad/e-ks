use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
pub struct HearingDetails {
    pub date_time: NaiveDateTime,
    pub members: Vec<String>,
}
