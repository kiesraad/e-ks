use std::{collections::HashMap, sync::LazyLock};

use serde::{Deserialize, Serialize};

/// A predefined omission ("verzuim") offered as a quick-fill suggestion in the
/// add-omission dialog, split into the model I 1 [`Self::description`] and the
/// [`Self::help_text`] telling the political group how to restore it.
#[derive(Debug, Clone, Deserialize)]
pub struct PresetOmission {
    /// Short title shown in the quick-fill pill.
    pub title: String,
    /// Empty when the texts come from [`Self::district_variants`].
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub help_text: String,
    #[serde(default = "super::recoverable_by_default")]
    pub recoverable: bool,
    #[serde(default)]
    pub group: Option<String>,
    /// Texts that depend on how many districts are selected; the dialog picks
    /// one whenever the district selection changes.
    #[serde(default)]
    pub district_variants: Option<DistrictVariants>,
}

/// The title and texts an omission is filled with.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PresetText {
    pub title: String,
    pub description: String,
    pub help_text: String,
}

impl PresetText {
    fn interpolate(&self, placeholders: &OmissionPlaceholders) -> Self {
        PresetText {
            title: self.title.clone(),
            description: placeholders.interpolate(&self.description),
            help_text: placeholders.interpolate(&self.help_text),
        }
    }
}

/// The texts for one, several or all of the available districts.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DistrictVariants {
    pub one: PresetText,
    pub multiple: PresetText,
    pub all: PresetText,
}

impl DistrictVariants {
    pub fn interpolate(&self, placeholders: &OmissionPlaceholders) -> Self {
        DistrictVariants {
            one: self.one.interpolate(placeholders),
            multiple: self.multiple.interpolate(placeholders),
            all: self.all.interpolate(placeholders),
        }
    }
}

impl PresetOmission {
    /// Every title and text set this preset can fill the form with.
    pub fn texts(&self) -> Vec<PresetText> {
        match &self.district_variants {
            Some(v) => vec![v.one.clone(), v.multiple.clone(), v.all.clone()],
            None => vec![PresetText {
                title: self.title.clone(),
                description: self.description.clone(),
                help_text: self.help_text.clone(),
            }],
        }
    }
}

/// Values used to interpolate the `{token}` placeholders in an omission
/// description with the correct data for the referenced item.
#[derive(Debug, Default, Clone)]
pub struct OmissionPlaceholders {
    /// `{candidate_number}`: the candidate's position on the list.
    pub candidate_number: Option<String>,
    /// `{candidate_name}`: the candidate's initials and last name.
    pub candidate_name: Option<String>,
    /// `{district}` and `{districts}`: the district names the omission applies
    /// to. Only known up front when the group has a single district; otherwise
    /// the dialog fills them from the selected district checkboxes.
    pub districts: Option<String>,
}

impl OmissionPlaceholders {
    /// Replace every placeholder we have a value for, leaving the rest in place
    /// (as `{token}`) for the committee to fill in.
    pub fn interpolate(&self, template: &str) -> String {
        let mut result = template.to_string();
        for (token, value) in [
            ("{candidate_number}", &self.candidate_number),
            ("{candidate_name}", &self.candidate_name),
            ("{district}", &self.districts),
            ("{districts}", &self.districts),
        ] {
            if let Some(value) = value {
                result = result.replace(token, value);
            }
        }
        result
    }
}

/// The standard omissions per omission type, loaded from `omissions.json`.
pub static PRESET_OMISSIONS: LazyLock<HashMap<String, Vec<PresetOmission>>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("omissions.json")).expect("omissions.json should be valid")
});

