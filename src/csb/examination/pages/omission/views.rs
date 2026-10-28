use askama::Template;
use axum_extra::routing::TypedPath;

use crate::{
    AppError, Context, CsbStoreData, ElectionConfig, ElectoralDistrict, Overlay,
    csb::examination::OmissionForm,
    filters,
    form::FormData,
    projection::WithCorrections,
    structs::{
        candidate_lists::CandidateListId,
        csb::{Omission, OmissionPlaceholders, OmissionType},
        persons::PersonId,
    },
};

use super::OmissionTarget;

/// One selectable candidate list in the add-omission dialog for candidate omissions.
pub(super) struct CandidateListOption {
    pub(super) id: CandidateListId,
    /// Dutch label derived from the list's electoral districts (e.g. "1. Groningen").
    pub(super) label: String,
    /// Whether this omission can also be applied to this candidate list. False
    /// if the position on the list is different from the currently selected
    /// list.
    pub(super) applicable: bool,
}

/// The add-omission form tab of the dialog.
#[derive(Template)]
#[template(path = "csb/examination/pages/omission.html")]
pub(super) struct CsbAddOmissionTemplate {
    pub(super) form: FormData<OmissionForm>,
    pub(super) overlay: Overlay,
    pub(super) close_action: String,
    pub(super) presets: Vec<PresetGroupView>,
    pub(super) omission_target: OmissionTarget,
    /// Districts that appear on at least one paper-corrected candidate list of
    /// this political group. The districts section is hidden when this is
    /// empty. Districts absent from all lists are shown disabled so the user
    /// cannot select them.
    pub(super) available_districts: Vec<ElectoralDistrict>,
    /// Candidate lists for a Candidate omission. Hidden when empty.
    pub(super) available_candidate_lists: Vec<CandidateListOption>,
    pub(super) title_suffix: String,
}

impl CsbAddOmissionTemplate {
    /// Returns true when the position of the candidate is the same on all lists
    /// and the omission can thus be applied to all lists
    fn all_candidate_lists_apply(&self) -> bool {
        self.available_candidate_lists
            .iter()
            .all(|candidate_list_option| candidate_list_option.applicable)
    }
}

/// The overview tab of the dialog: the omissions already added to this entity.
#[derive(Template)]
#[template(path = "csb/examination/pages/omission_overview.html")]
pub(super) struct CsbOmissionOverviewTemplate {
    pub(super) overlay: Overlay,
    /// Where the close button returns to.
    pub(super) close_action: String,
    /// The omissions already added to this entity, each with a remove action.
    pub(super) omissions: Vec<OmissionView>,
    /// target to generate urls from, for the steps sidebar.
    pub(super) omission_target: OmissionTarget,
    pub(super) title_suffix: String,
}

/// An omission in the overview tab; the URL of its remove action is derived
/// from the dialog's target via `remove_url`.
pub(super) struct OmissionView {
    omission: Omission,
    /// Formatted district string for display (e.g. "1. Groningen, 2. Friesland").
    districts: String,
}

impl OmissionView {
    fn remove_url(&self, omission_target: &OmissionTarget) -> impl TypedPath {
        omission_target.delete_url(self.omission.id)
    }
}

/// A preset shown in the dialog, with the `{token}` placeholders in its texts
/// already filled from the referenced item (the rest left for the dialog or
/// manual entry).
pub(super) struct PresetView {
    title: String,
    description: String,
    help_text: String,
    /// Whether this preset describes a recoverable omission ("herstelbaar").
    recoverable: bool,
}

/// The presets of one category, listed together under [`Self::title`].
pub(super) struct PresetGroupView {
    title: Option<String>,
    presets: Vec<PresetView>,
}

