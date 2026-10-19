//! Proptest generators shared by the property tests.
//!
//! Valid domain values are built from raw strings filtered through the real
//! parsers, so validity is whatever production code accepts.

use std::{fmt::Debug, str::FromStr, sync::LazyLock};

use proptest::{
    collection::vec,
    option,
    prelude::*,
    sample::select,
    test_runner::{Config, TestCaseError},
};

use crate::{
    form::is_teletex_char,
    structs::{
        common::{
            Address, Appellation, BsnOrNoneConfirmed, COUNTRY_CODES, CountryCode, DateOfBirth,
            DutchAddress, FirstName, FullName, Gender, HouseNumber, HouseNumberAddition, Initials,
            InternationalAddress, InternationalPostalCode, LastName, LastNamePrefix, Locality,
            PlaceOfResidence, PostalCode, StateOrProvince, StreetName,
        },
        list_submitters::{ListSubmitter, ListSubmitterId},
        persons::{Person, PersonId, PersonalData, Representative},
    },
};

/// [`proptest_config`](crate::test_utils::proptest_config) for properties
/// over [`parsed`] generators: their rejections add up over a run, so the cap
/// scales with the case count.
pub fn parsed_config(default: u32) -> Config {
    let config = crate::test_utils::proptest_config(default);
    Config {
        max_local_rejects: config.cases.saturating_mul(1000),
        ..config
    }
}

/// Parse `input`; when accepted, its canonical spelling (`show`) re-parses to
/// the same value and spelling. Returns the accepted value.
pub fn canonical<T: PartialEq + Debug, E>(
    input: &str,
    parse: impl Fn(&str) -> Result<T, E>,
    show: impl Fn(&T) -> String,
) -> Result<Option<T>, TestCaseError> {
    let Ok(value) = parse(input) else {
        return Ok(None);
    };
    let spelling = show(&value);
    let Ok(again) = parse(&spelling) else {
        return Err(TestCaseError::fail(format!(
            "{input:?} parses, but its canonical spelling {spelling:?} does not"
        )));
    };
    prop_assert_eq!(&again, &value, "{:?} re-parses differently", spelling);
    prop_assert_eq!(show(&again), spelling);
    Ok(Some(value))
}

/// [`canonical`] for a `FromStr + Display` type.
pub fn canonical_display<T>(input: &str) -> Result<Option<T>, TestCaseError>
where
    T: FromStr + PartialEq + Debug + std::fmt::Display,
{
    canonical(input, str::parse::<T>, ToString::to_string)
}

/// All ASCII punctuation characters.
pub const ASCII_PUNCTUATION: &str = r##"!"#$%&'()*+,-./:;<=>?@[\]^_`{|}~"##;

/// Fragments that are syntax or otherwise hazardous somewhere: Markdown
/// dialect markers, XML, line breaks, control and format characters,
/// combining marks and Latin-1 / Latin Extended-A teletex characters.
const DANGEROUS: &[&str] = &[
    "<br>",
    "<br/>",
    "#",
    "# ",
    "### ",
    "- ",
    "* ",
    "+ ",
    "1. ",
    "a. ",
    "z. ",
    "> ",
    "| ",
    "|",
    "\\",
    "\\\\",
    "`",
    "```",
    "*",
    "**",
    "***",
    "_",
    "___",
    "___(12)",
    "[#x]",
    "![",
    "~~",
    "{",
    "{ a = 1 }",
    "@",
    "@pagebreak",
    "+++",
    "---",
    "===",
    "&amp;",
    "&#38;",
    "<",
    "<b>",
    "]]>",
    "<!--",
    "\"",
    "'",
    "\n",
    "\n\n",
    "\r\n",
    "\r",
    "\t",
    "\0",
    "\u{1}",
    "\u{7}",
    "\u{b}",
    "\u{c}",
    "\u{1b}",
    "\u{7f}",
    "\u{85}",
    "\u{a0}",
    "\u{ad}",
    "\u{2028}",
    "\u{2029}",
    "\u{3000}",
    "\u{feff}",
    "\u{200b}",
    "\u{202e}",
    "\u{301}",
    "\u{308}",
    "\u{327}",
    " ",
    "  ",
    "é",
    "ë",
    "ß",
    "Ø",
    "ñ",
    "ĳ",
    "Ĳ",
    "Ł",
    "œ",
    "ŉ",
    "ĸ",
    "ÿ",
    "ž",
    "'s-",
    "Jan",
    "van",
    "a",
    "1",
    "0",
];

