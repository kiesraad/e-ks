use serde::Deserialize;
use validate::Validate;

use crate::structs::csb::Objection;

#[derive(Default, Deserialize, Debug, Validate)]
#[validate(target = "Objection")]
#[serde(default)]
pub struct ObjectionForm {
    #[validate(not_empty)]
    pub objection_text: String,
}

impl From<Objection> for ObjectionForm {
    fn from(value: Objection) -> Self {
        ObjectionForm {
            objection_text: value.objection_text,
        }
    }
}
