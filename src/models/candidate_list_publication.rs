//! Model: Publicatie kandidatenlijsten. This model is Dutch-only; the document
//! text lives in the `templates/candidate-list-publication.md` Markdown
//! template.

use chrono::NaiveDate;
use textris_pdf::build::Textris;

use super::{
    Pdf,
    inputs::{DistrictLists, ValidList},
    layout::markdown_document,
    markdown::{filters, model_template},
};
use crate::{
    AppError,
    core::{AnyLocale, ModelLocale},
    structs::{csb::HearingDetails, persons::Person},
};

#[derive(Debug)]
pub struct CandidateListPublication {
    pub election_name: String,
    pub election_date: NaiveDate,
    /// Per district, the lists in list number order.
    pub valid_lists: Vec<DistrictLists<NumberedList>>,
    /// The configured location of the public session
    pub session_location: String,
    pub public_session: HearingDetails,
}

/// A valid list with its group's list number, the same in every district;
/// numbers of groups without a valid list in the district are skipped.
#[derive(Debug)]
pub struct NumberedList {
    pub number: usize,
    pub list: ValidList<PublishedCandidate>,
}

/// A candidate row, with the name as printed on the candidate list.
#[derive(Debug)]
pub struct PublishedCandidate {
    pub position: usize,
    /// E.g. `Kierkegaard, G.J. (Geertruda Johanna) (v)`.
    pub name: String,
    pub locality: String,
}

impl PublishedCandidate {
    pub fn new(position: usize, person: &Person) -> Self {
        Self {
            position,
            name: person.name_as_printed_on_list(AnyLocale::Nl),
            locality: person.personal_data.locality().unwrap_or_default(),
        }
    }
}

model_template!(
    CandidateListPublicationTemplate,
    CandidateListPublication,
    "models/templates/candidate-list-publication.md"
);

impl Pdf for CandidateListPublication {
    fn document(&self) -> Result<Textris, AppError> {
        markdown_document(CandidateListPublicationTemplate(self), ModelLocale::Nl)
    }

    fn filename(&self) -> String {
        "Publicatie_Kandidatenlijsten.pdf".to_string()
    }
}
