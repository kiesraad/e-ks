//! Model: Overzicht kandidatenlijsten.
//! This model is Dutch-only; the document text lives in the `templates/osv3-4.md`
//! Markdown template.

use textris_pdf::build::Textris;

use super::{Pdf, layout::markdown_document, markdown::filters, markdown::model_template};
use crate::{AppError, ElectoralDistrict};

#[derive(Debug)]
pub struct CandidateListOverview {
    pub election_name: String,
    pub election_date: String,
    pub electoral_districts: Vec<ElectoralDistrict>,
    pub lists: Vec<(String, Vec<Vec<ElectoralDistrict>>)>,
}

impl CandidateListOverview {
    fn active_districts(
        &self,
        district_batches: &[Vec<ElectoralDistrict>],
    ) -> Vec<ElectoralDistrict> {
        district_batches.iter().cloned().flatten().collect()
    }

    fn number_batched_districts(
        &self,
        district_batches: &[Vec<ElectoralDistrict>],
    ) -> Vec<(Option<usize>, String)> {
        let mut batches: Vec<Vec<_>> = district_batches
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

    fn affiliation_type(&self, district_batches: &[Vec<ElectoralDistrict>]) -> AffiliationType {
        if district_batches.len() == 1 {
            if district_batches[0].len() == 1 {
                AffiliationType::StandAloneList
            } else {
                AffiliationType::SetOfEqualLists
            }
        } else {
            AffiliationType::GroupOfLists
        }
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
