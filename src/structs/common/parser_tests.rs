//! Property tests for the field parsers in `structs::common` and the string
//! validators: no input panics, an accepted value is canonical (its spelling
//! re-parses to itself) and keeps to the documented character and length
//! rules.

use proptest::prelude::*;

use super::{
    Appellation, Bsn, BsnOrNoneConfirmed, COUNTRY_CODES, CountryCode, DateOfBirth, FirstName,
    FormAction, Gender, HouseNumber, HouseNumberAddition, Initials, InternationalPostalCode,
    LastName, LastNamePrefix, LegalName, Locality, PlaceOfResidence, PostalCode,
    PreviousElectionResults, StateOrProvince, StreetName, last_name_prefix::is_last_name_prefix,
};
use crate::{
    form::{
        ValidationError, is_teletex_char, validate_length, validate_multi_line_teletex_chars,
        validate_teletex_chars,
    },
    test_support::strategies::{
        LOCALITY_SAMPLES, PREFIX_SAMPLES, canonical, canonical_display, field_input,
        hostile_string, initials_input,
    },
    utils::locality_aliases::correct_locality_name,
};

/// Hostile or near-valid free text.
fn input(max: usize) -> impl Strategy<Value = String> {
    prop_oneof![hostile_string(max), field_input(max)]
}

/// `input` plus near-valid samples.
fn input_or(samples: &'static [&'static str]) -> impl Strategy<Value = String> {
    prop_oneof![
        2 => input(30),
        1 => proptest::sample::select(samples).prop_map(str::to_string),
        1 => (proptest::sample::select(samples), "[ \t\n\u{a0}]{0,2}", "[ \t\n\u{a0}]{0,2}")
            .prop_map(|(s, before, after)| format!("{before}{s}{after}")),
    ]
}

/// Near-valid dates in the accepted spellings, with Unicode digits mixed in.
fn date_input() -> impl Strategy<Value = String> {
    prop_oneof![
        "[0-9]{1,2}-[0-9]{1,2}-[0-9]{4}",
        "[0-9]{4}-[0-9]{2}-[0-9]{2}",
        "[0-9]{1,2}/[0-9]{1,2}/[0-9]{4}",
        "[0-9٠-٩０-９]{1,2}[-/][0-9٠-٩]{1,2}[-/][0-9٠-٩]{4}",
        "[ ]?(0|1|2|3)?[0-9]-(0|1)?[0-9]-(19|20|21|99|00)[0-9]{2}[ ]?",
        input(12),
    ]
}

fn is_teletex(value: &str) -> bool {
    value.chars().all(is_teletex_char)
}

/// The 11-check, independently: weights 9..=2 and -1, sum divisible by 11.
fn eleven_check(digits: &str) -> bool {
    let sum: i64 = digits
        .chars()
        .zip([9, 8, 7, 6, 5, 4, 3, 2, -1])
        .map(|(c, weight)| i64::from(c.to_digit(10).expect("digit")) * weight)
        .sum();
    sum.rem_euclid(11) == 0
}

/// Single-line constrained free text: the trimmed input, teletex only,
/// 1..=`max` bytes.
fn constrained(input: &str, value: &str, max: usize) -> Result<(), TestCaseError> {
    prop_assert_eq!(value, input.trim());
    prop_assert!(is_teletex(value), "{:?}", value);
    prop_assert!((1..=max).contains(&value.len()), "{:?}", value);
    Ok(())
}

