use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::structs::common::UtcDateTime;

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum BrpStatus {
    #[default]
    NotStarted,
    /// A sweep was started. For whether one is still running, ask
    /// [`crate::csb::import::brp_sweep_running`].
    InProgress {
        started_at: UtcDateTime,
    },
    /// The sweep stopped early. Candidates checked before that point keep
    /// their findings; the rest were not checked at all.
    Aborted(String),
    Finished,
}

impl BrpStatus {
    pub fn in_progress() -> Self {
        Self::InProgress {
            started_at: UtcDateTime::now(),
        }
    }
}

impl Display for BrpStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrpStatus::NotStarted => write!(f, "not_started"),
            BrpStatus::InProgress { .. } => write!(f, "in_progress"),
            BrpStatus::Aborted(_) => write!(f, "aborted"),
            BrpStatus::Finished => write!(f, "finished"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_status_has_its_own_name_for_the_audit_log() {
        let names: Vec<String> = [
            BrpStatus::NotStarted,
            BrpStatus::in_progress(),
            BrpStatus::Aborted("upstream unreachable".to_string()),
            BrpStatus::Finished,
        ]
        .iter()
        .map(ToString::to_string)
        .collect();

        let unique: std::collections::HashSet<&String> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "got: {names:?}");
    }
}
