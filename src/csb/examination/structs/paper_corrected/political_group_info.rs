use super::PaperCorrected;
use crate::{
    CsbStream, Locale,
    projection::WithCorrections,
    structs::{
        common::PreviousElectionResults, csb::CsbPhase, list_designation::ListDesignation,
        political_groups::PoliticalGroup,
    },
    trans,
};

/// The political group rows of the general information page.
pub struct PaperCorrectedPoliticalGroupInfo {
    /// Label of the appellation row, or `None` for blank designations, which
    /// have no appellation and so omit the row entirely.
    pub appellation_label: Option<String>,
    pub appellation: PaperCorrected,
    pub list_type: PaperCorrected,
    pub previous_results: PaperCorrected,
    /// Blank lists have no name authorisations.
    pub is_blank: bool,
}

impl PaperCorrectedPoliticalGroupInfo {
    pub fn new(store: &CsbStream, locale: Locale, mode: CsbPhase) -> Self {
        let imported_group = store.get_political_group(WithCorrections::None);
        let paper_corrected_group = store.get_political_group(WithCorrections::Paper);

        let designation = paper_corrected_group.list_designation.unwrap_or_default();

        let mut list_type = PaperCorrected::new(
            list_type_label(&imported_group, locale),
            list_type_label(&paper_corrected_group, locale),
        );
        if mode.is_recovery() && store.is_appellation_scrapped() {
            list_type = list_type
                .with_csb_correction(Some(trans!("political_group.type.blank_name", locale)));
        }

        Self {
            appellation_label: appellation_label(designation, locale),
            appellation: PaperCorrected::new(
                store.get_appellation(WithCorrections::None),
                store.get_appellation(WithCorrections::Paper),
            )
            .with_csb_correction(Some(store.get_appellation(WithCorrections::All))),
            list_type,
            previous_results: PaperCorrected::new(
                previous_results_label(&imported_group, locale),
                previous_results_label(&paper_corrected_group, locale),
            ),
            is_blank: designation == ListDesignation::Blank,
        }
    }
}

fn appellation_label(designation: ListDesignation, locale: Locale) -> Option<String> {
    match designation {
        ListDesignation::Standalone => Some(trans!("political_group.appellation", locale)),
        ListDesignation::Combined => Some(trans!("political_group.appellation_combined", locale)),
        ListDesignation::Blank => None,
    }
}

fn list_type_label(political_group: &PoliticalGroup, locale: Locale) -> String {
    match political_group.list_designation {
        Some(ListDesignation::Standalone) => trans!("political_group.type.registered_name", locale),
        Some(ListDesignation::Combined) => trans!("political_group.type.name_combination", locale),
        Some(ListDesignation::Blank) => trans!("political_group.type.blank_name", locale),
        None => "-".to_string(),
    }
}

fn previous_results_label(political_group: &PoliticalGroup, locale: Locale) -> String {
    match political_group.previous_election_results {
        Some(PreviousElectionResults::ZeroSeats) => {
            trans!("political_group.type.zero_seats", locale)
        }
        Some(PreviousElectionResults::OneToFifteenSeats) => {
            trans!("political_group.type.one_to_fifteen_seats", locale)
        }
        Some(PreviousElectionResults::SixteenOrMoreSeats) => {
            trans!("political_group.type.sixteen_or_more_seats", locale)
        }
        None => "-".to_string(),
    }
}