/// Arbitrary Unicode strings of up to `max_chars` characters, including
/// control, format and non-BMP characters.
pub fn unicode_string(max_chars: usize) -> impl Strategy<Value = String> {
    vec(any::<char>(), 0..=max_chars).prop_map(String::from_iter)
}

/// Strings of up to `max_parts` fragments, biased towards [`DANGEROUS`]
/// fragments and ASCII punctuation.
pub fn dangerous_string(max_parts: usize) -> impl Strategy<Value = String> {
    let punctuation: Vec<char> = ASCII_PUNCTUATION.chars().collect();
    let part = prop_oneof![
        4 => select(DANGEROUS).prop_map(str::to_string),
        3 => select(punctuation).prop_map(String::from),
        2 => any::<char>().prop_map(String::from),
        2 => "[a-zA-Z0-9 ]{1,4}",
    ];
    vec(part, 0..=max_parts).prop_map(|parts| parts.concat())
}

/// [`unicode_string`] or [`dangerous_string`], half each.
pub fn hostile_string(max_len: usize) -> impl Strategy<Value = String> {
    prop_oneof![unicode_string(max_len), dangerous_string(max_len)]
}

/// Every character [`is_teletex_char`] accepts.
static TELETEX_CHARS: LazyLock<Vec<char>> = LazyLock::new(|| {
    (0..=0x024F)
        .filter_map(char::from_u32)
        .filter(|&c| is_teletex_char(c))
        .collect()
});

/// A teletex character, biased towards ASCII punctuation and spaces.
pub fn teletex_char() -> impl Strategy<Value = char> {
    let punctuation: Vec<char> = ASCII_PUNCTUATION.chars().collect();
    prop_oneof![
        3 => select(TELETEX_CHARS.clone()),
        2 => select(punctuation),
        1 => Just(' '),
        2 => proptest::char::range('a', 'z'),
    ]
}

/// Teletex strings of up to `max_chars` characters, optionally with `\n`.
pub fn teletex_string(max_chars: usize, multi_line: bool) -> impl Strategy<Value = String> {
    let ch = if multi_line {
        prop_oneof![9 => teletex_char(), 1 => Just('\n')].boxed()
    } else {
        teletex_char().boxed()
    };
    vec(ch, 0..=max_chars).prop_map(String::from_iter)
}

/// Raw input for a free-text field: mostly teletex text (so the parsers
/// accept a fair share), sometimes hostile, sometimes padded.
pub fn field_input(max_chars: usize) -> impl Strategy<Value = String> {
    prop_oneof![
        6 => teletex_string(max_chars, false),
        1 => teletex_string(max_chars, true),
        2 => hostile_string(max_chars),
        1 => (teletex_string(max_chars, false), "[ \t\n]{0,3}", "[ \t\n]{0,3}")
            .prop_map(|(s, before, after)| format!("{before}{s}{after}")),
    ]
}

/// The `T`s that the real parser makes of `raw` inputs. Boxed, as are the
/// composite strategies below: nested value trees overflow the test stack.
pub fn parsed<T: FromStr + Debug + 'static>(
    raw: impl Strategy<Value = String> + 'static,
) -> BoxedStrategy<T> {
    raw.prop_filter_map("rejected by the parser", |s| s.parse().ok())
        .boxed()
}

/// Common Dutch last name prefixes, in several spellings.
pub const PREFIX_SAMPLES: &[&str] = &[
    "van",
    "van der",
    "van den",
    "van de",
    "de",
    "den",
    "der",
    "het",
    "in 't",
    "'s",
    "'t",
    "d'",
    "op de",
    "ten",
    "ter",
    "von",
    "zu",
    "la",
    "le",
    "du",
    "Van",
    "Van Der",
    "van De",
    "voor in 't",
    "in het",
    "VAN",
    "van  der",
    " van ",
];

