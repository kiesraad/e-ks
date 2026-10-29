//! Model H 3: authorisation to place a appellation above a candidate list.
//! Depending on the list designation this renders as H 3-1 (a single
//! designation) or H 3-2 (a combined designation); each variant is
//! its own Markdown template in `templates/`.

use textris_pdf::build::Textris;

use super::{
    Pdf,
    inputs::{ElectoralDistricts, ModelData, NameAuthorisation, Person},
    layout::h_document,
    markdown::{filters, model_template},
};
use crate::{AppError, core::ModelLocale, structs::list_designation::ListDesignation};

#[derive(Debug)]
pub struct H3 {
    pub common: ModelData,
    pub electoral_districts: ElectoralDistricts,
    pub list_designation: ListDesignation,
    pub list_submitter: Person,
    pub name_authorisations: Vec<NameAuthorisation>,
}

model_template!(H31Nl, H3, "models/templates/h3-1.nl.md");
model_template!(H31Fy, H3, "models/templates/h3-1.fy.md");
model_template!(H32Nl, H3, "models/templates/h3-2.nl.md");
model_template!(H32Fy, H3, "models/templates/h3-2.fy.md");

impl Pdf for H3 {
    fn document(&self) -> Result<Textris, AppError> {
        let combined = self.list_designation.is_combined();
        match (self.common.locale, combined) {
            (ModelLocale::Nl, false) => h_document(&self.common, H31Nl(self)),
            (ModelLocale::Fry, false) => h_document(&self.common, H31Fy(self)),
            (ModelLocale::Nl, true) => h_document(&self.common, H32Nl(self)),
            (ModelLocale::Fry, true) => h_document(&self.common, H32Fy(self)),
        }
    }

    fn filename(&self) -> String {
        match (self.common.locale, &self.list_designation) {
            (ModelLocale::Nl, ListDesignation::Combined { .. }) => {
                "h3-2-samengevoegde-aanduiding.pdf"
            }
            (ModelLocale::Fry, ListDesignation::Combined { .. }) => {
                "h3-2-gearfoege-oantsjutting.pdf"
            }
            (ModelLocale::Nl, _) => "h3-1-aanduiding.pdf",
            (ModelLocale::Fry, _) => "h3-1-oantsjutting.pdf",
        }
        .to_string()
    }
}
