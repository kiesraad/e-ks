mod preset;

pub use preset::OmissionPlaceholders;

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{
    ElectionConfig, ElectoralDistrict,
    form::ValidationError,
    id_newtype,
    structs::{
        candidate_lists::CandidateListId,
        common::{UtcDateTime, constrained_strings},
        persons::PersonId,
    },
};

// constants to use for `as_str` and `from_str` implementations of `OmissionType`
const POLITICAL_GROUP: &str = "political-group";
const CANDIDATE_LIST: &str = "candidate-list";
const DECLARATION_OF_SUPPORT: &str = "declarations-of-support";
const CANDIDATE: &str = "candidate";
const APPELLATION: &str = "appellation";

id_newtype!(pub struct OmissionId);

constrained_strings! {
    /// Short omission title shown in the pill/badge layout.
    pub struct OmissionTitle(max = 100, multiline = false);
    /// Free omission text: the model I 1 description or the omission letter
    /// help text.
    pub struct OmissionText(max = 2000, multiline = true);
}

/// The kind of item an omission is added to, carried as a path parameter so a
/// single "add omission" dialog can serve political groups, candidate lists and
/// candidates. Maps to a concrete [`OmissionCategory`] together with a
/// referenced item id (see [`OmissionCategory::from_type_and_reference`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OmissionType {
    PoliticalGroup,
    Appellation,
    CandidateList,
    DeclarationsOfSupport,
    Candidate,
}

impl OmissionType {
    fn as_str(self) -> &'static str {
        match self {
            OmissionType::PoliticalGroup => POLITICAL_GROUP,
            OmissionType::CandidateList => CANDIDATE_LIST,
            OmissionType::DeclarationsOfSupport => DECLARATION_OF_SUPPORT,
            OmissionType::Candidate => CANDIDATE,
            OmissionType::Appellation => APPELLATION,
        }
    }

    pub fn needs_districts(self) -> bool {
        matches!(self, OmissionType::DeclarationsOfSupport)
    }

    pub fn needs_candidate_lists(self) -> bool {
        matches!(self, OmissionType::CandidateList | OmissionType::Candidate)
    }
}

impl FromStr for OmissionType {
    type Err = ValidationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            POLITICAL_GROUP => Ok(OmissionType::PoliticalGroup),
            APPELLATION => Ok(OmissionType::Appellation),
            CANDIDATE_LIST => Ok(OmissionType::CandidateList),
            DECLARATION_OF_SUPPORT => Ok(OmissionType::DeclarationsOfSupport),
            CANDIDATE => Ok(OmissionType::Candidate),
            _ => Err(ValidationError::InvalidValue),
        }
    }
}

impl std::fmt::Display for OmissionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// Deserialize from a plain string so it works with the axum path deserializer
// (which drives every field through `deserialize_str`), mirroring `id_newtype`.
impl<'de> Deserialize<'de> for OmissionType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Default, Debug, Serialize, Eq, PartialEq, Deserialize, Clone)]
pub enum OmissionCategory {
    /// Anything else about the group as a whole, e.g. a missing deposit
    /// ("waarborgsom") or an unidentified submitter. Has no presets; the CSB
    /// describes these itself.
    #[default]
    PoliticalGroup,
    /// The appellation, e.g. not registered (H 3-1 / H 3-2). Unresolved, it
    /// scraps the appellation: the list continues as a blank list.
    Appellation,
    /// Omissions scoped to one or more specific candidate lists.
    CandidateList(Vec<CandidateListId>),
    /// Missing or incorrect "ondersteuningsverklaringen" (H 4), per district.
    DeclarationsOfSupport(Vec<ElectoralDistrict>),
    /// E.g. missing or invalid candidate data, missing or invalid "instemmingsverklaring" (H 9),
    /// missing copy of identity document
    Candidate {
        person: PersonId,
        /// The candidate lists to which this applies.
        lists: Vec<CandidateListId>,
    },
}

