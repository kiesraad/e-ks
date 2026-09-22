//! The typed values that appear in an audit-log diff.
//!
//! Values carry their kind (a date, a closed choice, a reference to another
//! entity) instead of a preformatted string, so the render step can format
//! and translate them and the diff can treat collections as collections.

use std::collections::BTreeSet;

use chrono::NaiveDate;

use crate::{
    ElectoralDistrict, Locale, StreamId,
    structs::{
        candidate_lists::CandidateListId,
        common::{
            BsnOrNoneConfirmed, DateOfBirth, Gender, PlaceOfResidence, PreviousElectionResults,
        },
        csb::OmissionStatus,
        list_designation::ListDesignation,
        persons::PersonId,
    },
    trans,
};

#[derive(Debug, Clone, PartialEq)]
pub enum AuditValue {
    /// Locale-independent and already human readable: names, postal codes,
    /// file names, counts.
    Text(String),
    Bool(bool),
    Date(NaiveDate),
    /// A closed choice, translated at render time.
    Enum(EnumValue),
    /// A reference to another stored entity, with the description it had at
    /// the time of the event.
    Entity(EntityRef),
    /// An order-insensitive collection (electoral districts).
    Set(Vec<AuditValue>),
    /// An order-sensitive collection (candidates on a list).
    Ordered(Vec<AuditValue>),
    /// `None`, or the entity does not exist on this side of the event.
    Missing,
}

impl AuditValue {
    pub fn text(value: impl ToString) -> Self {
        AuditValue::Text(value.to_string())
    }

    pub(super) fn is_empty_collection(&self) -> bool {
        matches!(self, AuditValue::Set(items) | AuditValue::Ordered(items) if items.is_empty())
    }

    /// Whether two collection items are the same item. Entity references are
    /// the same entity when their ids match, whatever they were called on
    /// either side of the event.
    pub(super) fn same_item(&self, other: &Self) -> bool {
        match (self, other) {
            (AuditValue::Entity(a), AuditValue::Entity(b)) => a.id == b.id,
            (a, b) => a == b,
        }
    }
}

/// The translatable enum values. Closed on purpose: `trans!` needs literal
/// keys, so the label of every variant is spelled out here.
#[derive(Debug, Clone, PartialEq)]
pub enum EnumValue {
    Gender(Gender),
    ListDesignation(ListDesignation),
    PreviousElectionResults(PreviousElectionResults),
    BsnNoneConfirmed,
    OmissionStatus(OmissionStatus),
}

impl EnumValue {
    pub fn label(&self, locale: Locale) -> String {
        match self {
            EnumValue::Gender(Gender::Female) => trans!("common.gender.female", locale),
            EnumValue::Gender(Gender::Male) => trans!("common.gender.male", locale),
            EnumValue::ListDesignation(ListDesignation::Standalone) => {
                trans!("political_group.type.registered_name", locale)
            }
            EnumValue::ListDesignation(ListDesignation::Blank) => {
                trans!("political_group.type.blank_name", locale)
            }
            EnumValue::ListDesignation(ListDesignation::Combined) => {
                trans!("political_group.type.name_combination", locale)
            }
            EnumValue::PreviousElectionResults(PreviousElectionResults::ZeroSeats) => {
                trans!("political_group.type.zero_seats", locale)
            }
            EnumValue::PreviousElectionResults(PreviousElectionResults::OneToFifteenSeats) => {
                trans!("political_group.type.one_to_fifteen_seats", locale)
            }
            EnumValue::PreviousElectionResults(PreviousElectionResults::SixteenOrMoreSeats) => {
                trans!("political_group.type.sixteen_or_more_seats", locale)
            }
            EnumValue::BsnNoneConfirmed => {
                trans!("audit_log.detail.values.bsn_none_confirmed", locale)
            }
            EnumValue::OmissionStatus(OmissionStatus::Pending) => {
                trans!("audit_log.detail.values.omission_pending", locale)
            }
            EnumValue::OmissionStatus(OmissionStatus::Recovered) => {
                trans!("csb.recovery.recovered", locale)
            }
            EnumValue::OmissionStatus(OmissionStatus::NotRecovered) => {
                trans!("csb.recovery.not_recovered", locale)
            }
        }
    }
}

/// A reference to another entity, resolved when the change is computed.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityRef {
    pub id: EntityId,
    /// How the entity was known at the time: a person's name, a list's
    /// districts. Empty when the entity could not be found.
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityId {
    Person(PersonId),
    CandidateList(CandidateListId),
}

impl std::fmt::Display for EntityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EntityId::Person(id) => id.fmt(f),
            EntityId::CandidateList(id) => id.fmt(f),
        }
    }
}

/// A value that occupies one row of the audit log.
///
/// Implemented per leaf type, not via `Display`: for several types `Display`
/// is the serde form (`female`, `zero_seats`, an ISO date), not what a user
/// should read.
pub trait AuditLeaf {
    fn audit_value(&self) -> AuditValue;
}

impl<T: AuditLeaf> AuditLeaf for Option<T> {
    fn audit_value(&self) -> AuditValue {
        self.as_ref()
            .map_or(AuditValue::Missing, AuditLeaf::audit_value)
    }
}

impl<T: AuditLeaf> AuditLeaf for Vec<T> {
    fn audit_value(&self) -> AuditValue {
        AuditValue::Ordered(self.iter().map(AuditLeaf::audit_value).collect())
    }
}

