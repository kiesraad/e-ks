use std::collections::{HashMap, HashSet};

use crate::{
    CsbStream,
    projection::WithCorrections,
    structs::{brp::BrpFinding, candidate_lists::CandidateList, persons::PersonId},
};

/// What the BRP check has to say about a set of candidates: one candidate, one
/// candidate list, or every list of a political group.
///
/// Always derived from the recorded findings, never stored beside them, so the
/// badge on the examination overview, the political group, the list and the
/// candidate cannot drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrpCheckState {
    /// None of these candidates has been checked.
    NotChecked,
    /// Some were checked and some were not, because the sweep is still running
    /// or stopped early. `errors` is what was found so far.
    Incomplete { errors: usize },
    /// All checked, and the BRP agreed on everything.
    Correct,
    /// All checked, with this many findings, of which `handled` were marked
    /// as dealt with by the committee.
    Errors { errors: usize, handled: usize },
}

impl BrpCheckState {
    /// The state for `candidates`. A candidate standing on more than one list
    /// is counted once.
    pub fn for_candidates(
        findings: &HashMap<PersonId, Vec<BrpFinding>>,
        candidates: impl IntoIterator<Item = PersonId>,
    ) -> Self {
        let mut seen = HashSet::new();
        let (mut total, mut checked, mut errors, mut handled) = (0, 0, 0, 0);

        for person_id in candidates {
            if !seen.insert(person_id) {
                continue;
            }
            total += 1;
            if let Some(found) = findings.get(&person_id) {
                checked += 1;
                errors += found.len();
                handled += found.iter().filter(|finding| finding.handled).count();
            }
        }

        match (total, checked) {
            // Nothing to check leaves nothing to report.
            (0, _) => Self::Correct,
            (_, 0) => Self::NotChecked,
            (total, checked) if checked < total => Self::Incomplete { errors },
            _ if errors == 0 => Self::Correct,
            _ => Self::Errors { errors, handled },
        }
    }

    pub fn for_candidate(store: &CsbStream, person_id: PersonId) -> Self {
        if !store.is_brp_checked(person_id) {
            return Self::NotChecked;
        }

        let findings = store.get_brp_findings_for_person(person_id);
        match findings.len() {
            0 => Self::Correct,
            errors => Self::Errors {
                errors,
                handled: findings.iter().filter(|finding| finding.handled).count(),
            },
        }
    }

    pub fn for_list(store: &CsbStream, list: &CandidateList) -> Self {
        Self::for_candidates(&store.get_brp_findings(), list.candidates.iter().copied())
    }

    /// The state over every candidate the committee is examining, which is why
    /// it reads the corrected lists: candidates the paper corrections added are
    /// examined too, and candidates they removed are not.
    pub fn for_political_group(store: &CsbStream) -> Self {
        Self::for_candidates(
            &store.get_brp_findings(),
            store
                .get_candidate_lists(WithCorrections::All)
                .into_iter()
                .flat_map(|list| list.candidates),
        )
    }

    pub fn errors(&self) -> usize {
        match self {
            Self::Incomplete { errors } | Self::Errors { errors, .. } => *errors,
            Self::NotChecked | Self::Correct => 0,
        }
    }

    pub fn has_errors(&self) -> bool {
        self.errors() > 0
    }

    /// Whether the check found something and the committee marked every
    /// finding as handled.
    pub fn is_all_handled(&self) -> bool {
        matches!(self, Self::Errors { errors, handled } if handled == errors)
    }

    pub fn is_not_checked(&self) -> bool {
        matches!(self, Self::NotChecked)
    }

    pub fn is_incomplete(&self) -> bool {
        matches!(self, Self::Incomplete { .. })
    }

    /// Whether every candidate in scope was checked, findings or not.
    pub fn is_checked(&self) -> bool {
        matches!(self, Self::Correct | Self::Errors { .. })
    }

    /// Checked in full, with nothing found. Anything else is worth showing.
    pub fn is_correct(&self) -> bool {
        matches!(self, Self::Correct)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structs::brp::BrpFindingKind;

    fn findings(entries: &[(PersonId, usize)]) -> HashMap<PersonId, Vec<BrpFinding>> {
        entries
            .iter()
            .map(|(id, count)| (*id, vec![BrpFindingKind::NotDutch.into(); *count]))
            .collect()
    }

    #[test]
    fn a_scope_nobody_checked_is_not_reported_as_correct() {
        let (a, b) = (PersonId::new(), PersonId::new());

        let state = BrpCheckState::for_candidates(&findings(&[]), [a, b]);

        assert_eq!(state, BrpCheckState::NotChecked);
        assert!(!state.is_correct());
    }

    #[test]
    fn a_partly_checked_scope_says_so_rather_than_reporting_what_it_has() {
        let (a, b) = (PersonId::new(), PersonId::new());

        let state = BrpCheckState::for_candidates(&findings(&[(a, 2)]), [a, b]);

        assert_eq!(state, BrpCheckState::Incomplete { errors: 2 });
    }

    #[test]
    fn findings_are_counted_once_per_candidate_however_many_lists_they_stand_on() {
        let a = PersonId::new();

        let state = BrpCheckState::for_candidates(&findings(&[(a, 3)]), [a, a, a]);

        assert_eq!(
            state,
            BrpCheckState::Errors {
                errors: 3,
                handled: 0
            }
        );
    }

    #[test]
    fn only_a_scope_whose_every_finding_is_handled_is_all_handled() {
        let a = PersonId::new();
        let mut all = findings(&[(a, 2)]);
        for finding in all.get_mut(&a).unwrap() {
            finding.handled = true;
        }

        let state = BrpCheckState::for_candidates(&all, [a]);
        assert_eq!(
            state,
            BrpCheckState::Errors {
                errors: 2,
                handled: 2
            }
        );
        assert!(state.is_all_handled());

        all.get_mut(&a).unwrap()[0].handled = false;
        let state = BrpCheckState::for_candidates(&all, [a]);
        assert!(!state.is_all_handled());

        assert!(!BrpCheckState::Correct.is_all_handled());
        assert!(!BrpCheckState::NotChecked.is_all_handled());
    }

    #[test]
    fn findings_for_someone_who_is_not_a_candidate_are_left_out() {
        let (candidate, other) = (PersonId::new(), PersonId::new());

        let state =
            BrpCheckState::for_candidates(&findings(&[(candidate, 0), (other, 4)]), [candidate]);

        assert_eq!(state, BrpCheckState::Correct);
    }
}
