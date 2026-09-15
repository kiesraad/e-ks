use serde::{Deserialize, Serialize};

use crate::id_newtype;

id_newtype!(pub struct ObjectionId);

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct Objection {
    pub id: ObjectionId,
    pub objection_text: String,
}
