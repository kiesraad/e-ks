//! Model I 4: Proces-verbaal over geldigheid en nummering kandidatenlijsten.
//! This model is Dutch-only; the document text lives in the `templates/i4.md`
//! Markdown template.

use chrono::NaiveDate;
use textris_pdf::build::Textris;

use super::{
    Pdf,
    inputs::{DistrictLists, OmissionGroup, ValidList},
    markdown::{filters, model_template},
};
use crate::{
    AppError, core::ModelLocale, models::layout::eml_hash_document, structs::csb::HearingDetails,
};

#[derive(Debug, Default)]
pub struct I4 {
    pub election_name: String,
    pub election_date: NaiveDate,
    /// The configured location of the public session
    pub session_location: String,
    pub public_session: HearingDetails,
    pub found_omissions: Vec<OmissionGroup>,
    pub recovered_omissions: Vec<OmissionGroup>,
    pub invalid_lists: Vec<OmissionGroup>,
    pub removed_candidates: Vec<RemovedCandidates>,
    pub removed_appellations: Vec<RemovedAppellation>,
    pub corrected_appellations: Vec<CorrectedAppellation>,
    pub valid_lists: Vec<DistrictLists<ValidList>>,
    pub numbered_based_on_votes: Vec<NumberedOnVotes>,
    pub numbered_based_on_districts: Vec<NumberedOnDistricts>,
    /// Empty: no objections raised.
    pub objections: Vec<String>,
    pub response_objections: Option<String>,
    pub document_status: I4Status,
}

/// `Draft` document does not number lists that still have have to be numbered by lot.
/// The `Final` document gets the hash of the EML 230c in the footer; none when
/// no list qualifies for the 230c.
#[derive(Debug, Default)]
pub enum I4Status {
    #[default]
    Draft,
    Final(Option<String>),
}

impl I4Status {
    pub fn is_draft(&self) -> bool {
        matches!(self, I4Status::Draft)
    }

    pub fn is_final(&self) -> bool {
        matches!(self, I4Status::Final(_))
    }

    fn hash(&self) -> Option<&str> {
        match self {
            I4Status::Final(hash) => hash.as_deref(),
            I4Status::Draft => None,
        }
    }
}

#[derive(Debug)]
pub struct RemovedCandidates {
    pub appellation: String,
    pub electoral_district: String,
    pub candidates: Vec<RemovedCandidate>,
}

#[derive(Debug)]
pub struct RemovedCandidate {
    pub name: String,
    pub reasons: Vec<String>,
}

#[derive(Debug)]
pub struct RemovedAppellation {
    pub appellation: String,
    pub electoral_district: String,
    pub first_candidate_name: String,
    pub reasons: Vec<String>,
}

#[derive(Debug)]
pub struct CorrectedAppellation {
    pub first_candidate_name: String,
    pub electoral_district: String,
    pub submitted_appellation: String,
    pub edited_appellation: String,
}

#[derive(Debug)]
pub struct NumberedOnVotes {
    /// `None` when the number is still to be determined (rendered blank).
    pub position: Option<usize>,
    pub appellation: String,
    pub previous_votes: u64,
}

#[derive(Debug)]
pub struct NumberedOnDistricts {
    /// `None` when the number is still to be determined (rendered blank).
    pub position: Option<usize>,
    pub appellation: String,
    pub districts: u64,
}

model_template!(I4Template, I4, "models/templates/i4.md");

impl Pdf for I4 {
    fn document(&self) -> Result<Textris, AppError> {
        eml_hash_document(
            I4Template(self),
            ModelLocale::Nl,
            self.document_status.hash(),
        )
    }

    fn filename(&self) -> String {
        if self.document_status.is_final() {
            "Model_I4_Proces_Verbaal.pdf"
        } else {
            "Model_I4_Proces_Verbaal_Concept.pdf"
        }
        .to_string()
    }
}
