use serde::{Deserialize, Serialize};

use crate::{
    OptionAsStrExt,
    structs::common::{InfoProblems, PotentialProblems, Problematic, Problems, Severity},
};

use super::{FirstName, Initials, LastName, LastNamePrefix};

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FullName {
    pub first_name: Option<FirstName>,
    pub last_name: LastName,
    pub last_name_prefix: Option<LastNamePrefix>,
    /// Absent for a person without first names, as the BRP allows.
    pub initials: Option<Initials>,
}

impl FullName {
    /// Returns e.g. "van Dijk, A.B. (Anne)"; without initials "van Dijk (Anne)"
    /// or just "van Dijk".
    pub fn display(&self) -> String {
        let last_name = self.last_name_with_prefix();
        match (&self.initials, &self.first_name) {
            (Some(initials), Some(first_name)) => {
                format!("{last_name}, {initials} ({first_name})")
            }
            (Some(initials), None) => format!("{last_name}, {initials}"),
            (None, Some(first_name)) => format!("{last_name} ({first_name})"),
            (None, None) => last_name,
        }
    }

    /// Returns e.g. "A.B. (Anne)", "A.B." or "(Anne)"; empty without either.
    pub fn initials_with_first_name(&self) -> String {
        match (&self.initials, &self.first_name) {
            (Some(initials), Some(first_name)) => format!("{initials} ({first_name})"),
            (Some(initials), None) => initials.to_string(),
            (None, Some(first_name)) => format!("({first_name})"),
            (None, None) => String::new(),
        }
    }

    /// Returns e.g. "van Dijk"
    pub fn last_name_with_prefix(&self) -> String {
        if let Some(prefix) = &self.last_name_prefix {
            format!("{} {}", prefix, self.last_name)
        } else {
            self.last_name.to_string()
        }
    }

    /// Returns e.g. "Dijk, van"
    pub fn last_name_with_prefix_appended(&self) -> String {
        if let Some(prefix) = &self.last_name_prefix {
            format!("{}, {}", self.last_name, prefix)
        } else {
            self.last_name.to_string()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.initials.is_none()
            && self.last_name.is_empty()
            && self.last_name_prefix.is_empty_or_none()
    }
}

impl PartialOrd for FullName {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Problematic<Severity> for FullName {
    /// `severity` is what a missing last name gets. Missing initials are never
    /// more than a warning: the BRP allows a person without first names, so a
    /// name without initials can be handed in as it is.
    fn get_problems(&self, severity: Severity) -> Problems {
        let mut potential_problems = Vec::new();
        let mut info_problems = Vec::new();
        if severity == Severity::Info {
            if self.initials.is_none() {
                info_problems.push(InfoProblems::NoInitials);
            }
            if self.last_name.is_empty() {
                info_problems.push(InfoProblems::NoLastName);
            }
        } else {
            if self.initials.is_none() {
                potential_problems
                    .push(PotentialProblems::NoInitials(severity.min(Severity::Warn)));
            }
            if self.last_name.is_empty() {
                potential_problems.push(PotentialProblems::NoLastName(severity));
            }
        }

        Problems {
            potential_problems,
            info_problems,
        }
    }
}

impl Ord for FullName {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.last_name
            .cmp(&other.last_name)
            .then_with(|| self.last_name_prefix.cmp(&other.last_name_prefix))
            .then_with(|| self.initials.cmp(&other.initials))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structs::common::HasSeverity;

    fn name(initials: Option<&str>, first_name: Option<&str>) -> FullName {
        FullName {
            first_name: first_name.map(|name| name.parse().unwrap()),
            last_name: "Dijk".parse().unwrap(),
            last_name_prefix: Some("van".parse().unwrap()),
            initials: initials.map(|initials| initials.parse().unwrap()),
        }
    }

    #[test]
    fn display_leaves_out_what_is_absent() {
        assert_eq!(
            name(Some("A.B."), Some("Anne")).display(),
            "van Dijk, A.B. (Anne)"
        );
        assert_eq!(name(Some("A.B."), None).display(), "van Dijk, A.B.");
        assert_eq!(name(None, Some("Anne")).display(), "van Dijk (Anne)");
        assert_eq!(name(None, None).display(), "van Dijk");

        assert_eq!(
            name(None, Some("Anne")).initials_with_first_name(),
            "(Anne)"
        );
        assert_eq!(name(None, None).initials_with_first_name(), "");
    }

    #[test]
    fn missing_initials_are_at_most_a_warning() {
        let problems = name(None, None).get_problems(Severity::Error);
        assert_eq!(
            problems.potential_problems,
            vec![PotentialProblems::NoInitials(Severity::Warn)]
        );
        assert_eq!(problems.highest_severity(), Some(Severity::Warn));

        let problems = name(None, None).get_problems(Severity::Info);
        assert_eq!(problems.info_problems, vec![InfoProblems::NoInitials]);

        assert!(
            name(Some("A.B."), None)
                .get_problems(Severity::Error)
                .is_all_good()
        );
    }
}