impl<T: AuditLeaf> AuditLeaf for BTreeSet<T> {
    fn audit_value(&self) -> AuditValue {
        AuditValue::Set(self.iter().map(AuditLeaf::audit_value).collect())
    }
}

impl AuditLeaf for bool {
    fn audit_value(&self) -> AuditValue {
        AuditValue::Bool(*self)
    }
}

/// An empty string is no value: the domain uses it where a name or address
/// part has not been filled in yet.
impl AuditLeaf for String {
    fn audit_value(&self) -> AuditValue {
        if self.is_empty() {
            AuditValue::Missing
        } else {
            AuditValue::Text(self.clone())
        }
    }
}

impl AuditLeaf for usize {
    fn audit_value(&self) -> AuditValue {
        AuditValue::text(self)
    }
}

impl AuditLeaf for DateOfBirth {
    fn audit_value(&self) -> AuditValue {
        AuditValue::Date(NaiveDate::from(self.clone()))
    }
}

impl AuditLeaf for Gender {
    fn audit_value(&self) -> AuditValue {
        AuditValue::Enum(EnumValue::Gender(*self))
    }
}

impl AuditLeaf for ListDesignation {
    fn audit_value(&self) -> AuditValue {
        AuditValue::Enum(EnumValue::ListDesignation(*self))
    }
}

impl AuditLeaf for PreviousElectionResults {
    fn audit_value(&self) -> AuditValue {
        AuditValue::Enum(EnumValue::PreviousElectionResults(*self))
    }
}

impl AuditLeaf for OmissionStatus {
    fn audit_value(&self) -> AuditValue {
        AuditValue::Enum(EnumValue::OmissionStatus(*self))
    }
}

impl AuditLeaf for BsnOrNoneConfirmed {
    fn audit_value(&self) -> AuditValue {
        match self {
            BsnOrNoneConfirmed::NoneConfirmed => AuditValue::Enum(EnumValue::BsnNoneConfirmed),
            // The audit log shows the number itself, as the projection stores it.
            BsnOrNoneConfirmed::Bsn(bsn) => AuditValue::Text(bsn.to_exposed_string()),
        }
    }
}

/// The name only: whether the BAG knows the place is not a field of its own.
impl AuditLeaf for PlaceOfResidence {
    fn audit_value(&self) -> AuditValue {
        AuditValue::text(self)
    }
}

impl AuditLeaf for ElectoralDistrict {
    fn audit_value(&self) -> AuditValue {
        AuditValue::text(self.title())
    }
}

impl AuditLeaf for StreamId {
    fn audit_value(&self) -> AuditValue {
        AuditValue::text(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structs::common::Bsn;

    #[test]
    fn option_none_is_missing_and_some_delegates() {
        assert_eq!(None::<bool>.audit_value(), AuditValue::Missing);
        assert_eq!(Some(true).audit_value(), AuditValue::Bool(true));
    }

    #[test]
    fn collections_map_to_their_kind() {
        assert_eq!(
            vec![1usize, 2].audit_value(),
            AuditValue::Ordered(vec![AuditValue::text(1), AuditValue::text(2)])
        );
        assert_eq!(
            BTreeSet::from([ElectoralDistrict::Utrecht]).audit_value(),
            AuditValue::Set(vec![AuditValue::text("Utrecht")])
        );
    }

    #[test]
    fn place_of_residence_drops_the_bag_variant() {
        assert_eq!(
            PlaceOfResidence::Unknown("Juinen".to_string()).audit_value(),
            AuditValue::text("Juinen")
        );
    }

    #[test]
    fn bsn_is_shown_as_its_digits_or_as_the_confirmation() {
        let bsn: Bsn = "999995972".parse().expect("bsn");
        assert_eq!(
            BsnOrNoneConfirmed::Bsn(bsn).audit_value(),
            AuditValue::text("999995972")
        );
        assert_eq!(
            BsnOrNoneConfirmed::NoneConfirmed.audit_value(),
            AuditValue::Enum(EnumValue::BsnNoneConfirmed)
        );
    }

    #[test]
    fn district_uses_its_title_with_diacritics() {
        assert_eq!(
            ElectoralDistrict::Fryslan.audit_value(),
            AuditValue::text("Fryslân")
        );
    }

    #[test]
    fn same_item_compares_entities_by_id() {
        let id = EntityId::Person(PersonId::new());
        let a = AuditValue::Entity(EntityRef {
            id,
            description: "before".to_string(),
        });
        let b = AuditValue::Entity(EntityRef {
            id,
            description: "after".to_string(),
        });
        assert!(a.same_item(&b));
        assert_ne!(a, b);
    }

    #[test]
    fn empty_strings_are_missing_values() {
        assert_eq!(String::new().audit_value(), AuditValue::Missing);
        assert_eq!(
            "Bos"
                .parse::<crate::structs::common::LastName>()
                .unwrap()
                .audit_value(),
            AuditValue::text("Bos")
        );
        assert_eq!(
            crate::structs::common::LastName::default().audit_value(),
            AuditValue::Missing
        );
    }

    #[test]
    fn enum_labels_translate() {
        assert_eq!(
            EnumValue::Gender(Gender::Female).label(Locale::En),
            "Female"
        );
        assert_eq!(
            EnumValue::PreviousElectionResults(PreviousElectionResults::SixteenOrMoreSeats)
                .label(Locale::En),
            "16 or more seats"
        );
        assert_eq!(
            EnumValue::OmissionStatus(OmissionStatus::Recovered).label(Locale::Nl),
            "Hersteld"
        );
    }
}
