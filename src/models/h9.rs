//! Model H 9: Instemmingsverklaring / Ynstimmingsferklearring. The document
//! text lives in the `templates/h9.*.md` Markdown templates.

use eks_utils::slugify_teletex;
use textris_pdf::build::Textris;

use super::{
    Pdf,
    inputs::{DetailedCandidate, ElectoralDistricts, ModelData},
    layout::h_document,
    markdown::{filters, model_template},
};
use crate::{
    AppError,
    core::{ElectionType, ModelLocale},
};

#[derive(Debug)]
pub struct H9 {
    pub common: ModelData,
    pub electoral_districts: ElectoralDistricts,
    pub detailed_candidate: DetailedCandidate,
}

model_template!(H9Nl, H9, "models/templates/h9.nl.md");
model_template!(H9Fy, H9, "models/templates/h9.fy.md");

impl Pdf for H9 {
    fn document(&self) -> Result<Textris, AppError> {
        match self.common.locale {
            ModelLocale::Nl => h_document(&self.common, H9Nl(self)),
            ModelLocale::Fry => h_document(&self.common, H9Fy(self)),
        }
    }

    fn filename(&self) -> String {
        format!(
            "h9-{}-{}.pdf",
            self.detailed_candidate.candidate.position,
            slugify_teletex(&self.detailed_candidate.candidate.last_name, true),
        )
    }
}