impl OmissionCategory {
    /// The districts this category is scoped to. Only the
    /// "ondersteuningsverklaringen" (H 4) are; no districts means all of them.
    pub fn electoral_districts(&self, election: &ElectionConfig) -> &[ElectoralDistrict] {
        match self {
            OmissionCategory::DeclarationsOfSupport(districts) if districts.is_empty() => {
                election.electoral_districts()
            }
            OmissionCategory::DeclarationsOfSupport(districts) => districts,
            OmissionCategory::PoliticalGroup
            | OmissionCategory::Appellation
            | OmissionCategory::CandidateList(_)
            | OmissionCategory::Candidate { .. } => &[],
        }
    }

    /// The candidate lists this category is scoped to. Both a candidate's own
    /// omissions and a list's are reported per list.
    pub fn candidate_lists(&self) -> &[CandidateListId] {
        match self {
            OmissionCategory::CandidateList(lists) | OmissionCategory::Candidate { lists, .. } => {
                lists
            }
            OmissionCategory::PoliticalGroup
            | OmissionCategory::Appellation
            | OmissionCategory::DeclarationsOfSupport(_) => &[],
        }
    }

    /// Whether both categories are scoped to the same kind of part: districts,
    /// lists, or the lists of the same candidate. A political group omission
    /// has no parts.
    fn has_same_scope(&self, other: &Self) -> bool {
        match (self, other) {
            (
                OmissionCategory::DeclarationsOfSupport(_),
                OmissionCategory::DeclarationsOfSupport(_),
            )
            | (OmissionCategory::CandidateList(_), OmissionCategory::CandidateList(_)) => true,
            (
                OmissionCategory::Candidate { person, .. },
                OmissionCategory::Candidate { person: other, .. },
            ) => person == other,
            _ => false,
        }
    }

    /// This category and `other` as one, covering the parts of both. `None`
    /// unless both are scoped to the same kind of part. Districts are read in
    /// region-number order, and no districts (all of them) absorb any others;
    /// lists keep this category's order, with the other ones appended.
    pub fn merged_with(&self, other: &Self) -> Option<Self> {
        if !self.has_same_scope(other) {
            return None;
        }

        match (self, other) {
            (
                OmissionCategory::DeclarationsOfSupport(mine),
                OmissionCategory::DeclarationsOfSupport(theirs),
            ) => {
                if mine.is_empty() || theirs.is_empty() {
                    return Some(OmissionCategory::DeclarationsOfSupport(Vec::new()));
                }
                let mut districts = mine.clone();
                districts.extend(theirs.iter().filter(|d| !mine.contains(d)));
                districts.sort_by_key(ElectoralDistrict::region_number);
                Some(OmissionCategory::DeclarationsOfSupport(districts))
            }
            _ => {
                let mut lists = self.candidate_lists().to_vec();
                let added: Vec<CandidateListId> = other
                    .candidate_lists()
                    .iter()
                    .filter(|list| !lists.contains(list))
                    .copied()
                    .collect();
                lists.extend(added);
                self.with_candidate_lists(lists)
            }
        }
    }

    /// Where a decision on `part` lands: on the omission as a whole when the
    /// part is all it covers, or on a split otherwise. `None` when the
    /// category does not cover `part`. The districts have to be spelled out
    /// (see [`Omission::with_explicit_districts`]).
    pub fn decide(&self, part: OmissionPart) -> Option<OmissionDecision> {
        match part {
            OmissionPart::ElectoralDistrict(district) => {
                let OmissionCategory::DeclarationsOfSupport(districts) = self else {
                    return None;
                };
                if !districts.contains(&district) {
                    return None;
                }
                if districts.len() == 1 {
                    return Some(OmissionDecision::Whole);
                }

                Some(OmissionDecision::Split {
                    remaining: OmissionCategory::DeclarationsOfSupport(
                        districts
                            .iter()
                            .copied()
                            .filter(|d| *d != district)
                            .collect(),
                    ),
                    split: OmissionCategory::DeclarationsOfSupport(vec![district]),
                })
            }
            OmissionPart::CandidateList(list_id) => {
                let lists = self.candidate_lists();
                if !lists.contains(&list_id) {
                    return None;
                }
                if lists.len() == 1 {
                    return Some(OmissionDecision::Whole);
                }

                Some(OmissionDecision::Split {
                    remaining: self.with_candidate_lists(
                        lists.iter().copied().filter(|l| *l != list_id).collect(),
                    )?,
                    split: self.with_candidate_lists(vec![list_id])?,
                })
            }
        }
    }

