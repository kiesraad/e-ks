//! The omission letter ("verzuimbrief") the central voting bureau sends to a
//! political group after the examination of its candidate lists. Not a
//! numbered official model, but rendered in the same house style; Dutch-only,
//! with the text in the `templates/omission-letter.md` Markdown template.

use chrono::NaiveDate;
use eks_utils::slugify_teletex;
use textris_pdf::build::Textris;

use super::{
    Pdf,
    inputs::Person,
    layout::markdown_document,
    markdown::{filters, model_template},
};
use crate::AppError;

#[derive(Debug)]
pub struct OmissionLetter {
    /// Election title as printed after "de verkiezing van".
    pub election_name: String,
    /// Place the letter is sent from, printed before the date.
    pub location: String,
    /// Date of the session the omissions were found in ("vergadering van
    /// heden"); the letter writes its dates out in full (`16 september 2025`).
    pub date: NaiveDate,
    /// The list submitter the letter is addressed to.
    pub addressee: Person,
    pub appellation: String,
    /// Election code stamped into the filename, e.g. `ek27`.
    pub election_code: String,
    /// One section per set of districts. Empty: the letter reports that no
    /// omissions were found.
    pub omission_groups: Vec<DistrictOmissions>,
    pub recovery_deadline_date: NaiveDate,
    pub recovery_deadline_time: String,
    /// Where recovered documents are handed in, as printed after "op".
    pub recovery_address: String,
    /// Signatories; empty leaves room to sign, as the models do.
    pub chair: String,
    pub secretary: String,
    /// Appendix 1; empty drops the appendix.
    pub declarations_of_support: Vec<DeclarationsOfSupport>,
}

/// The omissions applying to one set of districts, under their own heading.
#[derive(Debug)]
pub struct DistrictOmissions {
    /// E.g. `kieskring 1 (Groningen), 3 (Drenthe)`; unused when
    /// [`Self::covers_all_districts`] is set.
    pub electoral_districts: String,
    /// Heads the section differently.
    pub covers_all_districts: bool,
    /// About the group, its lists and the declarations of support: plain
    /// bullets.
    pub omissions: Vec<LetterOmission>,
    /// Each candidate's omissions under a heading naming the candidate, in
    /// list order.
    pub candidates: Vec<CandidateOmissions>,
}

impl DistrictOmissions {
    pub fn heading(&self) -> String {
        if self.covers_all_districts {
            "Alle kieskringen".to_string()
        } else {
            format!("Een deel van de kieskringen: {}", self.electoral_districts)
        }
    }
}

/// One candidate's omissions, bulleted under their own heading.
#[derive(Debug)]
pub struct CandidateOmissions {
    /// Position on the list; `None` (not on it) drops the number.
    pub position: Option<usize>,
    /// As displayed in the app, e.g. `de Boer, B. (Bas)`.
    pub name: String,
    pub omissions: Vec<LetterOmission>,
}

impl CandidateOmissions {
    /// E.g. `Kandidaat nr. 2: de Boer, B. (Bas)`.
    pub fn heading(&self) -> String {
        match self.position {
            Some(position) => format!("Kandidaat nr. {position}: {}", self.name),
            None => format!("Kandidaat: {}", self.name),
        }
    }
}

/// One omission bullet: the finding plus how to recover it.
#[derive(Debug)]
pub struct LetterOmission {
    /// The omission as described on model I 1.
    pub description: String,
    /// "Dit verzuim is te herstellen door ...".
    pub help_text: Option<String>,
}

/// One appendix row. A `None` count renders as a dash.
#[derive(Debug)]
pub struct DeclarationsOfSupport {
    /// As printed in the first column, e.g. `1. Groningen`.
    pub electoral_district: String,
    pub submitted: Option<usize>,
    pub approved: Option<usize>,
    pub still_required: Option<usize>,
}

model_template!(
    OmissionLetterTemplate,
    OmissionLetter,
    "models/templates/omission-letter.md"
);

impl Pdf for OmissionLetter {
    fn document(&self) -> Result<Textris, AppError> {
        markdown_document(OmissionLetterTemplate(self))
    }

    fn filename(&self) -> String {
        // Slugifying drops the empty parts, so a blank list (no appellation)
        // still gets a name.
        let slug = slugify_teletex(
            &format!("{} {}", self.appellation, self.election_code),
            true,
        );
        if slug.is_empty() {
            "verzuimbrief.pdf".to_string()
        } else {
            format!("verzuimbrief-{slug}.pdf")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_heading_names_the_position_when_known() {
        let mut candidate = CandidateOmissions {
            position: Some(2),
            name: "de Boer, B. (Bas)".to_string(),
            omissions: Vec::new(),
        };
        assert_eq!(candidate.heading(), "Kandidaat nr. 2: de Boer, B. (Bas)");

        candidate.position = None;
        assert_eq!(candidate.heading(), "Kandidaat: de Boer, B. (Bas)");
    }
}
