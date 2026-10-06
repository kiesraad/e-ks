//! Model OSV 3-2: Publicatie kandidatenlijsten.
//! This model is Dutch-only; the document text lives in the `templates/osv3-2.md`
//! Markdown template.

use textris_pdf::build::Textris;

use super::{
    Pdf,
    inputs::{DistrictLists, ValidList},
    layout::markdown_document,
    markdown::{filters, model_template},
};
use crate::{AppError, core::AnyLocale, models::inputs::PublicSession, structs::persons::Person};

#[derive(Debug)]
pub struct OSV3_2 {
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

model_template!(OSV3_2Template, OSV3_2, "models/templates/osv3-2.md");

impl Pdf for OSV3_2 {
    fn document(&self) -> Result<Textris, AppError> {
        markdown_document(OSV3_2Template(self))
    }

    fn filename(&self) -> String {
        "OSV_3-2_publicatie_kandidatenlijsten.pdf".to_string()
    }
}