    /// The same category scoped to `lists`. `None` when it is not scoped to
    /// candidate lists at all.
    fn with_candidate_lists(&self, lists: Vec<CandidateListId>) -> Option<Self> {
        match self {
            OmissionCategory::CandidateList(_) => Some(OmissionCategory::CandidateList(lists)),
            OmissionCategory::Candidate { person, .. } => Some(OmissionCategory::Candidate {
                person: *person,
                lists,
            }),
            OmissionCategory::PoliticalGroup
            | OmissionCategory::Appellation
            | OmissionCategory::DeclarationsOfSupport(_) => None,
        }
    }

    /// Build the category for a newly added omission from the parameters of the
    /// "add omission" dialog. For `DeclarationsOfSupport`, construct the category
    /// directly with the selected districts (see `add_omission_submit`).
    pub fn from_type_and_reference(
        omission_type: OmissionType,
        reference: uuid::Uuid,
        lists: Vec<CandidateListId>,
    ) -> Self {
        match omission_type {
            OmissionType::PoliticalGroup => OmissionCategory::PoliticalGroup,
            OmissionType::Appellation => OmissionCategory::Appellation,
            OmissionType::CandidateList => OmissionCategory::CandidateList(lists),
            OmissionType::DeclarationsOfSupport => {
                unreachable!(
                    "DeclarationsOfSupport omissions must be created with explicit districts"
                )
            }
            OmissionType::Candidate => OmissionCategory::Candidate {
                person: reference.into(),
                lists,
            },
        }
    }
}

#[derive(Default, Debug, Clone, Copy, Serialize, Eq, PartialEq, Deserialize)]
pub enum OmissionStatus {
    /// Not yet assessed in the "Herstelde lijsten" phase.
    #[default]
    Pending,
    /// The omission was recovered ("hersteld").
    Recovered,
    /// The omission was not recovered and is now permanent.
    NotRecovered,
}

impl OmissionStatus {
    pub fn is_recovered(&self) -> bool {
        matches!(self, OmissionStatus::Recovered)
    }

    pub fn is_not_recovered(&self) -> bool {
        matches!(self, OmissionStatus::NotRecovered)
    }
}

/// The part of an omission one recovery decision applies to: an electoral
/// district for the declarations of support, a candidate list for a
/// candidate's or a list's own omissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OmissionPart {
    ElectoralDistrict(ElectoralDistrict),
    CandidateList(CandidateListId),
}

/// Where a decision on one part of an omission lands.
#[derive(Debug, PartialEq, Eq)]
pub enum OmissionDecision {
    /// The part is all the omission covers, so the decision applies to it as
    /// a whole.
    Whole,
    /// The omission is split, so the other parts keep their own decision.
    Split {
        remaining: OmissionCategory,
        split: OmissionCategory,
    },
}

/// Progress through the recovery ("Herstelde lijsten") phase of a political
/// group, counted in decisions rather than in omissions: an omission the CSB
/// assesses part by part stands for one decision per part (see
/// [`Omission::decision_count`]). Irreparable omissions were never in the
/// omission letter, so there is nothing to assess and they are in neither
/// count.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryProgress {
    /// The decisions that still need a recovered / not-recovered answer, a
    /// subset of `total`.
    pub pending: usize,
    /// The decisions to be made at all.
    pub total: usize,
}

