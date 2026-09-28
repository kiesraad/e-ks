//! The overview the central voting bureau hands a political group after the
//! pre-submission check (*voorinlevering*): every candidate with BRP
//! discrepancies, or whose details the application itself flagged,
//! with the details that were checked and what was found, so the group can
//! fix them before nomination day. Not a numbered official model, but rendered
//! in the same house style; Dutch-only, with the text in the
//! `templates/brp-overview.md` Markdown template.

use chrono::NaiveDate;
use eks_utils::slugify_teletex;
use textris_pdf::build::Textris;

use super::{
    Pdf,
    layout::markdown_document,
    markdown::{filters, model_template},
};
use crate::AppError;

#[derive(Debug)]
pub struct BrpOverview {
    /// Election title as printed after "de verkiezing van".
    pub election_name: String,
    pub appellation: String,
    /// Election code stamped into the filename, e.g. `ek27`.
    pub election_code: String,
    /// Date the overview was drawn up.
    pub date: NaiveDate,
    /// Whether every candidate was checked; when not, the overview warns that
    /// it may be incomplete.
    pub complete: bool,
    /// The candidates the BRP check agreed with on every field.
    pub candidates_without_brp_errors: usize,
    /// The candidates with BRP discrepancies.
    pub candidates_with_brp_errors: usize,
    /// All BRP findings together.
    pub brp_error_count: usize,
    /// All warnings the application flagged about the candidates' details.
    pub problem_count: usize,
    /// The candidates with something to report, in list order. Empty: the
    /// overview reports that nothing was found.
    pub candidates: Vec<OverviewCandidate>,
}

/// One candidate with something to report: the details the BRP check
/// compared, followed by what was found.
#[derive(Debug)]
pub struct OverviewCandidate {
    /// Position on the list.
    pub position: usize,
    /// As displayed in the app, e.g. `de Boer, B. (Bas)`.
    pub name: String,
    /// The candidate's details as handed in, one row per field the BRP check
    /// compares, in the order the candidate page lists them.
    pub details: Vec<CandidateDetail>,
    /// What the BRP check found, already translated.
    pub findings: Vec<String>,
    /// What the application flagged about the candidate's details while the
    /// political group filled them in, already translated.
    pub problems: Vec<String>,
}

impl OverviewCandidate {
    /// E.g. `Kandidaat nr. 2: de Boer, B. (Bas)`.
    pub fn heading(&self) -> String {
        format!("Kandidaat nr. {}: {}", self.position, self.name)
    }
}

/// One row of a candidate's details: the field's label and its value as
/// handed in, empty when it was not given.
#[derive(Debug)]
pub struct CandidateDetail {
    pub label: String,
    pub value: String,
}

model_template!(
    BrpOverviewTemplate,
    BrpOverview,
    "models/templates/brp-overview.md"
);

impl Pdf for BrpOverview {
    fn document(&self) -> Result<Textris, AppError> {
        markdown_document(BrpOverviewTemplate(self))
    }

    fn filename(&self) -> String {
        // Slugifying drops the empty parts, so a group without an appellation
        // still gets a name.
        let slug = slugify_teletex(
            &format!("{} {}", self.appellation, self.election_code),
            true,
        );
        if slug.is_empty() {
            "brp-overzicht.pdf".to_string()
        } else {
            format!("brp-overzicht-{slug}.pdf")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_heading_names_the_position() {
        let candidate = OverviewCandidate {
            position: 2,
            name: "de Boer, B. (Bas)".to_string(),
            details: Vec::new(),
            findings: Vec::new(),
            problems: Vec::new(),
        };
        assert_eq!(candidate.heading(), "Kandidaat nr. 2: de Boer, B. (Bas)");
    }
}
