//! Property tests for the correction overlay: the field names parse
//! canonically, and a field value submitted as shown is no correction at all.

use proptest::prelude::*;

use super::{CandidateCorrectionField, parse_person_correction};
use crate::{
    structs::{csb::PersonCorrection, persons::Person},
    test_support::strategies::{canonical_display, hostile_string, person},
};

const FIELDS: [CandidateCorrectionField; 5] = [
    CandidateCorrectionField::Initials,
    CandidateCorrectionField::LastNamePrefix,
    CandidateCorrectionField::LastName,
    CandidateCorrectionField::DateOfBirth,
    CandidateCorrectionField::PlaceOfResidence,
];

/// The correction that leaves `field` of `person` as it is, if it has a
/// value the dialog can submit.
fn unchanged(field: CandidateCorrectionField, person: &Person) -> Option<PersonCorrection> {
    let name = person.name.clone();
    let data = person.personal_data.clone();
    match field {
        CandidateCorrectionField::Initials => Some(PersonCorrection::Initials(name.initials)),
        CandidateCorrectionField::LastNamePrefix => {
            Some(PersonCorrection::LastNamePrefix(name.last_name_prefix))
        }
        CandidateCorrectionField::LastName => Some(PersonCorrection::LastName(name.last_name)),
        CandidateCorrectionField::DateOfBirth => {
            data.date_of_birth.map(PersonCorrection::DateOfBirth)
        }
        CandidateCorrectionField::PlaceOfResidence => data
            .place_of_residence
            .map(PersonCorrection::PlaceOfResidence),
    }
}

proptest! {
    #![proptest_config(crate::test_utils::proptest_config(2000))]

    #[test]
    fn correction_field(
        s in prop_oneof![
            hostile_string(20),
            "(initials|last-name-prefix|last-name|date-of-birth|place-of-residence| initials)",
        ]
    ) {
        canonical_display::<CandidateCorrectionField>(&s)?;
    }

    #[test]
    fn any_submitted_value_parses_without_panicking(field in 0usize..5, value in hostile_string(30)) {
        let _ = parse_person_correction(FIELDS[field], &value);
    }
}

proptest! {
    // Generating a person runs fuzzy BAG lookups.
    #![proptest_config(crate::test_support::strategies::parsed_config(300))]

    #[test]
    fn submitting_the_shown_value_changes_nothing(person in person()) {
        for field in FIELDS {
            if let Some(expected) = unchanged(field, &person) {
                let shown = field.extract(&person);
                prop_assert_eq!(parse_person_correction(field, &shown), Ok(expected), "{:?}", field);
            }
        }
    }
}
