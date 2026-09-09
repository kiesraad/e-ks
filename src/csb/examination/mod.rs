mod actions;
pub(in crate::csb) mod extractors;
mod forms;
mod model_inputs;
pub(in crate::csb) mod pages;
mod paths;
pub(in crate::csb) mod structs;

pub use forms::OmissionForm;
pub use pages::router;
pub use paths::{
    CsbExaminationOverviewPath, CsbFinishExaminationPath, CsbHearingDetailsPath, CsbI4DownloadPath,
    CsbPoliticalGroupPath,
};
