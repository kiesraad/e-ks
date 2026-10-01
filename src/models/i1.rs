//! Model I 1: Proces-verbaal van het onderzoek naar de kandidatenlijsten.
//! This model is Dutch-only; unlike I 4 it only reports the omissions found
//! per list, it does not reproduce the candidate lists themselves. The
//! document text lives in the `templates/i1.md` Markdown template.

use textris_pdf::build::Textris;

use super::{
    Pdf,
    inputs::{DistrictLists, OmissionGroup, PublicSession},
    layout::markdown_document,
    markdown::{filters, model_template},
};
use crate::AppError;

#[derive(Debug)]
pub struct I1 {
    pub election_name: String,
    pub election_date: String,
    /// The session in which the central voting bureau examined the lists.
    pub session: PublicSession,
    /// The lists that were submitted, one table per electoral district.
    pub submitted_lists: Vec<DistrictLists<SubmittedList>>,
    /// Empty when the examination found no omissions.
    pub found_omissions: Vec<OmissionGroup>,
}

/// One row of a district table. The row number is the position in
/// [`DistrictLists::lists`], so the template can number the rows itself.
#[derive(Debug)]
pub struct SubmittedList {
    /// The appellation printed above the candidate list.
    pub appellation: String,
    /// Name and initials of the first candidate, e.g. `van Dijk, A.B. (Anne)`.
    /// Empty when the list has no candidates.
    pub first_candidate_name: String,
    /// The number of names on the list.
    pub candidate_count: usize,
}

model_template!(I1Template, I1, "models/templates/i1.md");

impl Pdf for I1 {
    fn document(&self) -> Result<Textris, AppError> {
        markdown_document(I1Template(self))
    }

    fn filename(&self) -> String {
        "i1-proces-verbaal.pdf".to_string()
    }
}