impl RecoveryProgress {
    /// The decisions already answered.
    pub fn decided(&self) -> usize {
        self.total - self.pending
    }

    /// Whether every decision has been made. True as well when the group has
    /// no omission to assess.
    pub fn is_complete(&self) -> bool {
        self.pending == 0
    }
}

/// An omission ("verzuim") signifies something was wrong with the submitted data
#[derive(Default, Debug, Serialize, Eq, PartialEq, Deserialize, Clone)]
pub struct Omission {
    pub id: OmissionId,
    pub category: OmissionCategory,
    /// Short title shown in the pill/badge layout
    pub title: OmissionTitle,
    /// The description for on the model I 1
    pub description: OmissionText,
    /// Help text for political groups explaining how to resolve the omission
    /// ("Dit verzuim is te herstellen door ..."); irreparable omissions have
    /// none. Events persisted before this was optional store an empty string,
    /// so display code should go through [`Self::help_text`].
    #[serde(default)]
    pub(crate) help_text: Option<OmissionText>,
    #[serde(default = "recoverable_by_default")]
    pub recoverable: bool,
    #[serde(default)]
    pub status: OmissionStatus,
    pub updated_at: UtcDateTime,
}

fn recoverable_by_default() -> bool {
    true
}

impl Omission {
    pub fn new(
        category: OmissionCategory,
        title: OmissionTitle,
        description: OmissionText,
        help_text: Option<OmissionText>,
    ) -> Self {
        Omission {
            category,
            title,
            description,
            help_text,
            recoverable: true,
            ..Default::default()
        }
    }

    /// The help text, if any (legacy events persisted "no help text" as an
    /// empty string rather than as an absent value).
    pub fn help_text(&self) -> Option<&OmissionText> {
        self.help_text.as_ref().filter(|text| !text.is_empty())
    }

    pub fn class(&self) -> &str {
        if self.recoverable { "warning" } else { "error" }
    }

    /// The districts this omission is scoped to (see
    /// [`OmissionCategory::electoral_districts`]).
    pub fn electoral_districts(&self, election: &ElectionConfig) -> &[ElectoralDistrict] {
        self.category.electoral_districts(election)
    }

    /// The candidate lists this omission is scoped to (see
    /// [`OmissionCategory::candidate_lists`]).
    pub fn candidate_lists(&self) -> &[CandidateListId] {
        self.category.candidate_lists()
    }

    /// Whether `other` reads the same as this omission: the same texts and
    /// severity, scoped to the same kind of part. Parts of both decided the
    /// same way then belong in one omission.
    pub fn has_same_details(&self, other: &Omission) -> bool {
        self.title == other.title
            && self.description == other.description
            && self.help_text() == other.help_text()
            && self.recoverable == other.recoverable
            && self.category.has_same_scope(&other.category)
    }

    /// Whether the CSB decides on this omission part by part, so it can be
    /// recovered in some parts and not in others.
    pub fn is_assessed_per_part(&self, election: &ElectionConfig) -> bool {
        self.is_actionable() && self.decision_count(election) > 1
    }

    /// The decisions this omission stands for: one per part when assessed part
    /// by part, one otherwise. Keeps the progress stable across a split.
    pub fn decision_count(&self, election: &ElectionConfig) -> usize {
        if !self.is_actionable() {
            return 1;
        }

        // A category is scoped to districts or to lists, never to both.
        self.electoral_districts(election)
            .len()
            .max(self.candidate_lists().len())
            .max(1)
    }

    /// Whether a decision on `part` applies to this omission.
    pub fn covers(&self, election: &ElectionConfig, part: OmissionPart) -> bool {
        match part {
            OmissionPart::ElectoralDistrict(district) => {
                self.electoral_districts(election).contains(&district)
            }
            OmissionPart::CandidateList(list_id) => self.candidate_lists().contains(&list_id),
        }
    }

