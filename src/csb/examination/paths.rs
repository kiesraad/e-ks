//! Typed paths for CSB examination routes and path helpers on
//! [`CsbPoliticalGroup`].

use axum_extra::routing::TypedPath;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    AppError, QueryParamState, StreamId,
    csb::{
        examination::{extractors::CsbPoliticalGroup, structs::CandidateCorrectionField},
        recovery::paths::{
            CsbRecoveryCandidateListPath, CsbRecoveryCandidatePath,
            CsbRecoveryGeneralInformationPath, CsbRecoveryOmissionsPath, CsbRecoveryOverviewPath,
            CsbRecoveryPoliticalGroupPath,
        },
    },
    structs::{
        candidate_lists::CandidateListId,
        csb::{CsbPhase, OmissionId, OmissionType},
        persons::PersonId,
    },
};

#[derive(TypedPath)]
#[typed_path("/", rejection(AppError))]
pub struct PgIndexPath;

#[derive(TypedPath)]
#[typed_path("/csb/examination", rejection(AppError))]
pub struct CsbExaminationOverviewPath;

#[derive(TypedPath)]
#[typed_path("/csb/examination/i1.pdf", rejection(AppError))]
pub struct CsbI1DownloadPath;

#[derive(TypedPath)]
#[typed_path("/csb/examination/i4.pdf", rejection(AppError))]
pub struct CsbI4DownloadPath;

#[derive(TypedPath, Deserialize)]
#[typed_path("/csb/examination/{stream_id}", rejection(AppError))]
pub struct CsbPoliticalGroupPath {
    pub stream_id: StreamId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path("/csb/examination/{stream_id}/toggle-finish", rejection(AppError))]
pub struct CsbPoliticalGroupToggleFinishPath {
    pub stream_id: StreamId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path("/csb/examination/{stream_id}/brp-errors", rejection(AppError))]
pub struct CsbAllBrpFindingsPath {
    pub stream_id: StreamId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path("/csb/examination/{stream_id}/brp-check", rejection(AppError))]
pub struct CsbBrpCheckPath {
    pub stream_id: StreamId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path("/csb/examination/{stream_id}/delete", rejection(AppError))]
pub struct CsbPoliticalGroupDeletePath {
    pub stream_id: StreamId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path(
    "/csb/examination/{stream_id}/general-information",
    rejection(AppError)
)]
pub struct CsbGeneralInformationPath {
    pub stream_id: StreamId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path("/csb/examination/{stream_id}/paper-corrections", rejection(AppError))]
pub struct CsbPaperCorrectionsStartPath {
    pub stream_id: StreamId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path(
    "/csb/examination/{stream_id}/paper-corrections/stop",
    rejection(AppError)
)]
pub struct CsbPaperCorrectionsStopPath {
    pub stream_id: StreamId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path("/csb/examination/{stream_id}/list/{list_id}", rejection(AppError))]
pub struct CsbCandidateListPath {
    pub stream_id: StreamId,
    pub list_id: CandidateListId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path(
    "/csb/examination/{stream_id}/list/{list_id}/candidate/{person_id}",
    rejection(AppError)
)]
pub struct CsbCandidatePath {
    pub stream_id: StreamId,
    pub list_id: CandidateListId,
    pub person_id: PersonId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path(
    "/csb/examination/{stream_id}/list/{list_id}/candidate/{person_id}/brp-check",
    rejection(AppError)
)]
pub struct CsbCandidateBrpCheckPath {
    pub stream_id: StreamId,
    pub list_id: CandidateListId,
    pub person_id: PersonId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path(
    "/csb/examination/{stream_id}/omission/{omission_type}/{reference}",
    rejection(AppError)
)]
pub struct CsbAddOmissionPath {
    pub stream_id: StreamId,
    pub omission_type: OmissionType,
    pub reference: Uuid,
}

#[derive(TypedPath, Deserialize)]
#[typed_path(
    "/csb/examination/{stream_id}/omission/{omission_type}/{reference}/overview",
    rejection(AppError)
)]
pub struct CsbOmissionOverviewPath {
    pub stream_id: StreamId,
    pub omission_type: OmissionType,
    pub reference: Uuid,
}

#[derive(TypedPath, Deserialize)]
#[typed_path(
    "/csb/examination/{stream_id}/delete-omission/{omission_id}",
    rejection(AppError)
)]
pub struct CsbDeleteOmissionPath {
    pub stream_id: StreamId,
    pub omission_id: OmissionId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path("/csb/examination/{stream_id}/omissions", rejection(AppError))]
