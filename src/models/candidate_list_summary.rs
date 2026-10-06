//! Model: Overzicht kandidatenlijsten.
//! This model is Dutch-only; the document text lives in the `templates/osv3-4.md`
//! Markdown template.

use std::collections::BTreeMap;

use textris_pdf::build::Textris;

use super::{Pdf, layout::markdown_document, markdown::filters, markdown::model_template};
use crate::{AppError, ElectoralDistrict};

#[derive(Debug)]
pub struct CandidateListSummary {
    pub election_name: String,
    pub election_date: String,
    pub electoral_districts: Vec<ElectoralDistrict>,
    pub lists: BTreeMap<String, Vec<Vec<ElectoralDistrict>>>,
}

impl CandidateListSummary {
    fn active_districts(&self, list: &str) -> Vec<ElectoralDistrict> {
        self.lists.get(list).map_or_default(|batched_districts| {
            batched_districts.iter().cloned().flatten().collect()
        })
    }

    fn batched_districts(&self, list: &str) -> Vec<(Option<usize>, String)> {
        let Some(batches) = self.lists.get(list) else {
            return Vec::with_capacity(0);
        };

        let mut batches: Vec<Vec<_>> = batches
            .iter()
            .map(|batch| {
                let mut numbers: Vec<_> =
                    batch.iter().map(ElectoralDistrict::region_number).collect();
                numbers.sort_unstable();
                numbers
            })
            .collect();
        batches.sort_unstable();

        let mut counter = 0;
        batches
            .into_iter()
            .map(|numbers| {
                // A list in 1 district does not get a 'stel' number
                let stel = (numbers.len() > 1).then(|| {
                    counter += 1;
                    counter
                });
                let districts = numbers
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                (stel, districts)
            })
            .collect()
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
