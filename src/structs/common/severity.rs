use serde::Serialize;

/// Problem severities, in increasing order of severity
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug, Serialize)]
pub enum Severity {
    Info,
    Warn,
    Error,
}

impl Severity {
    pub fn class(&self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warn => "warning",
            Severity::Error => "error",
        }
    }

    /// Modifier of the `restoration-tag` a problem of this severity is shown
    /// with; there is no info tag, so anything below an error is a warning.
    pub fn restoration_tag_modifier(&self) -> &'static str {
        match self {
            Severity::Info | Severity::Warn => "warning",
            Severity::Error => "error",
        }
    }
}

pub trait HasSeverity {
    /// Returns the highest severity, or None if there are no problems
    fn highest_severity(&self) -> Option<Severity>;

    /// Returns true if there are no problems
    fn is_all_good(&self) -> bool {
        self.highest_severity().is_none()
    }

    /// Returns the CSS class associated with the highest severity
    fn highest_severity_class(&self) -> &'static str {
        self.highest_severity()
            .map(|severity| severity.class())
            .unwrap_or("ok")
    }

    fn has_severity_or_higher(&self, severity: Severity) -> bool {
        self.highest_severity()
            .map(|highest| highest >= severity)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_order() {
        assert!(Severity::Info < Severity::Warn);
        assert!(Severity::Warn < Severity::Error);
    }
}
