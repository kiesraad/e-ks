use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use validate::Validate;

use crate::{ElectoralDistrict, structs::candidate_lists::CandidateList};

#[derive(Default, Serialize, Deserialize, Clone, Debug, Validate)]
#[validate(target = "CandidateList")]
#[serde(default)]
pub struct CandidateListForm {
    #[validate(not_empty)]
    pub electoral_districts: BTreeSet<ElectoralDistrict>,
}

impl From<CandidateList> for CandidateListForm {
    fn from(value: CandidateList) -> Self {
        CandidateListForm {
            electoral_districts: value.electoral_districts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ElectoralDistrict, form::ValidationError};

    #[tokio::test]
    async fn builds_candidate_list() {
        let form = CandidateListForm {
            electoral_districts: BTreeSet::from([ElectoralDistrict::Utrecht]),
        };

        let list = form.validate_create().unwrap();
        assert_eq!(
            list.electoral_districts,
            BTreeSet::from([ElectoralDistrict::Utrecht])
        );
    }

    #[tokio::test]
    async fn rejects_empty_electoral_districts() {
        let form = CandidateListForm {
            electoral_districts: BTreeSet::new(),
        };

        let Err(data) = form.validate_create() else {
            panic!("expected validation errors");
        };

        assert!(data.errors().contains(&(
            "electoral_districts".to_string(),
            ValidationError::ChooseAtLeastOneOption
        )));
    }
}
