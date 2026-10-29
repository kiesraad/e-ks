//! Typed field identifiers for the audit log.
//!
//! A [`FieldKey`] names one thing the audit log can show: a leaf value
//! (`LastName`) or a group that prefixes the leaves below it
//! (`Representative`).

use crate::{Locale, trans};

/// Every field the audit log can name, with the key of its label. Labels
/// are literal `trans!` calls, so a key without a translation does not
/// compile and the locale test keeps both translation files in sync.
macro_rules! field_keys {
    ($( $(#[$meta:meta])* $variant:ident => $key:tt ),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum FieldKey {
            $( $(#[$meta])* $variant ),*
        }

        impl FieldKey {
            /// The translated label of this field.
            pub fn label(self, locale: Locale) -> String {
                match self {
                    $( FieldKey::$variant => trans!($key, locale) ),*
                }
            }
        }
    };
}

field_keys! {
    // Groups
    Representative => "audit_log.detail.fields.representative",
    Address => "audit_log.detail.fields.address",
    // Name
    FirstName => "audit_log.detail.fields.first_name",
    LastName => "audit_log.detail.fields.last_name",
    LastNamePrefix => "audit_log.detail.fields.last_name_prefix",
    Initials => "audit_log.detail.fields.initials",
    // Personal data
    Gender => "audit_log.detail.fields.gender",
    Bsn => "audit_log.detail.fields.bsn",
    DateOfBirth => "audit_log.detail.fields.date_of_birth",
    PlaceOfResidence => "audit_log.detail.fields.place_of_residence",
    Country => "audit_log.detail.fields.country",
    // Address
    StreetName => "audit_log.detail.fields.street_name",
    HouseNumber => "audit_log.detail.fields.house_number",
    HouseNumberAddition => "audit_log.detail.fields.house_number_addition",
    Locality => "audit_log.detail.fields.locality",
    PostalCode => "audit_log.detail.fields.postal_code",
    StateOrProvince => "audit_log.detail.fields.state_or_province",
    KnownInBag => "audit_log.detail.fields.known_in_bag",
    // Political group and name authorisation
    Appellation => "audit_log.detail.fields.appellation",
    ListDesignation => "audit_log.detail.fields.list_designation",
    PreviousElectionResults => "audit_log.detail.fields.previous_election_results",
    LegalName => "audit_log.detail.fields.legal_name",
    RegisteredPoliticalGroup => "audit_log.detail.fields.registered_political_group",
    // Candidate list
    ElectoralDistricts => "audit_log.detail.fields.electoral_districts",
    Candidates => "audit_log.detail.fields.candidates",
    // System events
    StreamId => "audit_log.detail.fields.stream_id",
    FileName => "audit_log.detail.fields.file_name",
    FileSize => "audit_log.detail.fields.file_size",
    DownloadPath => "audit_log.detail.fields.download_path",
    ListId => "audit_log.detail.fields.list_id",
    CreatedPersons => "audit_log.detail.fields.created_persons",
    UpdatedPersons => "audit_log.detail.fields.updated_persons",
    // Import summary
    Persons => "audit_log.detail.fields.persons",
    CandidateLists => "audit_log.detail.fields.candidate_lists",
    NameAuthorisations => "audit_log.detail.fields.name_authorisations",
    SubstituteSubmitters => "audit_log.detail.fields.substitute_submitters",
    // Omissions (CSB)
    OmissionTitle => "audit_log.detail.fields.omission_title",
    OmissionDescription => "audit_log.detail.fields.omission_description",
    OmissionHelpText => "audit_log.detail.fields.omission_help_text",
    Recoverable => "audit_log.detail.fields.recoverable",
    OmissionStatus => "audit_log.detail.fields.omission_status",
    // Registered political groups (CSB main stream)
    PreviousVotes => "audit_log.detail.fields.previous_votes",
    PreviousSeats => "audit_log.detail.fields.previous_seats",
    // Examination (CSB)
    ExaminationFinished => "audit_log.detail.fields.examination_finished",
    BrpStatus => "audit_log.detail.fields.brp_status",
    // Hearings, numbering and objections (CSB main stream)
    HearingDateTime => "audit_log.detail.fields.hearing_date_time",
    Chair => "audit_log.detail.fields.chair",
    Members => "audit_log.detail.fields.members",
    ListOrder => "audit_log.detail.fields.list_order",
    ObjectionText => "audit_log.detail.fields.objection_text",
}

/// Where a value sits in its entity, e.g. `[Representative, Address, PostalCode]`.
/// The last key names the field itself; the keys before it are the groups it
/// belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FieldPath(Vec<FieldKey>);

impl FieldPath {
    /// The path of an entity itself: no keys.
    pub const fn root() -> Self {
        Self(Vec::new())
    }

    /// This path extended with `key`.
    pub fn with(&self, key: FieldKey) -> Self {
        let mut keys = self.0.clone();
        keys.push(key);
        Self(keys)
    }

    /// The field this path names; `None` for the root.
    pub fn leaf(&self) -> Option<FieldKey> {
        self.0.last().copied()
    }

    /// The groups the field belongs to: every key but the last.
    pub fn group(&self) -> &[FieldKey] {
        self.0.split_last().map_or(&[], |(_, group)| group)
    }

    /// Translated label of the field itself, without its groups.
    pub fn leaf_label(&self, locale: Locale) -> String {
        self.leaf().map(|key| key.label(locale)).unwrap_or_default()
    }

    /// Translated label of the groups, e.g. "Gemachtigde › Adres". `None` for
    /// a field at the root of its entity.
    pub fn group_label(&self, locale: Locale) -> Option<String> {
        let group = self.group();
        if group.is_empty() {
            return None;
        }
        Some(
            group
                .iter()
                .map(|key| key.label(locale))
                .collect::<Vec<_>>()
                .join(" › "),
        )
    }
}

impl From<FieldKey> for FieldPath {
    fn from(key: FieldKey) -> Self {
        Self(vec![key])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_splits_into_group_and_leaf() {
        let path = FieldPath::root()
            .with(FieldKey::Representative)
            .with(FieldKey::Address)
            .with(FieldKey::PostalCode);

        assert_eq!(path.leaf(), Some(FieldKey::PostalCode));
        assert_eq!(path.group(), &[FieldKey::Representative, FieldKey::Address]);
        assert_eq!(path.leaf_label(Locale::Nl), "Postcode");
        // The long word carries a soft hyphen (U+00AD) in the translation.
        assert_eq!(
            path.group_label(Locale::Nl).as_deref(),
            Some("Gemachtigde › Correspondentie\u{ad}adres")
        );

        let root_level = FieldPath::from(FieldKey::LastName);
        assert_eq!(root_level.group(), &[] as &[FieldKey]);
        assert_eq!(root_level.group_label(Locale::En), None);
        assert_eq!(root_level.leaf_label(Locale::En), "Last name");

        assert_eq!(FieldPath::root().leaf(), None);
        assert_eq!(FieldPath::root().leaf_label(Locale::En), "");
    }
}
