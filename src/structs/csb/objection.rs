use serde::{Deserialize, Serialize};

use crate::{id_newtype, structs::common::constrained_strings};

id_newtype!(pub struct ObjectionId);

constrained_strings! {
    /// Free text of an objection raised during the public session.
    pub struct ObjectionText(max = 8000, multiline = true);
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct Objection {
    pub id: ObjectionId,
    pub objection_text: ObjectionText,
}