    /// This omission with the districts it covers spelled out, or `None` when
    /// they already are. Events persisted before districts were required may
    /// cover all of them as none, which a decision per district cannot be
    /// applied to.
    pub fn with_explicit_districts(&self, election: &ElectionConfig) -> Option<Self> {
        match &self.category {
            OmissionCategory::DeclarationsOfSupport(districts) if districts.is_empty() => {
                Some(Omission {
                    category: OmissionCategory::DeclarationsOfSupport(
                        election.electoral_districts().to_vec(),
                    ),
                    ..self.clone()
                })
            }
            _ => None,
        }
    }

    /// Whether the CSB can mark this omission as recovered or not recovered in
    /// the "Herstelde lijsten" phase. Irreparable omissions were never in the
    /// omission letter, so there is nothing to assess.
    pub fn is_actionable(&self) -> bool {
        self.recoverable
    }

    /// Whether this omission still needs a recovered / not-recovered decision.
    pub fn is_pending(&self) -> bool {
        self.recoverable && self.status == OmissionStatus::Pending
    }

    /// Whether this omission remains after the recovery window: irreparable, or
    /// explicitly marked as not recovered. Unresolved omissions scrap the
    /// candidate, list or district they apply to.
    pub fn is_unresolved(&self) -> bool {
        !self.recoverable || self.status == OmissionStatus::NotRecovered
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{AppError, CsbStore};

    pub fn sample_omission(category: OmissionCategory) -> Omission {
        Omission::new(
            category,
            "test title".parse().unwrap(),
            "test description".parse().unwrap(),
            Some("test help text".parse().unwrap()),
        )
    }

    #[test]
    fn omission_recoverable_defaults_to_true_for_legacy_events() {
        // Events persisted before the flag existed omit `recoverable`; they must
        // deserialize as recoverable rather than as errors.
        let json = r#"{
            "id": "00000000-0000-0000-0000-000000000000",
            "category": "PoliticalGroup",
            "title": "t",
            "description": "d",
            "help_text": "",
            "updated_at": "2026-01-01T00:00:00Z"
        }"#;
        let omission: Omission = serde_json::from_str(json).unwrap();
        assert!(omission.recoverable);
        // Events persisted before the status existed start out pending.
        assert_eq!(omission.status, OmissionStatus::Pending);
        // Legacy events persisted "no help text" as an empty string; the
        // accessor hides it.
        assert_eq!(omission.help_text(), None);
    }

    #[test]
    fn merged_categories_cover_the_parts_of_both() {
        // Districts follow the region-number order, whichever side they come from.
        let utrecht = OmissionCategory::DeclarationsOfSupport(vec![ElectoralDistrict::Utrecht]);
        let groningen = OmissionCategory::DeclarationsOfSupport(vec![ElectoralDistrict::Groningen]);
        assert_eq!(
            utrecht.merged_with(&groningen),
            Some(OmissionCategory::DeclarationsOfSupport(vec![
                ElectoralDistrict::Groningen,
                ElectoralDistrict::Utrecht
            ]))
        );

        // A legacy omission without districts covers all of them, so it
        // absorbs any other.
        let all = OmissionCategory::DeclarationsOfSupport(vec![]);
        assert_eq!(utrecht.merged_with(&all), Some(all.clone()));

        // Lists keep this side's order and are not repeated.
        let (a, b, c) = (
            CandidateListId::new(),
            CandidateListId::new(),
            CandidateListId::new(),
        );
        assert_eq!(
            OmissionCategory::CandidateList(vec![b, a])
                .merged_with(&OmissionCategory::CandidateList(vec![c, a])),
            Some(OmissionCategory::CandidateList(vec![b, a, c]))
        );

        // Only the same kind of part merges, and only for the same candidate.
        let person = PersonId::new();
        let candidate = |lists| OmissionCategory::Candidate { person, lists };
        assert_eq!(
            candidate(vec![a]).merged_with(&candidate(vec![b])),
            Some(candidate(vec![a, b]))
        );
        assert_eq!(
            candidate(vec![a]).merged_with(&OmissionCategory::Candidate {
                person: PersonId::new(),
                lists: vec![b],
            }),
            None
        );
        assert_eq!(
            candidate(vec![a]).merged_with(&OmissionCategory::CandidateList(vec![b])),
            None
        );
        assert_eq!(
            OmissionCategory::PoliticalGroup.merged_with(&OmissionCategory::PoliticalGroup),
            None
        );
    }

