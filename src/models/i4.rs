//! Model I 4: Proces-verbaal over geldigheid en nummering kandidatenlijsten.
//! This model is Dutch-only; the document text lives in the `templates/i4.md`
//! Markdown template.

use textris_pdf::build::Textris;

use super::{
    Pdf,
    layout::markdown_document,
    markdown::{filters, model_template},
};
use crate::{AppError, core::election};

#[derive(Debug)]
pub struct I4 {
    pub election_name: String,
    pub election_date: String,
    pub public_session: PublicSession,
    pub found_omissions: Vec<OmissionGroup>,
    pub recovered_omissions: Vec<OmissionGroup>,
    pub invalid_lists: Vec<OmissionGroup>,
    pub removed_candidates: Vec<RemovedCandidates>,
    pub removed_appellations: Vec<RemovedAppellation>,
    pub corrected_appellations: Vec<CorrectedAppellation>,
    pub valid_lists: Vec<DistrictLists>,
    pub numbered_based_on_votes: Vec<NumberedOnVotes>,
    pub numbered_based_on_districts: Vec<NumberedOnDistricts>,
    /// `None`: room to write during the session; empty: no objections raised.
    pub objections: Option<Vec<String>>,
    pub response_objections: Option<String>,
}

#[derive(Debug)]
pub struct PublicSession {
    pub location: String,
    pub date: String,
    pub time: String,
    pub chair: String,
    pub members: Vec<String>,
}

impl From<election::PublicSession> for PublicSession {
    fn from(session: election::PublicSession) -> Self {
        PublicSession {
            location: session.location.to_string(),
            date: session.formatted_date(),
            time: session.formatted_time(),
            chair: session.chair.to_string(),
            members: session.members.iter().map(ToString::to_string).collect(),
        }
    }
}

/// Omissions for one list, identified by its appellation and district(s).
#[derive(Debug)]
pub struct OmissionGroup {
    pub appellation: String,
    pub electoral_district: String,
    pub omission_descriptions: Vec<String>,
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
pub struct DistrictLists {
    pub electoral_district: String,
    pub lists: Vec<ValidList>,
}

#[derive(Debug, Clone)]
pub struct ValidList {
    pub appellation: String,
    pub candidates: Vec<ValidListCandidate>,
}

#[derive(Debug, Clone)]
pub struct ValidListCandidate {
    pub last_name: String,
    pub initials: String,
    pub locality: String,
    pub position: usize,
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
        markdown_document(I4Template(self))
    }

    fn filename(&self) -> String {
        "i4-proces-verbaal.pdf".to_string()
    }
}
