//! Model: Publicatie kandidatenlijsten. This model is Dutch-only; the document
//! text lives in the `templates/candidate-list-publication.md` Markdown
//! template.

use textris_pdf::build::Textris;

use super::{
    Pdf,
    inputs::{DistrictLists, ValidList},
    layout::markdown_document,
    markdown::{filters, model_template},
};
use crate::{AppError, core::AnyLocale, models::inputs::PublicSession, structs::persons::Person};

#[derive(Debug)]
pub struct CandidateListPublication {
    pub election_name: String,
    pub election_date: String,
    /// Per district, the lists in list number order.
    pub valid_lists: Vec<DistrictLists<NumberedList>>,
    pub public_session: PublicSession,
}

/// A valid list with its number in the district: the lists of a district
/// are numbered sequentially, without gaps.
#[derive(Debug)]
pub struct NumberedList {
    pub number: usize,
    pub list: ValidList<PublishedCandidate>,
}

/// A candidate row, with the name as printed on the candidate list.
#[derive(Debug, Clone)]
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
        markdown_document(CandidateListPublicationTemplate(self))
    }

    fn filename(&self) -> String {
        "publicatie-kandidatenlijsten.pdf".to_string()
    }
}
