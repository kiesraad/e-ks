use textris_pdf::build::Textris;

use crate::{
    AppError,
    models::{
        Pdf,
        candidate_list_publication::NumberedList,
        layout::eml_hash_document,
        markdown::{filters, model_template},
    },
};

#[derive(Debug)]
pub struct CandidateListPerDistrict {
    pub election_name: String,
    pub election_date: String,
    pub electoral_district: String,
    pub lists: Vec<NumberedList>,
    /// File name without extension, shared with the district's EML 230b, e.g.
    /// `Kandidatenlijsten_EK2027_Fryslan`
    pub file_stem: String,
    /// Hash of the district's EML 230b; none when the district has no 230b
    pub hash: Option<String>,
}

model_template!(
    CandidateListPerDistrictTemplate,
    CandidateListPerDistrict,
    "models/templates/candidate-list-per-district.md"
);

impl Pdf for CandidateListPerDistrict {
    fn document(&self) -> Result<Textris, AppError> {
        eml_hash_document(CandidateListPerDistrictTemplate(self), self.hash.as_deref())
    }

    fn filename(&self) -> String {
        format!("{}.pdf", self.file_stem)
    }
}
