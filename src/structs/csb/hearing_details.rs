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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hearing_details_gets_current_time() {
        let hearing_details = HearingDetails::default();
        let now = Local::now().naive_local();

        assert!(
            // Give a little leeway
            now.signed_duration_since(hearing_details.date_time)
                .num_milliseconds()
                < 20
        );
    }
}