/// Resolve the placeholder values that can be derived from the referenced item.
fn placeholders_for(target: &OmissionTarget, data: &CsbStoreData) -> OmissionPlaceholders {
    match target.omission_type {
        OmissionType::Candidate => {
            let person = PersonId::from(target.reference);
            OmissionPlaceholders {
                candidate_name: data
                    .pg(WithCorrections::All)
                    .get_person(person)
                    .ok()
                    .map(|person| person.name.display()),
                // A candidate's position differs per list, so it can only be
                // resolved when the dialog was opened for a specific list.
                candidate_number: target
                    .list
                    .and_then(|list| {
                        data.pg(WithCorrections::All)
                            .get_candidate_position(list, person)
                    })
                    .map(|nr| nr.to_string()),
                districts: None,
            }
        }
        // With a single district the district selector is hidden, so the
        // district tokens are filled here; otherwise the dialog fills them from
        // the selected checkboxes.
        OmissionType::DeclarationsOfSupport => OmissionPlaceholders {
            districts: match available_electoral_districts(data).as_slice() {
                [district] => Some(district.title().to_string()),
                _ => None,
            },
            ..OmissionPlaceholders::default()
        },
        OmissionType::CandidateList | OmissionType::PoliticalGroup | OmissionType::Appellation => {
            OmissionPlaceholders::default()
        }
    }
}

/// Candidate lists of the political group, optionally filtered to those
/// containing a specific candidate. When `person_filter` is `None`, returns all
/// lists; when `Some`, returns only lists this candidate appears on.
///
/// When the dialog was opened from a list (`current_list`), lists on which the
/// candidate has a different position are marked as not applicable, since the
/// omission refers to the candidate's position on the current list.
pub(super) fn candidate_list_options(
    data: &CsbStoreData,
    person_filter: Option<PersonId>,
    current_list: Option<CandidateListId>,
) -> Vec<CandidateListOption> {
    let pg = data.pg(WithCorrections::All);
    let current_position = person_filter
        .zip(current_list)
        .and_then(|(person, list)| pg.get_candidate_position(list, person));

    pg.get_candidate_lists()
        .into_iter()
        .filter(|l| person_filter.is_none_or(|id| l.candidates.contains(&id)))
        .map(|l| CandidateListOption {
            id: l.id,
            label: l.districts_name(),
            applicable: match (person_filter, current_position) {
                (Some(person), Some(position)) => l.position_of(person) == Some(position),
                _ => true,
            },
        })
        .collect()
}

/// All paper-corrected candidate list districts of the political group
/// for the candidate list omission form (mainly declarations of support)
pub(super) fn available_electoral_districts(data: &CsbStoreData) -> Vec<ElectoralDistrict> {
    let mut districts: Vec<_> = data
        .pg(WithCorrections::All)
        .get_candidate_lists()
        .into_iter()
        .flat_map(|l| l.electoral_districts.iter().copied())
        .collect();
    districts.sort();
    districts.dedup();
    districts
}

pub(super) fn preset_views(target: &OmissionTarget, data: &CsbStoreData) -> Vec<PresetGroupView> {
    let placeholders = placeholders_for(target, data);

    let mut groups: Vec<PresetGroupView> = Vec::new();
    for preset in target.omission_type.presets() {
        let view = PresetView {
            title: preset.title.clone(),
            description: placeholders.interpolate(&preset.description),
            help_text: placeholders.interpolate(&preset.help_text),
            recoverable: preset.recoverable,
        };

        match groups.iter_mut().find(|group| group.title == preset.group) {
            Some(group) => group.presets.push(view),
            None => groups.push(PresetGroupView {
                title: preset.group.clone(),
                presets: vec![view],
            }),
        }
    }

    groups.sort_by_key(|group| group.title.is_none());
    groups
}

/// The omissions already added to the entity the dialog was opened for, shown
/// on the overview tab. A candidate lists every omission for the person, both
/// list-scoped and general.
pub(super) fn omission_views(
    target: &OmissionTarget,
    data: &CsbStoreData,
    election: ElectionConfig,
) -> Result<Vec<OmissionView>, AppError> {
    let omissions = match target.omission_type {
        OmissionType::PoliticalGroup => data.get_political_group_omissions(),
        OmissionType::CandidateList => {
            data.get_candidate_list_omissions(CandidateListId::from(target.reference), election)?
        }
        OmissionType::DeclarationsOfSupport => {
            data.get_all_declarations_of_support_omissions(election)
        }
        OmissionType::Candidate => {
            data.get_candidate_omissions(PersonId::from(target.reference), None, election)
        }
        OmissionType::Appellation => data.get_appellation_omissions(),
    };

    let mut views = Vec::with_capacity(omissions.len());
    for omission in omissions {
        let districts = omission.category.electoral_district(data, &election)?;
        views.push(OmissionView {
            omission,
            districts,
        });
    }
    Ok(views)
}
