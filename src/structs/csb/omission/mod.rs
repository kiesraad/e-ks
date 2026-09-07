mod preset;

pub use preset::OmissionPlaceholders;

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{
    ElectoralDistrict,
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
    /// E.g. missing deposit ("waarborgsom"), unidentified submitter,
    /// or problems with authorised agent and/or statutory name (H 3-1 / H 3-2)
    #[default]
    PoliticalGroup,
    /// For omissions specifically having to do with the appellation
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

    /// The electoral districts this omission is scoped to. Only the
    /// "ondersteuningsverklaringen" (H 4) are reported per district; every
    /// other category applies to the political group, a list or a candidate as
    /// a whole, so it has no districts of its own.
    pub fn electoral_districts(&self) -> &[ElectoralDistrict] {
        match &self.category {
            OmissionCategory::DeclarationsOfSupport(districts) => districts,
            OmissionCategory::PoliticalGroup
            | OmissionCategory::Appellation
            | OmissionCategory::CandidateList(_)
            | OmissionCategory::Candidate { .. } => &[],
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
