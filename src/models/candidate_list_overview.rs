//! Model: Overzicht kandidatenlijsten.
//! This model is Dutch-only; the document text lives in the
//! `templates/candidate-list-overview.md` Markdown template.

use std::num::NonZeroU64;

use eml_nl::utils::AffiliationType;
use textris_pdf::build::Textris;

use super::{
    Pdf,
    established_lists::{ListSets, region_numbers, set_number},
    layout::markdown_document,
    markdown::{filters, model_template},
};
use crate::{AppError, ElectoralDistrict};

#[derive(Debug)]
pub struct CandidateListOverview {
    pub election_name: String,
    pub election_date: String,
    pub electoral_districts: Vec<ElectoralDistrict>,
    pub groups: Vec<OverviewGroup>,
}

/// A political group with an established list
#[derive(Debug)]
pub struct OverviewGroup {
    /// The group's list number
    pub number: usize,
    pub appellation: String,
    pub sets: ListSets,
}

impl OverviewGroup {
    /// Per set its stel number, shown only within a lijstengroep, and its
    /// region numbers
    fn stels(&self) -> Vec<(Option<NonZeroU64>, String)> {
        let numbered = self.sets.affiliation_type() == AffiliationType::GroupOfLists;
        self.sets
            .iter()
            .map(|set| (set_number(set).filter(|_| numbered), region_numbers(set)))
            .collect()
    }
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

    fn stels(batches: &[&[ElectoralDistrict]]) -> Vec<(Option<u64>, String)> {
        OverviewGroup {
            number: 1,
            appellation: String::new(),
            sets: ListSets::new(batches.iter().map(|batch| batch.to_vec()))
                .unwrap()
                .unwrap(),
        }
        .stels()
        .into_iter()
        .map(|(number, districts)| (number.map(NonZeroU64::get), districts))
        .collect()
    }

    #[test]
    fn a_standalone_list_gets_no_stel_number() {
        assert_eq!(stels(&[&[Utrecht]]), [(None, "7".to_string())]);
    }

    #[test]
    fn a_set_of_equal_lists_gets_no_stel_number() {
        assert_eq!(
            stels(&[&[Drenthe, Groningen]]),
            [(None, "1, 3".to_string())]
        );
    }

    #[test]
    fn lists_in_one_district_get_no_stel_number_and_are_skipped_in_the_count() {
        assert_eq!(
            stels(&[&[Drenthe, Groningen], &[Fryslan], &[Bonaire, Overijssel]]),
            [
                (Some(1), "1, 3".to_string()),
                (None, "2".to_string()),
                (Some(2), "4, 13".to_string()),
            ]
        );
    }
}
