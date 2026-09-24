use crate::{
    Locale,
    csb::examination::structs::BrpCheckState,
    structs::brp::{BrpCheckedField, BrpFinding, BrpStatus},
    trans,
};

/// One finding as the candidate detail table tags it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrpFindingTag {
    pub message: String,
    pub handled: bool,
}

/// The BRP findings for one candidate, grouped and translated the way the
/// candidate detail table is laid out.
#[derive(Debug, Default)]
pub struct CandidateBrpFindings {
    pub bsn: Vec<BrpFindingTag>,
    pub initials: Vec<BrpFindingTag>,
    pub last_name_prefix: Vec<BrpFindingTag>,
    pub last_name: Vec<BrpFindingTag>,
    pub gender: Vec<BrpFindingTag>,
    pub date_of_birth: Vec<BrpFindingTag>,
    pub place_of_residence: Vec<BrpFindingTag>,
    /// Findings about the candidate as a whole rather than about one row.
    pub candidate: Vec<BrpFindingTag>,
}

impl CandidateBrpFindings {
    pub fn new(findings: &[BrpFinding], locale: Locale) -> Self {
        let mut grouped = Self::default();

        for finding in findings {
            let message = BrpFindingTag {
                message: finding.message(locale),
                handled: finding.handled,
            };
            let target = match finding.field() {
                Some(BrpCheckedField::Bsn) => &mut grouped.bsn,
                Some(BrpCheckedField::Initials) => &mut grouped.initials,
                Some(BrpCheckedField::LastNamePrefix) => &mut grouped.last_name_prefix,
                Some(BrpCheckedField::LastName) => &mut grouped.last_name,
                Some(BrpCheckedField::Gender) => &mut grouped.gender,
                Some(BrpCheckedField::DateOfBirth) => &mut grouped.date_of_birth,
                Some(BrpCheckedField::PlaceOfResidence) => &mut grouped.place_of_residence,
                None => &mut grouped.candidate,
            };
            target.push(message);
        }

        grouped
    }
}

/// Why the BRP data on screen may be incomplete, or `None` when everything in
/// scope was checked and the check ran to completion. Without it, an unchecked
/// candidate reads as one the BRP agreed with.
///
/// `running` comes from [`crate::csb::import::brp_sweep_running`]: the
/// recorded status cannot tell a live sweep from an abandoned one.
pub fn brp_incomplete_reason(
    status: &BrpStatus,
    state: &BrpCheckState,
    running: bool,
    locale: Locale,
) -> Option<String> {
    match status {
        BrpStatus::NotStarted => Some(trans!("csb.brp.status.not_started", locale)),
        BrpStatus::InProgress { .. } if running => {
            Some(trans!("csb.brp.status.in_progress", locale))
        }
        // Recorded as running with nothing behind it: stopped without getting
        // to record how.
        BrpStatus::InProgress { .. } => Some(trans!("csb.brp.status.interrupted", locale)),
        BrpStatus::Aborted(error) => Some(trans!("csb.brp.status.aborted", locale, error)),
        // A sweep that ran to completion still leaves candidates unchecked
        // when a correction dropped their findings afterwards.
        BrpStatus::Finished if !state.is_checked() => {
            Some(trans!("csb.brp.status.changed_since_check", locale))
        }
        BrpStatus::Finished => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structs::brp::{BrpFindingKind, BrpValue};

    #[test]
    fn findings_are_grouped_onto_the_row_they_belong_to() {
        let grouped = CandidateBrpFindings::new(
            &[
                BrpFindingKind::Mismatch {
                    brp_value: BrpValue::LastName("Bruin".parse().unwrap()),
                }
                .into(),
                BrpFindingKind::Mismatch {
                    brp_value: BrpValue::LastNamePrefix("de".parse().unwrap()),
                }
                .into(),
                BrpFindingKind::ResidenceAbroad.into(),
                BrpFindingKind::NotDutch.into(),
            ],
            Locale::En,
        );

        assert_eq!(grouped.last_name.len(), 1);
        assert_eq!(grouped.last_name_prefix.len(), 1);
        assert_eq!(grouped.place_of_residence.len(), 1);
        assert_eq!(grouped.candidate.len(), 1);
        assert!(grouped.initials.is_empty());
    }

    #[test]
    fn only_an_incomplete_check_is_reported() {
        let checked = BrpCheckState::Correct;
        assert!(brp_incomplete_reason(&BrpStatus::Finished, &checked, false, Locale::En).is_none());
        assert!(
            brp_incomplete_reason(&BrpStatus::NotStarted, &checked, false, Locale::En).is_some()
        );
        assert!(
            brp_incomplete_reason(&BrpStatus::in_progress(), &checked, true, Locale::En).is_some()
        );

        let aborted = BrpStatus::Aborted("upstream unreachable".to_string());
        let reason = brp_incomplete_reason(&aborted, &checked, false, Locale::En)
            .expect("aborted needs a reason");
        assert!(
            reason.contains("upstream unreachable"),
            "the reason should name what went wrong, got: {reason}"
        );
    }

    #[test]
    fn a_sweep_recorded_as_running_with_nothing_behind_it_is_not_reported_as_running() {
        let status = BrpStatus::in_progress();
        let state = BrpCheckState::Incomplete { errors: 0 };

        let running = brp_incomplete_reason(&status, &state, true, Locale::En)
            .expect("a running sweep needs a reason");
        let interrupted = brp_incomplete_reason(&status, &state, false, Locale::En)
            .expect("an interrupted sweep needs a reason");

        assert!(running.contains("still running"), "got: {running}");
        assert!(
            !interrupted.contains("still running"),
            "an interrupted sweep must not tell the committee to wait, got: {interrupted}"
        );
        assert!(interrupted.contains("stopped"), "got: {interrupted}");
    }

    #[test]
    fn a_candidate_corrected_after_the_check_is_reported_as_needing_another() {
        let reason = brp_incomplete_reason(
            &BrpStatus::Finished,
            &BrpCheckState::NotChecked,
            false,
            Locale::En,
        )
        .expect("a finished sweep with unchecked candidates needs a reason");

        assert!(reason.contains("changed"), "got: {reason}");
    }
}
