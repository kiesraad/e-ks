use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use validate::Validate;

use crate::{ElectoralDistrict, structs::candidate_lists::CandidateList};

#[derive(Default, Serialize, Deserialize, Clone, Debug, Validate)]
#[validate(target = "CandidateList")]
#[serde(default)]
pub struct CandidateListCreateForm {
    #[validate(not_empty)]
    pub electoral_districts: BTreeSet<ElectoralDistrict>,
    #[validate(ignore)]
    pub copy_candidates: bool,
}
