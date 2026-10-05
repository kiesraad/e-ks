mod actions;
pub(in crate::csb) mod extractors;
mod forms;
pub(in crate::csb) mod numbering;
pub(in crate::csb) mod pages;
pub(in crate::csb) mod paths;
pub(in crate::csb) mod structs;

pub use forms::OmissionForm;
pub use pages::router;
pub use paths::{
    CsbExaminationOverviewPath, CsbFinishExaminationPath, CsbHearingDetailsPath,
    CsbPoliticalGroupPath,
};