pub struct CsbAllRestorationsPath {
    pub stream_id: StreamId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path(
    "/csb/examination/{stream_id}/correction/appellation",
    rejection(AppError)
)]
pub struct CsbAppellationCorrectionPath {
    pub stream_id: StreamId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path(
    "/csb/examination/{stream_id}/correction/person/{person_id}/{field}",
    rejection(AppError)
)]
pub struct CsbPersonCorrectionPath {
    pub stream_id: StreamId,
    pub person_id: PersonId,
    pub field: CandidateCorrectionField,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
pub struct OmissionListQuery {
    /// The candidate list the omission dialog was opened from. Used to resolve
    /// the candidate's position for the preset placeholders and to return to the
    /// candidate detail page, which is always scoped to a list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<CandidateListId>,
}

/// The navigation helpers below dispatch on the group's [`CsbPhase`] mode, so
/// the shared examination templates link within the phase they render for.
/// Helpers for examination-only actions (omissions, corrections, paper
/// corrections, delete, toggle-finish) stay single-phase: templates only call
/// them from examination-mode branches.
impl CsbPoliticalGroup {
    /// Translation key naming the phase, for the overview breadcrumb.
    pub fn overview_title_key(&self) -> &'static str {
        match self.mode {
            CsbPhase::Examination => "csb.examination",
            CsbPhase::Recovery => "csb.recovery.title",
        }
    }

    /// Path to the phase's overview page listing all political groups.
    pub fn overview_path(&self) -> String {
        match self.mode {
            CsbPhase::Examination => CsbExaminationOverviewPath.to_string(),
            CsbPhase::Recovery => CsbRecoveryOverviewPath.to_string(),
        }
    }

    /// Path to this group's detail page within the phase.
    pub fn group_path(&self) -> String {
        match self.mode {
            CsbPhase::Examination => CsbPoliticalGroupPath {
                stream_id: self.stream_id,
            }
            .to_string(),
            CsbPhase::Recovery => CsbRecoveryPoliticalGroupPath {
                stream_id: self.stream_id,
            }
            .to_string(),
        }
    }

    pub fn delete_path(&self) -> impl TypedPath {
        CsbPoliticalGroupDeletePath {
            stream_id: self.stream_id,
        }
    }

    pub fn examination_toggle_finish_path(
        &self,
        redirect_to: impl std::fmt::Display,
    ) -> impl TypedPath {
        CsbPoliticalGroupToggleFinishPath {
            stream_id: self.stream_id,
        }
        .with_query_params(QueryParamState::redirect_to(redirect_to.to_string()))
    }

    pub fn general_information_path(&self) -> String {
        match self.mode {
            CsbPhase::Examination => CsbGeneralInformationPath {
                stream_id: self.stream_id,
            }
            .to_string(),
            CsbPhase::Recovery => CsbRecoveryGeneralInformationPath {
                stream_id: self.stream_id,
            }
            .to_string(),
        }
    }

    /// Path that puts the session in paper-corrections mode for this stream.
    pub fn all_brp_findings_path(&self) -> impl TypedPath {
        CsbAllBrpFindingsPath {
            stream_id: self.stream_id,
        }
    }

    pub fn start_brp_check_path(&self) -> impl TypedPath {
        CsbBrpCheckPath {
            stream_id: self.stream_id,
        }
    }

    pub fn start_paper_corrections_path(&self) -> impl TypedPath {
        CsbPaperCorrectionsStartPath {
            stream_id: self.stream_id,
        }
    }

    /// Path to the dialog that adds a general (political group level) omission.
    pub fn add_political_group_omission_path(&self) -> impl TypedPath {
        CsbAddOmissionPath {
            stream_id: self.stream_id,
            omission_type: OmissionType::PoliticalGroup,
            reference: self.stream_id.into(),
        }
    }

    /// Path to the overview page listing the general (political group level)
    /// omissions already added.
    pub fn manage_political_group_omissions_path(&self) -> impl TypedPath {
        CsbOmissionOverviewPath {
            stream_id: self.stream_id,
            omission_type: OmissionType::PoliticalGroup,
            reference: self.stream_id.into(),
        }
    }

    /// Path to the add-omission dialog for declarations of support.
    pub fn add_declarations_of_support_omission_path(&self) -> impl TypedPath {
        CsbAddOmissionPath {
            stream_id: self.stream_id,
            omission_type: OmissionType::DeclarationsOfSupport,
            reference: self.stream_id.into(),
        }
    }

    /// Path to the overview page listing declarations-of-support omissions.
    pub fn manage_declarations_of_support_omissions_path(&self) -> impl TypedPath {
        CsbOmissionOverviewPath {
            stream_id: self.stream_id,
            omission_type: OmissionType::DeclarationsOfSupport,
            reference: self.stream_id.into(),
        }
    }

    /// Path to the dialog that adds an omission to a specific candidate list.
    pub fn add_candidate_list_omission_path(&self, list: &CandidateListId) -> impl TypedPath {
        CsbAddOmissionPath {
            stream_id: self.stream_id,
            omission_type: OmissionType::CandidateList,
            reference: (*list).into(),
        }
    }

    /// Path to the overview page listing the omissions already added to this
    /// candidate list.
    pub fn manage_candidate_list_omissions_path(&self, list: &CandidateListId) -> impl TypedPath {
        CsbOmissionOverviewPath {
            stream_id: self.stream_id,
            omission_type: OmissionType::CandidateList,
            reference: (*list).into(),
        }
    }

