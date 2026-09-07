use crate::{
    ElectoralDistrict,
    csb::examination::structs::{BrpCheckState, RestorationStatus},
    structs::candidate_lists::CandidateList,
};

pub struct CsbCandidateList {
    pub list: CandidateList,
    pub brp: BrpCheckState,
    pub restoration_status: RestorationStatus,
    pub is_paper_added: bool,
    /// Whether the whole list is scrapped, by an unresolved list-level
    /// omission or because all of its districts are scrapped. Only rendered in
    /// the recovery ("Herstelde lijsten") phase.
    pub is_scrapped: bool,
    /// The districts of this list that are scrapped, in the list's district
    /// order.
    pub scrapped_districts: Vec<ElectoralDistrict>,
}

impl CsbCandidateList {
    pub fn is_district_scrapped(&self, district: &ElectoralDistrict) -> bool {
        self.scrapped_districts.contains(district)
    }

    /// Whether every district this list was submitted in is scrapped.
    pub fn all_districts_scrapped(&self) -> bool {
        !self.list.electoral_districts.is_empty()
            && self.scrapped_districts.len() == self.list.electoral_districts.len()
    }
}
