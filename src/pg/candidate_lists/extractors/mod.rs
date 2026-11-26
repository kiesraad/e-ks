use serde::Deserialize;

use crate::structs::candidate_lists::CandidateListId;

mod candidate_list;
mod file_form;
mod full_candidate_list;

pub(crate) use file_form::FileForm;

#[derive(Deserialize)]
struct CandidateListPathParams {
    #[serde(alias = "list_id")]
    list_id: CandidateListId,
}