    #[test]
    fn omissions_with_the_same_texts_and_scope_have_the_same_details() {
        let omission = sample_omission(OmissionCategory::DeclarationsOfSupport(vec![
            ElectoralDistrict::Utrecht,
        ]));
        let other_district = sample_omission(OmissionCategory::DeclarationsOfSupport(vec![
            ElectoralDistrict::Groningen,
        ]));
        assert!(omission.has_same_details(&other_district));

        let mut other_title = other_district.clone();
        other_title.title = "other title".parse().unwrap();
        assert!(!omission.has_same_details(&other_title));

        let mut irreparable = other_district.clone();
        irreparable.recoverable = false;
        assert!(!omission.has_same_details(&irreparable));

        // Legacy events stored a missing help text as an empty string.
        let mut legacy = other_district.clone();
        legacy.help_text = serde_json::from_str(r#""""#).unwrap();
        let mut without_help_text = omission.clone();
        without_help_text.help_text = None;
        assert!(without_help_text.has_same_details(&legacy));

        assert!(!omission.has_same_details(&sample_omission(OmissionCategory::PoliticalGroup)));
    }

    #[tokio::test]
    async fn create_and_get_omission() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();
        let omission = sample_omission(OmissionCategory::PoliticalGroup);

        omission.create(&store).await?;

        let loaded = store.get_omission(omission.id)?;
        assert_eq!(loaded.id, omission.id);
        assert_eq!(loaded.description.to_string(), "test description");

        Ok(())
    }

    #[tokio::test]
    async fn update_omission_overwrites_fields() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();
        let mut omission = sample_omission(OmissionCategory::PoliticalGroup);

        omission.create(&store).await?;

        omission.description = "Updated description".parse().unwrap();
        omission.update(&store).await?;

        let updated = store.get_omission(omission.id)?;
        assert_eq!(updated.description.to_string(), "Updated description");

        Ok(())
    }

    #[tokio::test]
    async fn set_status_records_the_decision() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();
        let omission = sample_omission(OmissionCategory::PoliticalGroup);

        omission.create(&store).await?;
        omission
            .set_status(&store, OmissionStatus::Recovered)
            .await?;

        let updated = store.get_omission(omission.id)?;
        assert_eq!(updated.status, OmissionStatus::Recovered);
        assert!(!updated.is_pending());
        assert!(!updated.is_unresolved());

        omission
            .set_status(&store, OmissionStatus::NotRecovered)
            .await?;
        assert!(store.get_omission(omission.id)?.is_unresolved());

        Ok(())
    }

    #[tokio::test]
    async fn set_status_rejects_irreparable_omissions() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();
        let mut omission = sample_omission(OmissionCategory::PoliticalGroup);
        omission.recoverable = false;

        omission.create(&store).await?;

        assert!(!omission.is_actionable());
        // Irreparable omissions count as unresolved without a decision.
        assert!(omission.is_unresolved());
        assert!(!omission.is_pending());
        assert!(
            omission
                .set_status(&store, OmissionStatus::Recovered)
                .await
                .is_err()
        );
        assert_eq!(
            store.get_omission(omission.id)?.status,
            OmissionStatus::Pending
        );

        Ok(())
    }

    #[tokio::test]
    async fn delete_omission_removes_record() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();
        let omission = sample_omission(OmissionCategory::PoliticalGroup);

        omission.create(&store).await?;
        omission.delete(&store).await?;

        let missing = store.get_omission(omission.id);
        assert!(missing.is_err());

        Ok(())
    }
}
