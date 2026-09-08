use axum_extra::routing::TypedPath;

use crate::{
    StreamId,
    csb::examination::{
        extractors::CsbPoliticalGroup,
        pages::{CsbAddOmissionPath, CsbOmissionOverviewPath},
    },
    structs::{
        candidate_lists::CandidateListId,
        csb::{OmissionCategory, OmissionType},
        persons::PersonId,
    },
};

use super::OmissionTarget;

/// The page the dialog returns to: the general information page for political
/// group omissions, the candidate list page for candidate list omissions, the
/// candidate detail page for candidate omissions opened from a specific list,
/// otherwise the political group examination overview.
pub(super) fn return_path(target: &OmissionTarget, political_group: &CsbPoliticalGroup) -> String {
    match target.omission_type {
        OmissionType::PoliticalGroup | OmissionType::Appellation => {
            political_group.general_information_path().to_string()
        }
        OmissionType::CandidateList => political_group
            .candidate_list_path(&CandidateListId::from(target.reference))
            .to_string(),
        OmissionType::DeclarationsOfSupport => political_group.group_path().to_string(),
        // The candidate detail page is scoped to a list, so it can only be the
        // return target when the dialog was opened for a specific list.
        OmissionType::Candidate => match target.list {
            Some(list) => political_group
                .candidate_path(&list, &PersonId::from(target.reference))
                .to_string(),
            None => political_group.group_path().to_string(),
        },
    }
}

/// Query string for links within the dialog: the list context. The overlay
/// marker and `redirect_to` are appended by `Overlay::forward` where the
/// templates render these links.
#[derive(serde::Serialize)]
struct DialogQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    list: Option<CandidateListId>,
}

/// Append the list context as a query string.
fn with_context(path: impl TypedPath, list: Option<CandidateListId>) -> impl TypedPath {
    path.with_query_params(DialogQuery { list })
}

impl OmissionTarget {
    /// The URL of the add-omission form for this entity, keeping the list context
    /// (the sidebar links here from the overview page).
    pub(super) fn add_url(&self) -> impl TypedPath {
        with_context(
            CsbAddOmissionPath {
                stream_id: self.stream_id,
                omission_type: self.omission_type,
                reference: self.reference,
            },
            self.list,
        )
    }

    /// The URL of the overview page for this entity, keeping the list context
    /// (the sidebar links here from the add form).
    pub(super) fn overview_url(&self) -> impl TypedPath + use<> {
        with_context(
            CsbOmissionOverviewPath {
                stream_id: self.stream_id,
                omission_type: self.omission_type,
                reference: self.reference,
            },
            self.list,
        )
    }
}

/// The overview URL to return to after removing an omission, derived from its
/// category so the redirect lands on the overview the omission was listed on.
pub(super) fn overview_url_for(category: &OmissionCategory, stream_id: StreamId) -> impl TypedPath {
    let target = match category {
        OmissionCategory::Candidate { person, lists } => OmissionTarget {
            stream_id,
            omission_type: OmissionType::Candidate,
            reference: (*person).into(),
            list: lists.first().copied(),
        },
        OmissionCategory::CandidateList(lists) if !lists.is_empty() => OmissionTarget {
            stream_id,
            omission_type: OmissionType::CandidateList,
            reference: lists[0].into(),
            list: None,
        },
        OmissionCategory::DeclarationsOfSupport(_) => OmissionTarget {
            stream_id,
            omission_type: OmissionType::DeclarationsOfSupport,
            reference: stream_id.into(),
            list: None,
        },
        _ => OmissionTarget {
            stream_id,
            omission_type: OmissionType::PoliticalGroup,
            reference: stream_id.into(),
            list: None,
        },
    };
    target.overview_url()
}
