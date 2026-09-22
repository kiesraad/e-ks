//! Typed field identifiers for the audit log.
//!
//! A [`FieldKey`] names one thing the audit log can show: a leaf value
//! (`LastName`) or a group that prefixes the leaves below it
//! (`Representative`). Labels are looked up per key with literal `trans!`
//! calls, so a key without a label does not compile and the locale test keeps
//! both translation files in sync.

use crate::{Locale, trans};

/// Every field the audit log can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldKey {
    // Groups
    Representative,
    Address,
    // Name
    FirstName,
    LastName,
    LastNamePrefix,
    Initials,
    // Personal data
    Gender,
    Bsn,
    DateOfBirth,
    PlaceOfResidence,
    Country,
    // Address
    StreetName,
    HouseNumber,
    HouseNumberAddition,
    Locality,
    PostalCode,
    StateOrProvince,
    KnownInBag,
    // Political group and name authorisation
    Appellation,
    ListDesignation,
    PreviousElectionResults,
    LegalName,
    // Candidate list
    ElectoralDistricts,
    Candidates,
    // System events
    StreamId,
    FileName,
    FileSize,
    DownloadPath,
    ListId,
    CreatedPersons,
    UpdatedPersons,
    // Import summary
    Persons,
    CandidateLists,
    NameAuthorisations,
    SubstituteSubmitters,
    // Omissions (CSB)
    OmissionTitle,
    OmissionDescription,
    OmissionHelpText,
    Recoverable,
    OmissionStatus,
    // Registered political groups (CSB main stream)
    PreviousVotes,
    PreviousSeats,
}

impl FieldKey {
    /// The translated label of this field.
    #[expect(
        clippy::cognitive_complexity,
        clippy::too_many_lines,
        reason = "A flat translation table, one arm per key; the `trans!` expansions inflate the metrics."
    )]
    pub fn label(self, locale: Locale) -> String {
        match self {
            FieldKey::Representative => trans!("audit_log.detail.fields.representative", locale),
            FieldKey::Address => trans!("audit_log.detail.fields.address", locale),
            FieldKey::FirstName => trans!("audit_log.detail.fields.first_name", locale),
            FieldKey::LastName => trans!("audit_log.detail.fields.last_name", locale),
            FieldKey::LastNamePrefix => trans!("audit_log.detail.fields.last_name_prefix", locale),
            FieldKey::Initials => trans!("audit_log.detail.fields.initials", locale),
            FieldKey::Gender => trans!("audit_log.detail.fields.gender", locale),
            FieldKey::Bsn => trans!("audit_log.detail.fields.bsn", locale),
            FieldKey::DateOfBirth => trans!("audit_log.detail.fields.date_of_birth", locale),
            FieldKey::PlaceOfResidence => {
                trans!("audit_log.detail.fields.place_of_residence", locale)
            }
            FieldKey::Country => trans!("audit_log.detail.fields.country", locale),
            FieldKey::StreetName => trans!("audit_log.detail.fields.street_name", locale),
            FieldKey::HouseNumber => trans!("audit_log.detail.fields.house_number", locale),
            FieldKey::HouseNumberAddition => {
                trans!("audit_log.detail.fields.house_number_addition", locale)
            }
            FieldKey::Locality => trans!("audit_log.detail.fields.locality", locale),
            FieldKey::PostalCode => trans!("audit_log.detail.fields.postal_code", locale),
            FieldKey::StateOrProvince => {
                trans!("audit_log.detail.fields.state_or_province", locale)
            }
            FieldKey::KnownInBag => trans!("audit_log.detail.fields.known_in_bag", locale),
            FieldKey::Appellation => trans!("audit_log.detail.fields.appellation", locale),
            FieldKey::ListDesignation => {
                trans!("audit_log.detail.fields.list_designation", locale)
            }
            FieldKey::PreviousElectionResults => {
                trans!("audit_log.detail.fields.previous_election_results", locale)
            }
            FieldKey::LegalName => trans!("audit_log.detail.fields.legal_name", locale),
            FieldKey::ElectoralDistricts => {
                trans!("audit_log.detail.fields.electoral_districts", locale)
            }
            FieldKey::Candidates => trans!("audit_log.detail.fields.candidates", locale),
            FieldKey::StreamId => trans!("audit_log.detail.fields.stream_id", locale),
            FieldKey::FileName => trans!("audit_log.detail.fields.file_name", locale),
            FieldKey::FileSize => trans!("audit_log.detail.fields.file_size", locale),
            FieldKey::DownloadPath => trans!("audit_log.detail.fields.download_path", locale),
            FieldKey::ListId => trans!("audit_log.detail.fields.list_id", locale),
            FieldKey::CreatedPersons => trans!("audit_log.detail.fields.created_persons", locale),
            FieldKey::UpdatedPersons => trans!("audit_log.detail.fields.updated_persons", locale),
            FieldKey::Persons => trans!("audit_log.detail.fields.persons", locale),
            FieldKey::CandidateLists => trans!("audit_log.detail.fields.candidate_lists", locale),
            FieldKey::NameAuthorisations => {
                trans!("audit_log.detail.fields.name_authorisations", locale)
            }
            FieldKey::SubstituteSubmitters => {
                trans!("audit_log.detail.fields.substitute_submitters", locale)
            }
            FieldKey::OmissionTitle => trans!("audit_log.detail.fields.omission_title", locale),
            FieldKey::OmissionDescription => {
                trans!("audit_log.detail.fields.omission_description", locale)
            }
            FieldKey::OmissionHelpText => {
                trans!("audit_log.detail.fields.omission_help_text", locale)
            }
            FieldKey::Recoverable => trans!("audit_log.detail.fields.recoverable", locale),
            FieldKey::OmissionStatus => trans!("audit_log.detail.fields.omission_status", locale),
            FieldKey::PreviousVotes => trans!("audit_log.detail.fields.previous_votes", locale),
            FieldKey::PreviousSeats => trans!("audit_log.detail.fields.previous_seats", locale),
        }
    }
}

/// Where a value sits in its entity, e.g. `[Representative, Address, PostalCode]`.
/// The last key names the field itself; the keys before it are the groups it
/// belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
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

    pub fn keys(&self) -> &[FieldKey] {
        &self.0
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
    }

    #[test]
    fn root_level_field_has_no_group() {
        let path = FieldPath::from(FieldKey::LastName);

        assert_eq!(path.group(), &[] as &[FieldKey]);
        assert_eq!(path.group_label(Locale::En), None);
        assert_eq!(path.leaf_label(Locale::En), "Last name");
    }

    #[test]
    fn root_path_has_no_leaf() {
        assert_eq!(FieldPath::root().leaf(), None);
        assert_eq!(FieldPath::root().leaf_label(Locale::En), "");
    }
}
