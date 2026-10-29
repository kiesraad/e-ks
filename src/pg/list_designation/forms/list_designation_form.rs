use serde::Deserialize;
use validate::Validate;

use crate::structs::list_designation::ListDesignationKind;

#[derive(Default, Debug, Clone)]
pub struct ListDesignationTarget {
    pub list_designation_type: ListDesignationKind,
}

impl From<Option<ListDesignationKind>> for ListDesignationTarget {
    fn from(value: Option<ListDesignationKind>) -> Self {
        Self {
            list_designation_type: value.unwrap_or_default(),
        }
    }
}

#[derive(Default, Deserialize, Debug, Validate)]
#[validate(target = "ListDesignationTarget")]
#[serde(default)]
pub struct ListDesignationForm {
    #[validate(parse = "ListDesignationKind")]
    pub list_designation_type: String,
}

impl From<Option<ListDesignationKind>> for ListDesignationForm {
    fn from(value: Option<ListDesignationKind>) -> Self {
        ListDesignationForm {
            list_designation_type: value.map(|d| d.to_string()).unwrap_or_default(),
        }
    }
}
