use serde::{Deserialize, Serialize};

use crate::{OptionAsStrExt, structs::persons::Person};

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonSort {
    #[default]
    LastName,
    FirstName,
    Initials,
    Gender,
    PlaceOfResidence,
    UpdatedAt,
}

pub fn compare_persons(a: &Person, b: &Person, sort_field: &PersonSort) -> std::cmp::Ordering {
    match sort_field {
        PersonSort::LastName => a
            .name
            .last_name
            .cmp(&b.name.last_name)
            .then_with(|| {
                a.name
                    .last_name_prefix
                    .as_str_or_empty()
                    .cmp(b.name.last_name_prefix.as_str_or_empty())
            })
            .then_with(|| a.name.initials.cmp(&b.name.initials))
            .then_with(|| a.id.cmp(&b.id)),
        PersonSort::FirstName => a
            .name
            .first_name
            .as_str_or_empty()
            .cmp(b.name.first_name.as_str_or_empty())
            .then_with(|| a.name.last_name.cmp(&b.name.last_name))
            .then_with(|| a.id.cmp(&b.id)),
        PersonSort::Initials => a
            .name
            .initials
            .cmp(&b.name.initials)
            .then_with(|| a.name.last_name.cmp(&b.name.last_name))
            .then_with(|| a.id.cmp(&b.id)),
        PersonSort::Gender => a
            .personal_data
            .gender
            .cmp(&b.personal_data.gender)
            .then_with(|| a.name.last_name.cmp(&b.name.last_name))
            .then_with(|| a.id.cmp(&b.id)),
        PersonSort::PlaceOfResidence => a
            .personal_data
            .place_of_residence
            .as_str_or_empty()
            .cmp(b.personal_data.place_of_residence.as_str_or_empty())
            .then_with(|| a.name.last_name.cmp(&b.name.last_name))
            .then_with(|| a.id.cmp(&b.id)),
        PersonSort::UpdatedAt => a
            .updated_at
            .cmp(&b.updated_at)
            .then_with(|| a.id.cmp(&b.id)),
    }
}

