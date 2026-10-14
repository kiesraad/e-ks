use textris_pdf::build::Textris;

use crate::{
    AppError,
    models::{
        Pdf,
        candidate_list_publication::NumberedList,
        layout::markdown_document,
        markdown::{filters, model_template},
    },
};

#[derive(Debug)]
pub struct CandidateListPerDistrict {
    pub election_name: String,
    pub election_date: String,
    pub electoral_district: String,
    pub lists: Vec<NumberedList>,
}

model_template!(
    CandidateListPerDistrictTemplate,
    CandidateListPerDistrict,
    "models/templates/candidate-list-per-district.md"
);

impl Pdf for CandidateListPerDistrict {
    fn document(&self) -> Result<Textris, AppError> {
        markdown_document(CandidateListPerDistrictTemplate(self))
    }

    fn filename(&self) -> String {
        format!("kandidatenlijst-{}.pdf", self.electoral_district)
    }
}
