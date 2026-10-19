//! Property tests for the parsers of the enum and id newtypes and the CSB
//! free-text types: no input panics and an accepted value is canonical (its
//! spelling re-parses to itself).

use proptest::prelude::*;
use uuid::Uuid;

use crate::{
    StreamId,
    form::is_teletex_char,
    structs::{
        candidate_lists::CandidateListId,
        candidates::AddPersonAction,
        csb::{
            HearingModel, ObjectionId, ObjectionText, OmissionId, OmissionText, OmissionTitle,
            OmissionType, RegisteredPoliticalGroupId, SeatCount, VoteCount,
        },
        list_designation::ListDesignation,
        list_submitters::ListSubmitterId,
        name_authorisations::NameAuthorisationId,
        persons::PersonId,
    },
    test_support::strategies::{canonical_display, field_input, hostile_string, teletex_string},
};

fn input(max: usize) -> impl Strategy<Value = String> {
    prop_oneof![hostile_string(max), field_input(max)]
}

/// Every spelling `Uuid::parse_str` accepts, from a random UUID.
fn uuid_input() -> impl Strategy<Value = String> {
    (any::<u128>(), 0usize..6).prop_map(|(n, form)| {
        let id = Uuid::from_u128(n);
        match form {
            0 => id.hyphenated().to_string(),
            1 => id.simple().to_string(),
            2 => id.braced().to_string(),
            3 => id.urn().to_string(),
            4 => id.hyphenated().to_string().to_uppercase(),
            _ => format!(" {id}"),
        }
    })
}

/// Multi-line free text: the input with `\r\n` folded and trimmed, teletex
/// or `\n` only, 1..=`max` bytes.
fn multi_line(input: &str, value: &str, max: usize) -> Result<(), TestCaseError> {
    let folded = input.replace("\r\n", "\n");
    prop_assert_eq!(value, folded.trim());
    prop_assert!(
        value.chars().all(|c| c == '\n' || is_teletex_char(c)),
        "{:?}",
        value
    );
    prop_assert!((1..=max).contains(&value.len()));
    Ok(())
}

/// Near-valid multi-line text: teletex lines joined by `\n` or `\r\n`.
fn multi_line_input(max: usize) -> impl Strategy<Value = String> {
    prop_oneof![
        input(max),
        (teletex_string(max, true), any::<bool>()).prop_map(|(s, crlf)| if crlf {
            s.replace('\n', "\r\n")
        } else {
            s
        }),
    ]
}

/// The canonical spelling of an id newtype: lowercase hyphenated.
macro_rules! id_property {
    ($name:ident, $id:ty) => {
        proptest! {
            #![proptest_config(crate::test_utils::proptest_config(500))]

            #[test]
            fn $name(s in prop_oneof![input(40), uuid_input()]) {
                if let Some(id) = canonical_display::<$id>(&s)? {
                    let spelling = id.to_string();
                    prop_assert_eq!(spelling.len(), 36);
                    prop_assert_eq!(spelling.to_lowercase(), spelling);
                    prop_assert_eq!(Uuid::from(id).to_string(), id.uuid().to_string());
                }
            }
        }
    };
}

id_property!(stream_id, StreamId);
id_property!(person_id, PersonId);
id_property!(name_authorisation_id, NameAuthorisationId);
id_property!(list_submitter_id, ListSubmitterId);
id_property!(candidate_list_id, CandidateListId);
id_property!(objection_id, ObjectionId);
id_property!(omission_id, OmissionId);
id_property!(registered_political_group_id, RegisteredPoliticalGroupId);

proptest! {
    #![proptest_config(crate::test_utils::proptest_config(2000))]

    #[test]
    fn uuid_spellings_name_one_id(n in any::<u128>()) {
        let id = PersonId::from(Uuid::from_u128(n));
        let simple = Uuid::from_u128(n).simple().to_string();
        for spelling in [id.to_string(), simple.clone(), simple.to_uppercase()] {
            prop_assert_eq!(spelling.parse::<PersonId>().ok(), Some(id));
        }
    }

    #[test]
    fn list_designation(s in prop_oneof![input(12), "(standalone|blank|combined|Blank)[ ]?"]) {
        canonical_display::<ListDesignation>(&s)?;
    }

    #[test]
    fn add_person_action(
        s in prop_oneof![input(12), "(add-all|remove-all|)", uuid_input()]
    ) {
        canonical_display::<AddPersonAction>(&s)?;
    }

    #[test]
    fn vote_count(s in prop_oneof![input(12), "[ ]?[0-9]{1,22}[ ]?", "[+-][0-9]{1,3}"]) {
        if let Some(count) = canonical_display::<VoteCount>(&s)? {
            prop_assert_eq!(count.value(), s.trim().parse::<u64>().expect("digits"));
        }
    }

    #[test]
    fn seat_count(s in prop_oneof![input(12), "[ ]?[0-9]{1,12}[ ]?", "[+-][0-9]{1,3}"]) {
        if let Some(count) = canonical_display::<SeatCount>(&s)? {
            prop_assert_eq!(count.value(), s.trim().parse::<u32>().expect("digits"));
        }
    }

    #[test]
    fn omission_text(s in multi_line_input(60)) {
        if let Some(text) = canonical_display::<OmissionText>(&s)? {
            multi_line(&s, &text, 2000)?;
            let placeholder = text.contains(['{', '}']);
            prop_assert!(!placeholder, "{:?}", text);
        }
    }

    #[test]
    fn omission_title(s in input(40)) {
        if let Some(title) = canonical_display::<OmissionTitle>(&s)? {
            prop_assert_eq!(title.as_str(), s.trim());
            prop_assert!(title.chars().all(is_teletex_char));
            prop_assert!((1..=100).contains(&title.len()));
        }
    }

    #[test]
    fn objection_text(s in multi_line_input(60)) {
        if let Some(text) = canonical_display::<ObjectionText>(&s)? {
            multi_line(&s, &text, 8000)?;
        }
    }

    #[test]
    fn omission_type(
        s in prop_oneof![
            input(12),
            "(political-group|appellation|candidate-list|declarations-of-support|candidate)",
        ]
    ) {
        canonical_display::<OmissionType>(&s)?;
    }

    #[test]
    fn hearing_model(s in prop_oneof![input(4), "(i1|i4|I1| i4)"]) {
        canonical_display::<HearingModel>(&s)?;
    }
}
