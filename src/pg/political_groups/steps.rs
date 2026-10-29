use crate::structs::list_designation::ListDesignationKind;
use axum_extra::routing::TypedPath;

use crate::{
    AppError, PgStore, QueryParamState,
    structs::{
        common::{HasSeverity, PotentialProblems, Problematic, Severity},
        list_submitters::{ListSubmitter, SubmitterRole},
        name_authorisations::NameAuthorisation,
        political_groups::PoliticalGroup,
    },
};

/// The steps of the political group flow, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PoliticalGroupStep {
    ListDesignation,
    BasicInformation,
    NameAuthorisations,
    ListSubmitter,
}

#[derive(Clone, Debug)]
pub struct PoliticalGroupSteps {
    pub name_authorisations: Vec<NameAuthorisation>,
    pub list_submitter: ListSubmitter,
    pub substitute_submitters: Vec<ListSubmitter>,
    pub list_designation: Option<ListDesignationKind>,
    pub initial: bool,

    pub list_designation_state: &'static str,
    pub basic_state: &'static str,
    pub name_authorisations_state: &'static str,
    pub submitters_state: &'static str,
}

impl PoliticalGroupSteps {
    /// The steps as seen from `current`. While `initial`, a step not reached
    /// yet is fine when empty; a step skipped on the way shows its problems.
    pub fn new(
        store: &PgStore,
        initial: bool,
        current: PoliticalGroupStep,
    ) -> Result<Self, AppError> {
        let fine_if_empty = |step| initial && step >= current;
        let data = store.snapshot();
        let political_group = data.get_political_group();
        let name_authorisations: Vec<NameAuthorisation> = data
            .get_name_authorisations()
            .into_iter()
            .cloned()
            .collect();
        let list_submitter = data.get_list_submitter();
        let substitute_submitters = data.get_substitute_submitters();

        Ok(Self {
            initial,
            list_designation_state: Self::list_designation_state(political_group),
            basic_state: Self::basic_state(
                fine_if_empty(PoliticalGroupStep::BasicInformation),
                political_group,
            ),
            name_authorisations_state: Self::name_authorisations_state(
                fine_if_empty(PoliticalGroupStep::NameAuthorisations),
                political_group.list_designation_kind(),
                &name_authorisations,
            ),
            submitters_state: Self::submitters_state(
                fine_if_empty(PoliticalGroupStep::ListSubmitter),
                list_submitter,
                substitute_submitters,
            ),
            name_authorisations,
            list_submitter: list_submitter.clone(),
            substitute_submitters: substitute_submitters.to_vec(),
            list_designation: political_group.list_designation_kind(),
        })
    }