    /// Path to the candidate list page for a specific list.
    pub fn candidate_list_path(&self, list: &CandidateListId) -> String {
        match self.mode {
            CsbPhase::Examination => CsbCandidateListPath {
                stream_id: self.stream_id,
                list_id: *list,
            }
            .to_string(),
            CsbPhase::Recovery => CsbRecoveryCandidateListPath {
                stream_id: self.stream_id,
                list_id: *list,
            }
            .to_string(),
        }
    }

    /// Path to the detail page of a candidate on a specific list.
    pub fn candidate_path(&self, list: &CandidateListId, person: &PersonId) -> String {
        match self.mode {
            CsbPhase::Examination => CsbCandidatePath {
                stream_id: self.stream_id,
                list_id: *list,
                person_id: *person,
            }
            .to_string(),
            CsbPhase::Recovery => CsbRecoveryCandidatePath {
                stream_id: self.stream_id,
                list_id: *list,
                person_id: *person,
            }
            .to_string(),
        }
    }

    /// Path that re-checks one candidate against the BRP.
    pub fn candidate_brp_check_path(
        &self,
        list: &CandidateListId,
        person: &PersonId,
    ) -> impl TypedPath {
        CsbCandidateBrpCheckPath {
            stream_id: self.stream_id,
            list_id: *list,
            person_id: *person,
        }
    }

    /// Path to the dialog that adds an omission to a candidate. The list is
    /// carried as a query parameter so the candidate's position on it can be
    /// resolved for the preset placeholders and to return to this page after.
    pub fn add_candidate_omission_path(
        &self,
        person: &PersonId,
        list: &CandidateListId,
    ) -> impl TypedPath {
        CsbAddOmissionPath {
            stream_id: self.stream_id,
            omission_type: OmissionType::Candidate,
            reference: (*person).into(),
        }
        .with_query_params(OmissionListQuery { list: Some(*list) })
    }

    /// Path to the overview page listing the omissions already added for this
    /// candidate (both list-scoped and general). The `list` is carried so the
    /// overview can link back to the add form and return page for the list.
    pub fn manage_candidate_omissions_path(
        &self,
        person: &PersonId,
        list: &CandidateListId,
    ) -> impl TypedPath {
        CsbOmissionOverviewPath {
            stream_id: self.stream_id,
            omission_type: OmissionType::Candidate,
            reference: (*person).into(),
        }
        .with_query_params(OmissionListQuery { list: Some(*list) })
    }

    /// Path to the page listing all omissions and corrections of this group:
    /// the "Alle verzuimen" page during examination, the recovery todo page in
    /// the "Herstelde lijsten" phase.
    pub fn all_restorations_path(&self) -> String {
        match self.mode {
            CsbPhase::Examination => CsbAllRestorationsPath {
                stream_id: self.stream_id,
            }
            .to_string(),
            CsbPhase::Recovery => CsbRecoveryOmissionsPath {
                stream_id: self.stream_id,
            }
            .to_string(),
        }
    }

    /// Path recording the recovered / not-recovered decision for an omission
    /// in the recovery phase, returning to the page the control was on.
    pub fn set_omission_status_path(
        &self,
        omission_id: &OmissionId,
        redirect_to: impl std::fmt::Display,
    ) -> String {
        crate::csb::recovery::paths::CsbSetOmissionStatusPath {
            stream_id: self.stream_id,
            omission_id: *omission_id,
        }
        .with_query_params(QueryParamState::redirect_to(redirect_to.to_string()))
        .to_string()
    }

    /// Path to the correction overlay for the political group appellation.
    pub fn correction_appellation_path(&self) -> impl TypedPath {
        CsbAppellationCorrectionPath {
            stream_id: self.stream_id,
        }
    }

    /// Path to the correction overlay for a specific personal-data field of a
    /// candidate. The `list` is carried as a query parameter so the overlay can
    /// return to the candidate's detail page after saving.
    pub fn correction_person_path(
        &self,
        person: &PersonId,
        field: CandidateCorrectionField,
        list: &CandidateListId,
    ) -> impl TypedPath {
        CsbPersonCorrectionPath {
            stream_id: self.stream_id,
            person_id: *person,
            field,
        }
        .with_query_params(OmissionListQuery { list: Some(*list) })
    }

    pub fn correction_person_path_from_all_restorations(
        &self,
        person: &PersonId,
        field: CandidateCorrectionField,
    ) -> impl TypedPath {
        CsbPersonCorrectionPath {
            stream_id: self.stream_id,
            person_id: *person,
            field,
        }
        .with_query_params(QueryParamState::redirect_to(
            self.all_restorations_path().to_string(),
        ))
    }
}

#[cfg(test)]
mod prefix_guard {
    use super::CsbPaperCorrectionsStopPath;
    use crate::{StreamId, view::CSB_PAPER_CORRECTIONS_STOP_PREFIX};

    /// `view::Context` builds the paper-corrections exit link from a prefix
    /// constant; this keeps the two in sync.
    #[test]
    fn stop_path_matches_shared_prefix() {
        let stream_id = StreamId::new();
        let expected =
            format!("{CSB_PAPER_CORRECTIONS_STOP_PREFIX}/{stream_id}/paper-corrections/stop");
        assert_eq!(
            CsbPaperCorrectionsStopPath { stream_id }.to_string(),
            expected
        );
    }
}
