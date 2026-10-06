//! Model: Overzicht kandidatenlijsten.
//! This model is Dutch-only; the document text lives in the `templates/osv3-4.md`
//! Markdown template.

use std::collections::HashMap;

use textris_pdf::build::Textris;

use super::{Pdf, layout::markdown_document, markdown::filters, markdown::model_template};
use crate::{AppError, ElectoralDistrict};

#[derive(Debug)]
pub struct CandidateListSummary {
    pub election_name: String,
    pub election_date: String,
    pub electoral_districts: Vec<ElectoralDistrict>,
    pub lists: HashMap<String, Vec<Vec<ElectoralDistrict>>>,
}

impl CandidateListSummary {
    fn active_districts(&self, list: &String) -> Option<Vec<ElectoralDistrict>> {
        self.lists
            .get(list)
            .map(|batched_districts| batched_districts.iter().cloned().flatten().collect())
    }
}

model_template!(
    CandidateListSummaryTemplate,
    CandidateListSummary,
    "models/templates/candidate-list-summary.md"
);

impl Pdf for CandidateListSummary {
    fn document(&self) -> Result<Textris, AppError> {
        markdown_document(CandidateListSummaryTemplate(self))
    }

    fn filename(&self) -> String {
        "OSV_3-4_overzicht_kandidatenlijsten.pdf".to_string()
    }
}