/// Localities, misspellings and Caribbean places.
pub const LOCALITY_SAMPLES: &[&str] = &[
    "Juinen",
    "Utrecht",
    "'s-Gravenhage",
    "Den Haag",
    "den bosch",
    "St. Agatha",
    "Cuyk",
    "Berltsum",
    "Berlikum",
    "Kralendijk",
    "Saba",
    "Sint Eustatius",
    "Rincon",
    "Leeuwarden",
    "Ljouwert",
    "IJsselstein",
    "Nergenshuizen",
];

pub fn last_name() -> impl Strategy<Value = LastName> {
    parsed(field_input(30))
}

pub fn first_name() -> impl Strategy<Value = FirstName> {
    parsed(field_input(30))
}

pub fn last_name_prefix() -> impl Strategy<Value = LastNamePrefix> {
    parsed(select(PREFIX_SAMPLES).prop_map(str::to_string))
}

/// Raw initials input: letters with or without dots and separators.
pub fn initials_input() -> impl Strategy<Value = String> {
    let initial = prop_oneof![
        3 => proptest::char::range('A', 'Z'),
        1 => teletex_char(),
    ];
    let part = (
        initial,
        any::<bool>(),
        select(&["", " ", "  ", "\t", "\n"][..]),
    );
    vec(part, 0..8).prop_map(|parts| {
        parts
            .into_iter()
            .map(|(c, dot, sep)| format!("{c}{}{sep}", if dot { "." } else { "" }))
            .collect()
    })
}

pub fn initials() -> impl Strategy<Value = Initials> {
    parsed(initials_input())
}

pub fn full_name() -> BoxedStrategy<FullName> {
    (
        option::of(first_name()),
        last_name(),
        option::of(last_name_prefix()),
        option::of(initials()),
    )
        .prop_map(
            |(first_name, last_name, last_name_prefix, initials)| FullName {
                first_name,
                last_name,
                last_name_prefix,
                initials,
            },
        )
        .boxed()
}

pub fn appellation() -> impl Strategy<Value = Appellation> {
    parsed(field_input(40))
}

pub fn locality() -> impl Strategy<Value = Locality> {
    parsed(prop_oneof![
        3 => field_input(30),
        1 => select(LOCALITY_SAMPLES).prop_map(str::to_string),
    ])
}

/// Expensive: every parse runs a fuzzy BAG lookup.
pub fn place_of_residence() -> impl Strategy<Value = PlaceOfResidence> {
    parsed(prop_oneof![
        2 => field_input(20),
        1 => select(LOCALITY_SAMPLES).prop_map(str::to_string),
    ])
}

pub fn street_name() -> impl Strategy<Value = StreetName> {
    parsed(field_input(30))
}

pub fn house_number() -> impl Strategy<Value = HouseNumber> {
    parsed("[ ]?[0-9]{1,7}[ ]?")
}

pub fn house_number_addition() -> impl Strategy<Value = HouseNumberAddition> {
    parsed("[ ]?[A-Za-z0-9-]{1,4}[ ]?")
}

pub fn postal_code() -> impl Strategy<Value = PostalCode> {
    parsed("[ ]?[0-9]{2} ?[0-9]{2} ?[a-zA-Z] ?[a-zA-Z][ ]?")
}

pub fn international_postal_code() -> impl Strategy<Value = InternationalPostalCode> {
    parsed(field_input(20))
}

pub fn state_or_province() -> impl Strategy<Value = StateOrProvince> {
    parsed(field_input(30))
}

pub fn country_code() -> impl Strategy<Value = CountryCode> {
    parsed(prop_oneof![
        select(COUNTRY_CODES.to_vec()).prop_map(str::to_string),
        select(COUNTRY_CODES.to_vec()).prop_map(str::to_lowercase),
    ])
}

