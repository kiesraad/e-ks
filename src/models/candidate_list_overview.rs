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

    pub(crate) fn affiliation_type(
        &self,
        district_batches: &[Vec<ElectoralDistrict>],
    ) -> AffiliationType {
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

#[derive(Debug, PartialEq, Eq)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use ElectoralDistrict::{Bonaire, Drenthe, Fryslan, Groningen, Overijssel, Utrecht};

    fn overview() -> CandidateListOverview {
        CandidateListOverview {
            election_name: String::new(),
            election_date: String::new(),
            electoral_districts: vec![],
            lists: vec![],
        }
    }

    #[test]
    fn affiliation_types() {
        assert_eq!(
            overview().affiliation_type(&[vec![Utrecht]]),
            AffiliationType::StandAloneList
        );

        assert_eq!(
            overview().affiliation_type(&[vec![Groningen, Drenthe]]),
            AffiliationType::SetOfEqualLists
        );

        assert_eq!(
            overview().affiliation_type(&[vec![Groningen], vec![Drenthe]]),
            AffiliationType::GroupOfLists
        );

        assert_eq!(
            overview().affiliation_type(&[vec![Groningen, Drenthe], vec![Utrecht]]),
            AffiliationType::GroupOfLists
        );
    }

    #[test]
    fn active_districts_are_the_districts_of_every_batch() {
        let active = overview().active_districts(&[vec![Utrecht], vec![Groningen, Drenthe]]);

        assert_eq!(active.len(), 3);
        for district in [Utrecht, Groningen, Drenthe] {
            assert!(active.contains(&district));
        }
    }

    #[test]
    fn a_standalone_list_gets_no_stel_number() {
        assert_eq!(
            overview().number_batched_districts(&[vec![Utrecht]]),
            [(None, "7".to_string())]
        );
    }

    #[test]
    fn a_set_of_equal_lists_gets_no_stel_number() {
        assert_eq!(
            overview().number_batched_districts(&[vec![Drenthe, Groningen]]),
            [(None, "1, 3".to_string())]
        );
    }

    #[test]
    fn lists_in_one_district_get_no_stel_number_and_are_skipped_in_the_count() {
        assert_eq!(
            overview().number_batched_districts(&[
                vec![Drenthe, Groningen],
                vec![Fryslan],
                vec![Bonaire, Overijssel],
            ]),
            [
                (Some(1), "1, 3".to_string()),
                (None, "2".to_string()),
                (Some(2), "4, 13".to_string()),
            ]
        );
    }

    #[test]
    fn stel_numbers_districts_and_lists_are_sorted() {
        // Given in reverse; the districts sort numerically (13 after 7) and
        // the lists on their lowest district.
        assert_eq!(
            overview().number_batched_districts(&[
                vec![Bonaire, Utrecht],
                vec![Overijssel, Fryslan],
                vec![Drenthe, Groningen],
            ]),
            [
                (Some(1), "1, 3".to_string()),
                (Some(2), "2, 4".to_string()),
                (Some(3), "7, 13".to_string()),
            ]
        );
    }
}