impl super::OmissionType {
    /// The predefined omissions offered as quick-fill suggestions for this type.
    pub fn presets(self) -> &'static [PresetOmission] {
        PRESET_OMISSIONS
            .get(self.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{form::ValidationError, structs::csb::omission::OmissionType};

    #[test]
    fn presets_are_loaded_from_json_per_type() {
        assert_eq!(OmissionType::Appellation.presets().len(), 2);
        assert_eq!(OmissionType::CandidateList.presets().len(), 4);
        assert_eq!(OmissionType::DeclarationsOfSupport.presets().len(), 2);
        assert_eq!(OmissionType::Candidate.presets().len(), 12);

        // Every preset carries a title and description;
        // irreparable defects have no help text.
        assert!(
            OmissionType::Appellation
                .presets()
                .iter()
                .any(|p| p.help_text.is_empty())
        );
    }

    #[test]
    fn presets_carry_the_recoverable_flag() {
        // Most omissions are recoverable ("herstelbaar").
        assert!(
            OmissionType::Candidate
                .presets()
                .iter()
                .any(|p| p.recoverable)
        );
        // Irreparable defects ("onherstelbaar verzuim") have no help text and are
        // flagged as non-recoverable.
        assert!(
            OmissionType::Appellation
                .presets()
                .iter()
                .any(|p| !p.recoverable)
        );
    }

    #[test]
    fn candidate_presets_are_grouped_in_file_order() {
        let groups: Vec<_> = OmissionType::Candidate
            .presets()
            .iter()
            .map(|preset| preset.group.as_deref())
            .collect();

        let mut seen: Vec<Option<&str>> = Vec::new();
        for group in &groups {
            if seen.last() != Some(group) {
                assert!(
                    !seen.contains(group),
                    "candidate presets of group {group:?} are split up in omissions.json"
                );
                seen.push(*group);
            }
        }

        assert_eq!(
            seen,
            vec![
                Some("Handtekeningen en ID bewijzen"),
                Some("Over kandidaat en gemachtigde"),
                Some("Datum ondertekening en kandidatenlijst"),
            ]
        );
    }

    #[test]
    fn presets_fit_the_omission_field_constraints() {
        // A preset-filled form must pass validation unmodified, so every
        // preset has to parse into the constrained omission types, unless it
        // has placeholders left to fill in.
        use crate::structs::csb::{OmissionText, OmissionTitle};

        fn assert_fits(text: &str) {
            let omission_text = text.parse::<OmissionText>();
            if text.contains(['{', '}']) {
                assert!(
                    matches!(omission_text, Err(ValidationError::ContainsPlaceholder)),
                    "{text:?}"
                )
            } else {
                assert!(omission_text.is_ok(), "{text:?}: {omission_text:?}")
            }
        }

        for presets in PRESET_OMISSIONS.values() {
            for preset in presets {
                preset
                    .title
                    .parse::<OmissionTitle>()
                    .unwrap_or_else(|e| panic!("preset title {:?}: {e:?}", preset.title));
                for text in preset.texts() {
                    text.title
                        .parse::<OmissionTitle>()
                        .unwrap_or_else(|e| panic!("preset title {:?}: {e:?}", text.title));
                    assert_fits(&text.description);
                    if !text.help_text.is_empty() {
                        assert_fits(&text.help_text);
                    }
                }
            }
        }
    }

    #[test]
    fn declarations_of_support_presets_name_the_districts_in_the_help_text_only() {
        // Model I 1 never names districts, the omission letter does.
        let texts: Vec<_> = OmissionType::DeclarationsOfSupport
            .presets()
            .iter()
            .flat_map(PresetOmission::texts)
            .collect();
        for text in &texts {
            assert!(!text.description.contains("{district"), "{:?}", text.title);
        }
        let with_districts: Vec<_> = texts
            .iter()
            .filter(|t| t.help_text.contains("{district}") || t.help_text.contains("{districts}"))
            .map(|t| t.title.as_str())
            .collect();
        assert_eq!(
            with_districts,
            [
                "Voor één kieskring ontbreken ondersteuningsverklaringen",
                "Voor meerdere kieskringen ontbreken ondersteuningsverklaringen",
            ]
        );
    }

    #[test]
    fn interpolate_fills_known_tokens_and_keeps_the_rest() {
        let placeholders = OmissionPlaceholders {
            candidate_number: Some("3".to_string()),
            candidate_name: Some("A.B. de Vries".to_string()),
            districts: None,
        };

        let result = placeholders
            .interpolate("Kandidaat nr. {candidate_number}, {candidate_name} ... {appellation}");

        assert_eq!(
            result,
            // The known tokens are filled; the manual one is left in place.
            "Kandidaat nr. 3, A.B. de Vries ... {appellation}"
        );
    }

    #[test]
    fn interpolate_leaves_all_tokens_without_values() {
        let result =
            OmissionPlaceholders::default().interpolate("nr. {candidate_number} {candidate_name}");

        assert_eq!(result, "nr. {candidate_number} {candidate_name}");
    }

    #[test]
    fn interpolate_fills_both_district_tokens_with_the_same_names() {
        let placeholders = OmissionPlaceholders {
            districts: Some("Utrecht".to_string()),
            ..Default::default()
        };

        assert_eq!(
            placeholders.interpolate("kieskring {district}; kieskring(en) {districts}"),
            "kieskring Utrecht; kieskring(en) Utrecht"
        );
    }
}