/// A past date of birth, in each of the accepted spellings.
pub fn date_of_birth() -> impl Strategy<Value = DateOfBirth> {
    let spelling =
        (1900u32..=2020, 1u32..=12, 1u32..=31, 0usize..3).prop_map(|(y, m, d, f)| match f {
            0 => format!("{d}-{m}-{y}"),
            1 => format!("{y:04}-{m:02}-{d:02}"),
            _ => format!("{m}/{d}/{y}"),
        });
    parsed(spelling)
}

pub fn gender() -> impl Strategy<Value = Gender> {
    parsed(
        select(&["female", "male", "V", "m", "vrouw", "man", "frou"][..]).prop_map(str::to_string),
    )
}

pub fn bsn_or_none_confirmed() -> impl Strategy<Value = BsnOrNoneConfirmed> {
    parsed(prop_oneof![
        9 => "[0-9]{9}",
        1 => Just("none-confirmed".to_string()),
    ])
}

pub fn dutch_address() -> BoxedStrategy<DutchAddress> {
    (
        option::of(street_name()),
        option::of(house_number()),
        option::of(house_number_addition()),
        option::of(locality()),
        option::of(postal_code()),
        option::of(any::<bool>()),
    )
        .prop_map(
            |(street_name, house_number, house_number_addition, locality, postal_code, known)| {
                DutchAddress {
                    street_name,
                    house_number,
                    house_number_addition,
                    locality,
                    postal_code,
                    known_in_bag: known,
                }
            },
        )
        .boxed()
}

pub fn international_address() -> BoxedStrategy<InternationalAddress> {
    (
        option::of(street_name()),
        option::of(house_number()),
        option::of(house_number_addition()),
        option::of(locality()),
        option::of(state_or_province()),
        option::of(international_postal_code()),
        option::of(country_code()),
    )
        .prop_map(
            |(
                street_name,
                house_number,
                house_number_addition,
                locality,
                state_or_province,
                postal_code,
                country,
            )| InternationalAddress {
                street_name,
                house_number,
                house_number_addition,
                locality,
                state_or_province,
                postal_code,
                country,
            },
        )
        .boxed()
}

pub fn address() -> BoxedStrategy<Address> {
    prop_oneof![
        dutch_address().prop_map(Address::Dutch),
        international_address().prop_map(Address::International),
    ]
    .boxed()
}

/// Personal data complete enough for the EML exports: a place of residence
/// and a country (NL most of the time).
pub fn personal_data() -> BoxedStrategy<PersonalData> {
    (
        option::of(gender()),
        option::of(bsn_or_none_confirmed()),
        option::of(date_of_birth()),
        place_of_residence(),
        prop_oneof![
            3 => Just("NL".parse::<CountryCode>().expect("NL")),
            1 => country_code(),
        ],
    )
        .prop_map(
            |(gender, bsn, date_of_birth, place_of_residence, country)| PersonalData {
                gender,
                bsn,
                date_of_birth,
                place_of_residence: Some(place_of_residence),
                country: Some(country),
            },
        )
        .boxed()
}

pub fn representative() -> BoxedStrategy<Representative> {
    (full_name(), dutch_address())
        .prop_map(|(name, address)| Representative { name, address })
        .boxed()
}

pub fn person() -> BoxedStrategy<Person> {
    (
        any::<u128>(),
        full_name(),
        personal_data(),
        dutch_address(),
        option::of(representative()),
    )
        .prop_map(
            |(id, name, personal_data, address, representative)| Person {
                id: PersonId::from(uuid::Uuid::from_u128(id)),
                name,
                personal_data,
                address,
                representative: representative.map(Box::new),
                ..Person::default()
            },
        )
        .boxed()
}

pub fn list_submitter() -> BoxedStrategy<ListSubmitter> {
    (any::<u128>(), full_name(), address())
        .prop_map(|(id, name, address)| ListSubmitter {
            id: ListSubmitterId::from(uuid::Uuid::from_u128(id)),
            name,
            address,
            is_substitute: false,
        })
        .boxed()
}
