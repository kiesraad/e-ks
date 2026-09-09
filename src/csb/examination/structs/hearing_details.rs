use chrono::NaiveDateTime;

pub struct HearingDetails {
    pub date_time: NaiveDateTime,
    pub members: Vec<String>,
}
