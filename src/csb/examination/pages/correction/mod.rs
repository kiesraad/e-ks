mod structs;
mod views;
use serde::Deserialize;

use crate::{
    csb::examination::extractors::CsbPoliticalGroup,
    form::ValidationError,
    structs::{
        candidate_lists::CandidateListId,
        common::{DateOfBirth, Initials, LastName, LastNamePrefix, PlaceOfResidence},
        csb::PersonCorrection,
        persons::PersonId,
    },
};

pub use views::{
    appellation_correction_submit, appellation_name_correction, person_correction,
    person_correction_submit,
};

pub use crate::csb::examination::structs::CandidateCorrectionField;

/// Form backing the correction overlay. A single free-text value is submitted
/// and validated as the appropriate typed value in the handler.
/// The `date_of_birth` alias allows the date-input JS to submit under its
/// native field name while the handler reads it as `value`.
#[derive(Deserialize, Debug, Default)]
pub struct CorrectionForm {
    #[serde(alias = "date_of_birth")]
    pub value: String,
}

/// The return path after closing or saving in the correction overlay:
/// the candidate detail page when a list is known, otherwise the examination
/// overview for the political group.
fn return_path(
    political_group: &CsbPoliticalGroup,
    person_id: PersonId,
    list: Option<CandidateListId>,
) -> String {
    // TODO handle case where user is coming from the all omission page (#897)
    match list {
        Some(list_id) => political_group
            .candidate_path(&list_id, &person_id)
            .to_string(),
        None => political_group.group_path().to_string(),
    }
}

/// Parse the submitted form value into the appropriate `PersonCorrection`
/// variant for the given field.
fn parse_person_correction(
    field: CandidateCorrectionField,
    value: &str,
) -> Result<PersonCorrection, ValidationError> {
    match field {
        CandidateCorrectionField::Initials => {
            value.parse::<Initials>().map(PersonCorrection::Initials)
        }
        // Empty clears the prefix rather than failing to parse.
        CandidateCorrectionField::LastNamePrefix => match value.trim() {
            "" => Ok(PersonCorrection::LastNamePrefix(None)),
            prefix => prefix
                .parse::<LastNamePrefix>()
                .map(|prefix| PersonCorrection::LastNamePrefix(Some(prefix))),
        },
        CandidateCorrectionField::LastName => {
            value.parse::<LastName>().map(PersonCorrection::LastName)
        }
        CandidateCorrectionField::DateOfBirth => value
            .parse::<DateOfBirth>()
            .map(PersonCorrection::DateOfBirth),
        CandidateCorrectionField::PlaceOfResidence => value
            .parse::<PlaceOfResidence>()
            .map(PersonCorrection::PlaceOfResidence),
    }
}
