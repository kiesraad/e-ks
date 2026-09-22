use crate::{
    AppError, OptionAsStrExt,
    structs::{
        audit_log::audit_fields,
        common::{Appellation, FullName, PreviousElectionResults, Problematic, Problems},
        list_designation::ListDesignation,
    },
};
use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct PoliticalGroup {
    pub appellation: Option<Appellation>,
    pub list_designation: Option<ListDesignation>,
    pub previous_election_results: Option<PreviousElectionResults>,
}

audit_fields!(PoliticalGroup {
    appellation: leaf(Appellation),
    list_designation: leaf(ListDesignation),
    previous_election_results: leaf(PreviousElectionResults),
});

impl Problematic<()> for PoliticalGroup {
    fn get_problems(&self, _: ()) -> Problems {
        Problems::merge(vec![
            self.appellation.get_problems(self.list_designation),
            self.list_designation.get_problems(()),
            self.previous_election_results
                .get_problems(self.list_designation),
        ])
    }
}

impl PoliticalGroup {
    /// Appellation for use in exported PG documents (EML 210 and H-models)
    pub fn pg_appellation(&self) -> Result<String, AppError> {
        if self.list_designation == Some(ListDesignation::Blank) {
            // empty place holder
            return Ok(String::new());
        }
        self.appellation
            .as_ref()
            .map(|d| Ok(d.to_string()))
            .unwrap_or(Err(AppError::IncompleteData("Missing appellation")))
    }

    /// Appellation for use in the UI of the CSB module and the I-models
    pub fn csb_appellation(&self, first_candidate_name: Option<&FullName>) -> String {
        if self.list_designation == Some(ListDesignation::Blank) {
            return match first_candidate_name {
                Some(name) => format!(
                    "Blanco ({}, {})",
                    name.last_name_with_prefix(),
                    name.initials
                ),
                None => "Blanco".to_string(),
            };
        }
        if let Some(name) = &self.appellation {
            name.to_string()
        } else {
            "???".to_string()
        }
    }

    pub fn get_max_candidates(&self) -> usize {
        if self.list_designation == Some(ListDesignation::Blank) {
            return 50;
        }
        match self.previous_election_results {
            Some(PreviousElectionResults::SixteenOrMoreSeats) => 80,
            _ => 50,
        }
    }

    pub fn was_previously_seated(&self) -> bool {
        if self.list_designation == Some(ListDesignation::Blank) {
            return false;
        }
        self.previous_election_results
            .is_some_and(|r| r != PreviousElectionResults::ZeroSeats)
    }

    pub fn is_list_designation_type_empty(&self) -> bool {
        self.list_designation.is_none()
    }

    pub fn is_group_information_empty(&self) -> bool {
        self.appellation.is_empty_or_none() && self.previous_election_results.is_none()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        structs::common::{InfoProblems, Initials, LastName, PotentialProblems},
        test_utils::sample_political_group,
    };

    use super::*;

    use std::str::FromStr;

    #[test]
    fn incomplete_items_empty() {
        let empty_items = PoliticalGroup {
            previous_election_results: None,
            list_designation: None,
            appellation: None,
        }
        .get_problems(());

        assert_eq!(empty_items.potential_problems.len(), 1);
        assert!(
            empty_items
                .potential_problems
                .contains(&PotentialProblems::NoAppellation)
        );

        assert_eq!(empty_items.info_problems.len(), 2);
        assert!(
            empty_items
                .info_problems
                .contains(&InfoProblems::NoPreviousElectionResults)
        );
        assert!(
            empty_items
                .info_problems
                .contains(&InfoProblems::NoListDesignation)
        );
    }

    #[test]
    fn complete_no_problems() {
        let problems = PoliticalGroup {
            previous_election_results: Some(PreviousElectionResults::OneToFifteenSeats),
            list_designation: Some(ListDesignation::Standalone),
            appellation: Appellation::from_str("test").ok(),
        }
        .get_problems(());

        assert!(problems.potential_problems.is_empty());
        assert!(problems.info_problems.is_empty());
    }

    #[test]
    fn complete_blank_list_no_problems() {
        let problems = PoliticalGroup {
            previous_election_results: None,
            list_designation: Some(ListDesignation::Blank),
            appellation: None,
        }
        .get_problems(());
        assert!(problems.potential_problems.is_empty());
        assert!(problems.info_problems.is_empty());
    }

    #[test]
    fn blank_lists_force_defaults_even_if_set_differently() {
        let mut group = PoliticalGroup {
            previous_election_results: Some(PreviousElectionResults::SixteenOrMoreSeats),
            list_designation: Some(ListDesignation::Standalone),
            appellation: Appellation::from_str("test").ok(),
        };
        assert_eq!(group.pg_appellation().unwrap(), "test");
        assert_eq!(group.get_max_candidates(), 80);
        assert!(group.was_previously_seated());

        // the set values should be ignored when switching to a blank list
        group.list_designation = Some(ListDesignation::Blank);
        assert_eq!(group.pg_appellation().unwrap(), "");
        assert_eq!(group.get_max_candidates(), 50);
        assert!(!group.was_previously_seated());
    }

    #[test]
    fn csb_appellation() {
        let mut pg = sample_political_group();
        let first_candidate_name = FullName {
            first_name: None,
            last_name: LastName::from_str("Nagelhout").unwrap(),
            last_name_prefix: None,
            initials: Initials::from_str("A.B.").unwrap(),
        };
        let cases = [
            (None, "Test Partij", "Test Partij", "???", "???"),
            (
                Some(ListDesignation::Blank),
                "Blanco (Nagelhout, A.B.)",
                "Blanco",
                "Blanco (Nagelhout, A.B.)",
                "Blanco",
            ),
            (
                Some(ListDesignation::Combined),
                "Test Partij",
                "Test Partij",
                "???",
                "???",
            ),
            (
                Some(ListDesignation::Standalone),
                "Test Partij",
                "Test Partij",
                "???",
                "???",
            ),
        ];
        for (
            designation,
            expected_with_candidate,
            expected_without_candidate,
            expected_with_candidate_no_appellation,
            expected_without_candidate_no_appellation,
        ) in cases
        {
            pg.appellation = Appellation::from_str("Test Partij").ok();
            pg.list_designation = designation;
            assert_eq!(
                expected_with_candidate,
                pg.csb_appellation(Some(&first_candidate_name))
            );
            assert_eq!(expected_without_candidate, pg.csb_appellation(None));

            pg.appellation = None;
            assert_eq!(
                expected_with_candidate_no_appellation,
                pg.csb_appellation(Some(&first_candidate_name))
            );
            assert_eq!(
                expected_without_candidate_no_appellation,
                pg.csb_appellation(None)
            );
        }
    }
}