impl PartialOrd for Person {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Person {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name
            .cmp(&other.name)
            .then_with(|| {
                self.personal_data
                    .place_of_residence
                    .cmp(&other.personal_data.place_of_residence)
            })
            .then_with(|| self.id.cmp(&other.id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        structs::{
            common::{Gender, PlaceOfResidence},
            persons::PersonId,
        },
        test_utils::{
            parse_first_name, parse_initials, parse_last_name, parse_last_name_prefix,
            sample_person_with,
        },
    };
    use chrono::{TimeZone, Utc};
    use std::cmp::Ordering;
    use uuid::Uuid;

    fn timestamp(day: u32) -> crate::structs::common::UtcDateTime {
        crate::structs::common::UtcDateTime::from(
            Utc.with_ymd_and_hms(2026, 1, day, 0, 0, 0)
                .single()
                .unwrap(),
        )
    }

    fn person_with_id(id: u128) -> Person {
        sample_person_with(
            PersonId::from(Uuid::from_u128(id)),
            None,
            "Smith",
            None,
            "A.B.",
        )
    }

    #[test]
    fn compare_last_name_uses_prefix_initials_and_id_tiebreakers() {
        let mut a = person_with_id(1);
        let mut b = person_with_id(2);

        a.name.last_name = parse_last_name("Adams");
        b.name.last_name = parse_last_name("Brown");
        assert_eq!(
            compare_persons(&a, &b, &PersonSort::LastName),
            Ordering::Less
        );

        b.name.last_name = parse_last_name("Adams");
        a.name.last_name_prefix = None;
        b.name.last_name_prefix = Some(parse_last_name_prefix("van"));
        assert_eq!(
            compare_persons(&a, &b, &PersonSort::LastName),
            Ordering::Less
        );

        b.name.last_name_prefix = None;
        a.name.initials = Some(parse_initials("A.A."));
        b.name.initials = Some(parse_initials("B.B."));
        assert_eq!(
            compare_persons(&a, &b, &PersonSort::LastName),
            Ordering::Less
        );

        b.name.initials = Some(parse_initials("A.A."));
        assert_eq!(
            compare_persons(&a, &b, &PersonSort::LastName),
            Ordering::Less
        );
    }

    #[test]
    fn compare_first_name_then_last_name_and_id() {
        let mut a = person_with_id(1);
        let mut b = person_with_id(2);

        a.name.first_name = None;
        b.name.first_name = Some(parse_first_name("Adam"));
        a.name.last_name = parse_last_name("Zulu");
        b.name.last_name = parse_last_name("Alpha");
        assert_eq!(
            compare_persons(&a, &b, &PersonSort::FirstName),
            Ordering::Less
        );

        a.name.first_name = Some(parse_first_name("Bob"));
        b.name.first_name = Some(parse_first_name("Bob"));
        a.name.last_name = parse_last_name("Alpha");
        b.name.last_name = parse_last_name("Zulu");
        assert_eq!(
            compare_persons(&a, &b, &PersonSort::FirstName),
            Ordering::Less
        );

        b.name.last_name = parse_last_name("Alpha");
        assert_eq!(
            compare_persons(&a, &b, &PersonSort::FirstName),
            Ordering::Less
        );
    }

    #[test]
    fn compare_initials_then_last_name_and_id() {
        let mut a = person_with_id(1);
        let mut b = person_with_id(2);

        a.name.initials = Some(parse_initials("A.A."));
        b.name.initials = Some(parse_initials("B.B."));
        a.name.last_name = parse_last_name("Zulu");
        b.name.last_name = parse_last_name("Alpha");
        assert_eq!(
            compare_persons(&a, &b, &PersonSort::Initials),
            Ordering::Less
        );

        b.name.initials = Some(parse_initials("A.A."));
        a.name.last_name = parse_last_name("Alpha");
        b.name.last_name = parse_last_name("Zulu");
        assert_eq!(
            compare_persons(&a, &b, &PersonSort::Initials),
            Ordering::Less
        );

        b.name.last_name = parse_last_name("Alpha");
        assert_eq!(
            compare_persons(&a, &b, &PersonSort::Initials),
            Ordering::Less
        );
    }

    #[test]
    fn compare_gender_then_last_name_and_id() {
        let mut a = person_with_id(1);
        let mut b = person_with_id(2);

        a.personal_data.gender = None;
        b.personal_data.gender = Some(Gender::Female);
        a.name.last_name = parse_last_name("Zulu");
        b.name.last_name = parse_last_name("Alpha");
        assert_eq!(compare_persons(&a, &b, &PersonSort::Gender), Ordering::Less);

        a.personal_data.gender = Some(Gender::Female);
        b.personal_data.gender = Some(Gender::Male);
        a.name.last_name = parse_last_name("Zulu");
        b.name.last_name = parse_last_name("Alpha");
        assert_eq!(compare_persons(&a, &b, &PersonSort::Gender), Ordering::Less);

        a.personal_data.gender = Some(Gender::Female);
        b.personal_data.gender = Some(Gender::Female);
        a.name.last_name = parse_last_name("Alpha");
        b.name.last_name = parse_last_name("Zulu");
        assert_eq!(compare_persons(&a, &b, &PersonSort::Gender), Ordering::Less);
    }

    #[test]
    fn compare_place_of_residence_then_last_name_and_id() {
        let mut a = person_with_id(1);
        let mut b = person_with_id(2);

        a.personal_data.place_of_residence = None;
        b.personal_data.place_of_residence = Some(PlaceOfResidence::Known("Utrecht".to_string()));
        a.name.last_name = parse_last_name("Zulu");
        b.name.last_name = parse_last_name("Alpha");
        assert_eq!(
            compare_persons(&a, &b, &PersonSort::PlaceOfResidence),
            Ordering::Less
        );

        a.personal_data.place_of_residence = Some(PlaceOfResidence::Known("Amsterdam".to_string()));
        b.personal_data.place_of_residence = Some(PlaceOfResidence::Known("Amsterdam".to_string()));
        a.name.last_name = parse_last_name("Alpha");
        b.name.last_name = parse_last_name("Zulu");
        assert_eq!(
            compare_persons(&a, &b, &PersonSort::PlaceOfResidence),
            Ordering::Less
        );

        b.name.last_name = parse_last_name("Alpha");
        assert_eq!(
            compare_persons(&a, &b, &PersonSort::PlaceOfResidence),
            Ordering::Less
        );
    }

    #[test]
    fn compare_created_at_and_updated_at_use_id_tiebreaker() {
        let mut a = person_with_id(1);
        let mut b = person_with_id(2);

        a.updated_at = timestamp(3);
        b.updated_at = timestamp(4);
        assert_eq!(
            compare_persons(&a, &b, &PersonSort::UpdatedAt),
            Ordering::Less
        );

        b.updated_at = timestamp(3);
        assert_eq!(
            compare_persons(&a, &b, &PersonSort::UpdatedAt),
            Ordering::Less
        );
    }
}
