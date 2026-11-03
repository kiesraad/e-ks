//! Omissions as the omission pages list them (see [`OmissionRow`]).

use std::collections::BTreeSet;

use crate::{
    CsbStoreData, ElectoralDistrict,
    csb::examination::extractors::CsbPoliticalGroup,
    structs::{
        candidate_lists::CandidateListId,
        csb::{Omission, OmissionPart},
        persons::PersonId,
    },
};

/// An omission as the omission pages list it.
pub struct OmissionRow {
    pub omission: Omission,
    /// Where the title links to; empty for no link.
    pub path: String,
    /// One entry per district or list the omission is decided on, in election
    /// order; empty when it is decided as a whole. A decision on one part
    /// splits the omission (see [`Omission::set_part_status`]), so each part
    /// names the omission holding its decision now: the parts of a split stay
    /// in one row.
    pub parts: Vec<OmissionPartRow>,
}

pub struct OmissionPartRow {
    pub part: OmissionPart,
    pub omission: Omission,
}

impl OmissionPartRow {
    pub fn district(&self) -> Option<ElectoralDistrict> {
        match self.part {
            OmissionPart::ElectoralDistrict(district) => Some(district),
            OmissionPart::CandidateList(_) => None,
        }
    }

    pub fn list(&self) -> Option<&CandidateListId> {
        match &self.part {
            OmissionPart::ElectoralDistrict(_) => None,
            OmissionPart::CandidateList(list_id) => Some(list_id),
        }
    }

    /// The form field the status control posts this part as.
    pub fn field(&self) -> &'static str {
        match self.part {
            OmissionPart::ElectoralDistrict(_) => "electoral_district",
            OmissionPart::CandidateList(_) => "candidate_list",
        }
    }

    pub fn value(&self) -> String {
        match &self.part {
            OmissionPart::ElectoralDistrict(district) => district.serde_name().to_string(),
            OmissionPart::CandidateList(list_id) => list_id.to_string(),
        }
    }
}

impl OmissionRow {
    /// `omissions` as rows, in their order, each linking to `path(omission)`.
    /// In the recovery phase the parts of a split omission (see
    /// [`Omission::has_same_details`]) share one row, so a decision on one
    /// part does not move the others. The examination phase, where nothing
    /// is split yet, lists every omission as it was added.
    pub fn rows(
        omissions: Vec<Omission>,
        political_group: &CsbPoliticalGroup,
        path: impl Fn(&Omission) -> String,
    ) -> Vec<Self> {
        let election = political_group.election;
        let mut rows: Vec<Self> = Vec::new();
        for omission in omissions {
            let parts: Vec<OmissionPartRow> = omission
                .parts(&election)
                .into_iter()
                .map(|part| OmissionPartRow {
                    part,
                    omission: omission.clone(),
                })
                .collect();
            let shared =
                political_group.mode.is_recovery() && omission.is_actionable() && !parts.is_empty();
            match rows.iter_mut().find(|row| {
                shared
                    && row.omission.has_same_details(&omission)
                    && row
                        .parts
                        .iter()
                        .all(|p| parts.iter().all(|q| q.part != p.part))
            }) {
                Some(row) => row.parts.extend(parts),
                None => rows.push(Self {
                    path: path(&omission),
                    omission,
                    parts,
                }),
            }
        }

        let districts = election.electoral_districts();
        let order = |part: &OmissionPartRow| {
            let district = match part.part {
                OmissionPart::ElectoralDistrict(district) => Some(district),
                OmissionPart::CandidateList(list_id) => political_group
                    .candidate_list_districts(&list_id)
                    .next()
                    .copied(),
            };
            district
                .and_then(|district| districts.iter().position(|d| *d == district))
                .unwrap_or(usize::MAX)
        };
        for row in &mut rows {
            row.parts.sort_by_key(order);
        }
        rows
    }

    /// `omissions` as rows whose titles link nowhere, for the page of the
    /// item they apply to.
    pub fn unlinked(omissions: Vec<Omission>, political_group: &CsbPoliticalGroup) -> Vec<Self> {
        Self::rows(omissions, political_group, |_| String::new())
    }

    /// The rows a candidate's page on `list_id` shows: every omission of the
    /// candidate, so a split one stays in one row, kept to those touching
    /// the list.
    pub fn for_candidate_on_list(
        data: &CsbStoreData,
        political_group: &CsbPoliticalGroup,
        person_id: PersonId,
        list_id: CandidateListId,
    ) -> Vec<Self> {
        let mut rows = Self::unlinked(
            data.get_candidate_omissions(person_id, None, political_group.election),
            political_group,
        );
        rows.retain(|row| row.parts.iter().any(|part| part.list() == Some(&list_id)));
        rows
    }

    /// The declarations-of-support omissions touching any of `districts`,
    /// shown on a list's recovery page; none in the examination phase.
    pub fn declarations_of_support_for_districts(
        data: &CsbStoreData,
        political_group: &CsbPoliticalGroup,
        districts: &BTreeSet<ElectoralDistrict>,
    ) -> Vec<Self> {
        if !political_group.mode.is_recovery() {
            return Vec::new();
        }
        let election = political_group.election;
        Self::unlinked(
            data.get_all_declarations_of_support_omissions(election)
                .into_iter()
                .filter(|omission| {
                    omission
                        .electoral_districts(&election)
                        .iter()
                        .any(|district| districts.contains(district))
                })
                .collect(),
            political_group,
        )
    }
}
