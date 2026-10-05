mod actions;
pub(in crate::csb) mod extractors;
mod forms;
pub(in crate::csb) mod numbering;
pub(in crate::csb) mod pages;
mod paths;
pub(in crate::csb) mod structs;

pub use forms::OmissionForm;
pub use numbering::ListNumbering;
pub use pages::router;
pub use paths::{
    CsbExaminationOverviewPath, CsbFinishExaminationPath, CsbHearingDetailsPath,
    CsbI4DocxDownloadPath, CsbI4DownloadPath, CsbPoliticalGroupPath,
};