    fn list_designation_state(political_group: &PoliticalGroup) -> &'static str {
        if political_group.is_list_designation_type_empty() {
            "empty"
        } else {
            "ok"
        }
    }

    fn basic_state(fine_if_empty: bool, political_group: &PoliticalGroup) -> &'static str {
        if fine_if_empty && political_group.is_group_information_empty() {
            "empty"
        } else {
            political_group.get_problems(()).highest_severity_class()
        }
    }

    fn name_authorisations_state(
        fine_if_empty: bool,
        list_designation: Option<ListDesignationKind>,
        name_authorisations: &[NameAuthorisation],
    ) -> &'static str {
        if name_authorisations.is_empty() {
            return if fine_if_empty { "empty" } else { "warning" };
        }

        let size_severity =
            NameAuthorisation::get_size_problems(list_designation, name_authorisations.len())
                .iter()
                .map(PotentialProblems::severity)
                .max();

        match name_authorisations
            .iter()
            .filter_map(|na| na.get_problems(()).highest_severity())
            .chain(size_severity)
            .max()
        {
            None => "ok",
            Some(severity) => severity.class(),
        }
    }

    fn submitters_state(
        fine_if_empty: bool,
        list_submitter: &ListSubmitter,
        substitute_submitters: &[ListSubmitter],
    ) -> &'static str {
        if list_submitter.is_empty() {
            return if fine_if_empty { "empty" } else { "error" };
        }

        match substitute_submitters
            .iter()
            .filter_map(|ss| {
                ss.get_problems(SubmitterRole::Substitute)
                    .highest_severity()
            })
            .chain(
                list_submitter
                    .get_problems(SubmitterRole::Main)
                    .highest_severity(),
            )
            .chain(substitute_submitters.is_empty().then_some(Severity::Info))
            .max()
        {
            None => "ok",
            Some(severity) => severity.class(),
        }
    }

    pub fn is_blank(&self) -> bool {
        self.list_designation == Some(ListDesignationKind::Blank)
    }

    pub fn is_combined(&self) -> bool {
        self.list_designation == Some(ListDesignationKind::Combined)
    }

    /// The authorisation form for the appellation: H 3-2 for a combined
    /// designation, H 3-1 otherwise
    pub fn designation_form(&self) -> &'static str {
        if self.is_combined() {
            "H\u{A0}3-2"
        } else {
            "H\u{A0}3-1"
        }
    }

    /// Returns the URL for a step link, preserving `?initial=true`
    pub fn step_url(&self, path: impl TypedPath) -> String {
        if self.initial {
            path.with_query_params(QueryParamState::initial())
                .to_string()
        } else {
            path.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppError, PgStore, structs::name_authorisations::NameAuthorisationId,
        test_utils::sample_name_authorisation,
    };

    #[tokio::test]
    async fn name_authorisations_state_error_when_too_many() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        sample_name_authorisation(NameAuthorisationId::new())
            .create(&store)
            .await?;
        sample_name_authorisation(NameAuthorisationId::new())
            .create(&store)
            .await?;

        let steps = PoliticalGroupSteps::new(&store, false, PoliticalGroupStep::ListSubmitter)?;
        assert_eq!(steps.name_authorisations_state, "error");

        Ok(())
    }

    #[tokio::test]
    async fn submitter_state_empty_with_initial() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        sample_name_authorisation(NameAuthorisationId::new())
            .create(&store)
            .await?;

        let steps = PoliticalGroupSteps::new(&store, true, PoliticalGroupStep::NameAuthorisations)?;
        assert_eq!(steps.submitters_state, "empty");

        Ok(())
    }

    #[tokio::test]
    async fn initial_steps_skipped_on_the_way_show_their_problems() -> Result<(), AppError> {
        let store = PgStore::new_for_test();

        // On the step itself, and before it, an empty step is fine.
        let steps = PoliticalGroupSteps::new(&store, true, PoliticalGroupStep::NameAuthorisations)?;
        assert_eq!(steps.name_authorisations_state, "empty");
        assert_eq!(steps.submitters_state, "empty");

        // Past it, the skipped steps warn.
        let steps = PoliticalGroupSteps::new(&store, true, PoliticalGroupStep::ListSubmitter)?;
        assert_eq!(steps.name_authorisations_state, "warning");
        assert_eq!(steps.submitters_state, "empty");

        Ok(())
    }

    #[tokio::test]
    async fn submitter_state_error_without_initial() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        sample_name_authorisation(NameAuthorisationId::new())
            .create(&store)
            .await?;

        let steps = PoliticalGroupSteps::new(&store, false, PoliticalGroupStep::ListSubmitter)?;
        assert_eq!(steps.submitters_state, "error");

        Ok(())
    }

    #[test]
    fn designation_form_depends_on_combined() -> Result<(), AppError> {
        let mut steps = PoliticalGroupSteps::new(
            &PgStore::new_for_test(),
            false,
            PoliticalGroupStep::ListDesignation,
        )?;
        assert_eq!(steps.designation_form(), "H\u{A0}3-1");

        steps.list_designation = Some(ListDesignationKind::Standalone);
        assert_eq!(steps.designation_form(), "H\u{A0}3-1");

        steps.list_designation = Some(ListDesignationKind::Blank);
        assert_eq!(steps.designation_form(), "H\u{A0}3-1");

        steps.list_designation = Some(ListDesignationKind::Combined);
        assert_eq!(steps.designation_form(), "H\u{A0}3-2");

        Ok(())
    }
}
