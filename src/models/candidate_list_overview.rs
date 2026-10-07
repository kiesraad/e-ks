//! Model: Overzicht kandidatenlijsten.
//! This model is Dutch-only; the document text lives in the `templates/osv3-4.md`
//! Markdown template.

use std::collections::BTreeMap;

use textris_pdf::build::Textris;

use super::{Pdf, layout::markdown_document, markdown::filters, markdown::model_template};
use crate::{AppError, ElectoralDistrict};

#[derive(Debug)]
pub struct CandidateListOverview {
    pub election_name: String,
    pub election_date: String,
    pub electoral_districts: Vec<ElectoralDistrict>,
    pub lists: BTreeMap<String, Vec<Vec<ElectoralDistrict>>>,
}

impl CandidateListOverview {
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
        let batch_count = batches.len();

        let mut counter = 0;
        batches
            .into_iter()
            .map(|numbers| {
                // A list in 1 district does not get a 'stel' number. Lists that are 'gelijkluidend' also don't.
                let stel = (numbers.len() > 1 && batch_count > 1).then(|| {
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

    fn affiliation_type(&self, list: &str) -> Option<AffiliationType> {
        self.lists.get(list).and_then(|districts| {
            if districts.len() == 1 {
                if districts[0].len() == 1 {
                    Some(AffiliationType::StandAloneList)
                } else {
                    Some(AffiliationType::SetOfEqualLists)
                }
            } else if districts.len() > 1 {
                Some(AffiliationType::GroupOfLists)
            } else {
                None
            }
        })
    }
}

pub enum AffiliationType {
    /// lijstengroep
    GroupOfLists,
    /// stel gelijkluidende lijsten
    SetOfEqualLists,
    /// op zichzelf staande lijst
    StandAloneList,
}

model_template!(
    CandidateListOverviewTemplate,
    CandidateListOverview,
    "models/templates/candidate-list-overview.md"
);

impl Pdf for CandidateListOverview {
    fn document(&self) -> Result<Textris, AppError> {
        markdown_document(CandidateListOverviewTemplate(self))
    }

    fn filename(&self) -> String {
        "overzicht_kandidatenlijsten.pdf".to_string()
    }
}
