use axum::Router;
use axum_extra::routing::RouterExt;

use crate::AppRequestState;

use super::paths::{
    CsbAddOmissionPath, CsbAllBrpFindingsPath, CsbAllRestorationsPath,
    CsbAppellationCorrectionPath, CsbBrpCheckPath, CsbCandidateBrpCheckPath, CsbCandidateListPath,
    CsbCandidatePath, CsbDeleteOmissionPath, CsbExaminationOverviewPath, CsbGeneralInformationPath,
    CsbI1DownloadPath, CsbI4DownloadPath, CsbOmissionOverviewPath, CsbPaperCorrectionsStartPath,
    CsbPaperCorrectionsStopPath, CsbPersonCorrectionPath, CsbPoliticalGroupPath,
    CsbPoliticalGroupToggleFinishPath, OmissionListQuery, PgIndexPath,
};

mod all_brp_findings;
mod all_restorations;
pub(in crate::csb) mod candidate;
pub(in crate::csb) mod candidate_list;
mod correction;
mod delete;
pub(in crate::csb) mod general_information;
mod i1;
mod i4;
mod omission;
mod overview;
mod paper_corrections;
pub(in crate::csb) mod political_group;

pub fn router<S: AppRequestState>() -> Router<S> {
    Router::new()
        .typed_get(overview::overview)
        .typed_get(i1::gen_i1::<S>)
        .typed_get(i4::gen_i4::<S>)
        .typed_get(political_group::overview)
        .typed_post(political_group::toggle_examination_finish)
        .typed_post(political_group::start_brp_check::<S>)
        .typed_get(delete::delete)
        .typed_post(delete::delete_submit)
        .typed_get(general_information::overview)
        .typed_post(paper_corrections::start_paper_corrections::<S>)
        .typed_post(paper_corrections::stop_paper_corrections::<S>)
        .typed_get(candidate_list::overview)
        .typed_get(candidate::overview)
        .typed_post(candidate::check_against_brp::<S>)
        .typed_get(omission::add_omission)
        .typed_post(omission::add_omission_submit)
        .typed_get(omission::overview)
        .typed_post(omission::delete_omission)
        .typed_get(all_restorations::all_restorations)
        .typed_get(all_brp_findings::all_brp_findings)
        .typed_get(correction::appellation_name_correction)
        .typed_post(correction::appellation_correction_submit)
        .typed_get(correction::person_correction)
        .typed_post(correction::person_correction_submit)
}
