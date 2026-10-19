//! Property tests for the hearing date and time parsers: no input panics and
//! an accepted value is canonical (its formatted spelling re-parses to it).

use chrono::{NaiveDate, NaiveTime};
use proptest::prelude::*;

use super::{DateOfHearing, TimeOfHearing};
use crate::test_support::strategies::{canonical, hostile_string};

proptest! {
    #![proptest_config(crate::test_utils::proptest_config(2000))]

    #[test]
    fn date_of_hearing(
        s in prop_oneof![
            hostile_string(14),
            "[ ]?[0-9]{1,2}-[0-9]{1,2}-[0-9]{4}[ ]?",
            "(0|1|2|3)?[0-9]-(0|1)?[0-9]-(19|20|99|00)[0-9]{2}",
            "[0-9٠-٩]{1,2}-[0-9٠-٩]{1,2}-[0-9٠-٩]{4}",
        ]
    ) {
        let parse = |s: &str| s.parse::<DateOfHearing>().map(|date| *date);
        let show = |date: &NaiveDate| DateOfHearing(*date).format();
        canonical(&s, parse, show)?;
    }

    #[test]
    fn time_of_hearing(
        s in prop_oneof![
            hostile_string(8),
            "[ ]?[0-9 ]{1,3}:[0-9 ]{1,3}[ ]?",
            "[0-2][0-9]:[0-6][0-9]",
        ]
    ) {
        let parse = |s: &str| s.parse::<TimeOfHearing>().map(|time| *time);
        let show = |time: &NaiveTime| TimeOfHearing(*time).format();
        canonical(&s, parse, show)?;
    }
}
