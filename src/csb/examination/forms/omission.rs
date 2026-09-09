use serde::Deserialize;
use validate::Validate;

use crate::{
    ElectoralDistrict,
    structs::{
        candidate_lists::CandidateListId,
        csb::{Omission, OmissionText, OmissionTitle},
    },
};

/// Form backing the "add omission" dialog. The category is not part of the form:
/// it is derived from the dialog's path parameters and set on the resulting
/// [`Omission`] by the handler after validation.
#[derive(Deserialize, Debug, Validate)]
#[validate(target = "Omission")]
#[serde(default)]
pub struct OmissionForm {
    #[validate(parse = "OmissionTitle")]
    pub title: String,
    /// The description shown on model I 1.
    #[validate(parse = "OmissionText")]
    pub description: String,
    /// The note added to the omission letter ("verzuimbrief").
    #[validate(parse = "OmissionText", optional)]
    pub help_text: String,
    /// Whether the omission is recoverable ("herstelbaar"). Rendered as a
    /// checkbox: when it is unchecked the browser submits nothing, so serde
    /// falls back to `false` here, explicitly marking the omission irreparable.
    #[serde(default)]
    pub recoverable: bool,
    /// The electoral districts selected for a CandidateList omission.
    /// Ignored for other omission types; validated in the handler.
    #[validate(ignore)]
    pub electoral_districts: Vec<ElectoralDistrict>,
    /// The candidate lists selected for a Candidate omission.
    /// Ignored for other omission types; validated in the handler.
    #[validate(ignore)]
    pub candidate_lists: Vec<CandidateListId>,
}

impl Default for OmissionForm {
    fn default() -> Self {
        OmissionForm {
            title: String::new(),
            description: String::new(),
            help_text: String::new(),
            // A fresh omission is recoverable unless the committee marks it
            // otherwise (the common case); presets override this via the dialog.
            recoverable: true,
            electoral_districts: Vec::new(),
            candidate_lists: Vec::new(),
        }
    }
}

impl From<Omission> for OmissionForm {
    fn from(value: Omission) -> Self {
        OmissionForm {
            title: value.title.to_string(),
            description: value.description.to_string(),
            help_text: value
                .help_text()
                .map(ToString::to_string)
                .unwrap_or_default(),
            recoverable: value.recoverable,
            electoral_districts: Vec::new(),
            candidate_lists: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchecked_recoverable_checkbox_submits_as_false() {
        // An unchecked checkbox is omitted from the submitted body, which must
        // mark the omission irreparable rather than falling back to the form's
        // recoverable-by-default value.
        let form: OmissionForm =
            serde_urlencoded::from_str("title=t&description=d&help_text=").unwrap();
        assert!(!form.recoverable);
    }

    #[test]
    fn checked_recoverable_checkbox_submits_as_true() {
        let form: OmissionForm =
            serde_urlencoded::from_str("title=t&description=d&recoverable=true").unwrap();
        assert!(form.recoverable);
    }

    #[test]
    fn fresh_form_defaults_to_recoverable() {
        assert!(OmissionForm::default().recoverable);
    }

    #[test]
    fn title_rejects_line_breaks() {
        let form = OmissionForm {
            title: "titel met\nregeleinde".to_string(),
            description: "omschrijving".to_string(),
            ..OmissionForm::default()
        };
        let errors = form.validate_create().expect_err("newline in title");
        assert!(errors.errors().iter().any(|(field, _)| field == "title"));
    }

    #[test]
    fn description_allows_line_breaks_but_rejects_other_control_chars() {
        let form = OmissionForm {
            title: "titel".to_string(),
            // A textarea submits \r\n line breaks; those must stay valid.
            description: "regel één\r\nregel twee".to_string(),
            help_text: "notitie\nmet regels".to_string(),
            ..OmissionForm::default()
        };
        form.validate_create().expect("line breaks are valid");

        let form = OmissionForm {
            title: "titel".to_string(),
            description: "omschrijving met \u{0008} backspace".to_string(),
            ..OmissionForm::default()
        };
        let errors = form.validate_create().expect_err("control char");
        assert!(
            errors
                .errors()
                .iter()
                .any(|(field, _)| field == "description")
        );
    }
}
