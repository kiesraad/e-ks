use serde::{Deserialize, Serialize};
use validate::Validate;

use crate::{
    OptionStringExt,
    structs::common::{FirstName, FullName, Initials, LastName, LastNamePrefix},
};

/// A name form containing only the initials and last name (with optional prefix)
#[derive(Default, Serialize, Deserialize, Clone, Debug, Validate)]
#[validate(target = "FullName")]
#[serde(default)]
pub struct MinimalNameForm {
    #[validate(parse = "LastName")]
    pub last_name: String,
    #[validate(parse = "LastNamePrefix", optional)]
    pub last_name_prefix: String,
    #[validate(parse = "Initials", required)]
    pub initials: String,
}

impl MinimalNameForm {
    /// The `MinimalNameForm` has no first name field
    pub fn first_name_opt(&self) -> Option<&str> {
        None
    }

    pub fn is_empty(&self) -> bool {
        self.last_name.is_empty() && self.last_name_prefix.is_empty() && self.initials.is_empty()
    }
}

impl From<FullName> for MinimalNameForm {
    fn from(name: FullName) -> Self {
        MinimalNameForm {
            last_name: name.last_name.to_string(),
            last_name_prefix: name.last_name_prefix.to_string_or_default(),
            initials: name.initials.to_string_or_default(),
        }
    }
}

#[derive(Default, Serialize, Deserialize, Clone, Debug, Validate)]
#[validate(target = "FullName")]
#[serde(default)]
pub struct FullNameForm {
    #[validate(parse = "FirstName", optional)]
    pub first_name: String,
    #[validate(parse = "LastName")]
    pub last_name: String,
    #[validate(parse = "LastNamePrefix", optional)]
    pub last_name_prefix: String,
    #[validate(parse = "Initials", required)]
    pub initials: String,
}

impl From<FullName> for FullNameForm {
    fn from(name: FullName) -> Self {
        FullNameForm {
            first_name: name.first_name.to_string_or_default(),
            last_name: name.last_name.to_string(),
            last_name_prefix: name.last_name_prefix.to_string_or_default(),
            initials: name.initials.to_string_or_default(),
        }
    }
}

impl FullNameForm {
    /// The `FullNameForm` does have a first name field
    pub fn first_name_opt(&self) -> Option<&str> {
        Some(&self.first_name)
    }
}