proptest! {
    #![proptest_config(crate::test_utils::proptest_config(2000))]

    #[test]
    fn bsn(s in prop_oneof![input(12), "[ \t]?[0-9]{8,10}[ \t]?", "[0-9]{9}"]) {
        if let Some(bsn) = canonical(&s, str::parse::<Bsn>, Bsn::to_exposed_string)? {
            let digits = bsn.expose();
            prop_assert_eq!(digits, s.trim());
            prop_assert!(digits.len() == 9 && digits.bytes().all(|b| b.is_ascii_digit()));
            prop_assert!(eleven_check(digits));
        }
    }

    #[test]
    fn bsn_accepts_exactly_the_eleven_check(s in "[0-9]{9}") {
        prop_assert_eq!(s.parse::<Bsn>().is_ok(), eleven_check(&s));
    }

    #[test]
    fn bsn_or_none_confirmed(
        s in prop_oneof![input(12), "[0-9]{9}", "[ ]?none-confirmed[ ]?", "[Nn]one-[Cc]onfirmed"]
    ) {
        if let Some(value) = canonical(
            &s,
            str::parse::<BsnOrNoneConfirmed>,
            BsnOrNoneConfirmed::to_exposed_string,
        )? {
            match value {
                BsnOrNoneConfirmed::NoneConfirmed => prop_assert_eq!(s.trim(), "none-confirmed"),
                BsnOrNoneConfirmed::Bsn(bsn) => prop_assert!(eleven_check(bsn.expose())),
            }
        }
    }

    #[test]
    fn postal_code(
        s in prop_oneof![input(10), "[ \u{a0}\t]?[0-9]{4}[ \u{2028}]{0,2}[a-zA-Zß]{2}[ ]?"]
    ) {
        if let Some(code) = canonical_display::<PostalCode>(&s)? {
            let compact: String = s.split_whitespace().collect();
            prop_assert_eq!(code.as_str(), compact.to_ascii_uppercase());
            let bytes = code.as_bytes();
            prop_assert!(
                bytes.len() == 6
                    && bytes[..4].iter().all(u8::is_ascii_digit)
                    && bytes[4..].iter().all(u8::is_ascii_uppercase)
            );
        }
    }

    #[test]
    fn international_postal_code(s in input(24)) {
        if let Some(code) = canonical_display::<InternationalPostalCode>(&s)? {
            constrained(&s, &code, 20)?;
        }
    }

    #[test]
    fn locality(s in input_or(LOCALITY_SAMPLES)) {
        if let Some(locality) = canonical_display::<Locality>(&s)? {
            prop_assert!(is_teletex(&locality));
            prop_assert!((1..=200).contains(&locality.len()));
            prop_assert!(correct_locality_name(&locality).is_none_or(|c| c == locality.as_str()));
        }
    }

    #[test]
    fn last_name(s in input_or(PREFIX_SAMPLES)) {
        if let Some(name) = canonical_display::<LastName>(&s)? {
            constrained(&s, &name, 255)?;
            prop_assert!(name.split_once(' ').is_none_or(|(first, _)| !is_last_name_prefix(first)));
        }
    }

    #[test]
    fn last_name_prefix(s in input_or(PREFIX_SAMPLES)) {
        if let Some(prefix) = canonical_display::<LastNamePrefix>(&s)? {
            prop_assert_eq!(prefix.as_str(), s.trim());
            prop_assert!(is_last_name_prefix(&prefix));
        }
    }

    #[test]
    fn initials(s in prop_oneof![input(20), initials_input()]) {
        if let Some(initials) = canonical_display::<Initials>(&s)? {
            let chars: Vec<char> = initials.chars().collect();
            let mut count = 0;
            let mut i = 0;
            while i < chars.len() {
                prop_assert!(chars[i].is_alphanumeric() && is_teletex_char(chars[i]), "{:?}", initials);
                count += 1;
                i += 1;
                match chars.get(i) {
                    Some('.') => i += 1,
                    Some(' ') => {
                        i += 1;
                        prop_assert!(i < chars.len(), "trailing space in {:?}", initials);
                    }
                    None => {}
                    Some(_) => prop_assert!(false, "unseparated initials in {:?}", initials),
                }
            }
            prop_assert!((1..=100).contains(&count));
        }
    }

    #[test]
    fn first_name(s in input(40)) {
        if let Some(name) = canonical_display::<FirstName>(&s)? {
            constrained(&s, &name, 200)?;
        }
    }

    #[test]
    fn legal_name(s in input(40)) {
        if let Some(name) = canonical_display::<LegalName>(&s)? {
            constrained(&s, &name, 200)?;
        }
    }

    #[test]
    fn street_name(s in input(40)) {
        if let Some(name) = canonical_display::<StreetName>(&s)? {
            constrained(&s, &name, 200)?;
        }
    }

    #[test]
    fn state_or_province(s in input(40)) {
        if let Some(name) = canonical_display::<StateOrProvince>(&s)? {
            constrained(&s, &name, 200)?;
        }
    }

    #[test]
    fn free_text_length_bounds_are_bytes(c in crate::test_support::strategies::teletex_char(), n in 195usize..=205) {
        let s = c.to_string().repeat(n);
        let accepted = s.parse::<FirstName>().is_ok();
        let trimmed = s.trim();
        prop_assert_eq!(accepted, !trimmed.is_empty() && trimmed.len() <= 200);
    }

    #[test]
    fn appellation(s in input(50)) {
        if let Some(appellation) = canonical_display::<Appellation>(&s)? {
            prop_assert!(is_teletex(&appellation));
            prop_assert_eq!(
                appellation.as_str(),
                appellation.split_whitespace().collect::<Vec<_>>().join(" ")
            );
            let chars = appellation.chars().filter(|&c| c != ' ').count();
            prop_assert!((1..=Appellation::MAX_CHAR_COUNT).contains(&chars));
        }
    }

    #[test]
    fn country_code(
        s in prop_oneof![
            input(4),
            proptest::sample::select(COUNTRY_CODES.to_vec()).prop_map(str::to_lowercase),
            "[ ]?[a-zA-Zſıß]{1,2}[ ]?",
        ]
    ) {
        if let Some(code) = canonical_display::<CountryCode>(&s)? {
            prop_assert!(COUNTRY_CODES.contains(&code.as_str()));
        }
    }

    #[test]
    fn date_of_birth(s in date_input()) {
        if let Some(date) = canonical_display::<DateOfBirth>(&s)? {
            prop_assert!(*date <= chrono::Utc::now().date_naive());
        }
    }

    #[test]
    fn gender(s in prop_oneof![input(8), "(?i)(female|male|vrouw|man|frou|f|v|m)"]) {
        if let Some(gender) = canonical_display::<Gender>(&s)? {
            prop_assert!(["female", "male"].contains(&gender.to_string().as_str()));
        }
    }

    #[test]
    fn form_action(s in prop_oneof![input(8), "(save|remove|Save| save)"]) {
        let parse = |s: &str| s.parse::<FormAction>().map(|a| a.to_string());
        if let Some(action) = canonical(&s, parse, Clone::clone)? {
            prop_assert!(["save", "remove"].contains(&action.as_str()));
        }
    }

    #[test]
    fn house_number(s in prop_oneof![input(10), "[ ]?[0-9]{0,9}[ ]?", "[0-9٣]{1,3}"]) {
        if let Some(number) = canonical_display::<HouseNumber>(&s)? {
            prop_assert_eq!(number.as_str(), s.trim());
            prop_assert!((1..=7).contains(&number.len()));
            prop_assert!(number.bytes().all(|b| b.is_ascii_digit()));
        }
    }

    #[test]
    fn house_number_addition(s in prop_oneof![input(6), "[ ]?[A-Za-z0-9ß-]{0,5}[ ]?"]) {
        if let Some(addition) = canonical_display::<HouseNumberAddition>(&s)? {
            prop_assert_eq!(addition.as_str(), s.trim());
            prop_assert!((1..=4).contains(&addition.len()));
            prop_assert!(addition.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
        }
    }

    #[test]
    fn previous_election_results(
        s in prop_oneof![input(8), "(zero_seats|one_to_fifteen_seats|sixteen_or_more_seats)[ ]?"]
    ) {
        canonical_display::<PreviousElectionResults>(&s)?;
    }

    #[test]
    fn validate_length_trims_and_bounds(s in input(20), min in 0usize..8, max in 0usize..24) {
        match validate_length(&s, min, max) {
            Ok(value) => {
                prop_assert_eq!(value.as_str(), s.trim());
                prop_assert!(!value.is_empty() && min <= value.len() && value.len() <= max);
            }
            Err(ValidationError::ValueShouldNotBeEmpty) => prop_assert!(s.trim().is_empty()),
            Err(_) => {
                let len = s.trim().len();
                prop_assert!(len < min || len > max);
            }
        }
    }

    #[test]
    fn teletex_validators(s in input(20)) {
        prop_assert_eq!(validate_teletex_chars(&s).is_ok(), is_teletex(&s));
        prop_assert_eq!(
            validate_multi_line_teletex_chars(&s).is_ok(),
            s.chars().all(|c| c == '\n' || is_teletex_char(c))
        );
    }
}

proptest! {
    // Every parse runs a fuzzy BAG lookup.
    #![proptest_config(crate::test_utils::proptest_config(200))]

    #[test]
    fn place_of_residence(s in input_or(LOCALITY_SAMPLES)) {
        if let Some(place) = canonical_display::<PlaceOfResidence>(&s)? {
            prop_assert!(is_teletex(&place));
            prop_assert!((1..=200).contains(&place.len()));
        }
    }
}
