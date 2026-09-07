use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::{
    Locale,
    constants::DEFAULT_DATE_FORMAT,
    structs::common::{Bsn, Gender, Initials, LastName, LastNamePrefix, PlaceOfResidence},
    trans,
};

/// The candidate fields that are compared against the BRP.
///
/// The correspondence address is deliberately absent: it is not published on
/// the list, and the BRP models an address differently than this application
/// does. Only the `woonplaats` is verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BrpCheckedField {
    Bsn,
    Initials,
    /// The `voorvoegsel`, held and checked apart from the name itself.
    LastNamePrefix,
    LastName,
    Gender,
    DateOfBirth,
    PlaceOfResidence,
}

impl BrpCheckedField {
    /// The label of the candidate-detail row this field belongs to.
    pub fn label(self, locale: Locale) -> String {
        match self {
            Self::Bsn => trans!("person.fields.bsn", locale),
            Self::Initials => trans!("person.fields.initials", locale),
            Self::LastNamePrefix => trans!("person.fields.last_name_prefix", locale),
            Self::LastName => trans!("person.fields.last_name", locale),
            Self::Gender => trans!("person.fields.gender", locale),
            Self::DateOfBirth => trans!("person.fields.date_of_birth", locale),
            Self::PlaceOfResidence => trans!("person.fields.place_of_residence", locale),
        }
    }
}

/// A value the BRP holds, in the type of the field it belongs to, so a finding
/// cannot pair a field with a value from another one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrpValue {
    Initials(Initials),
    LastNamePrefix(LastNamePrefix),
    LastName(LastName),
    Gender(Gender),
    DateOfBirth(NaiveDate),
    PlaceOfResidence(PlaceOfResidence),
}

impl BrpValue {
    /// The candidate-detail row this value belongs to.
    pub fn field(&self) -> BrpCheckedField {
        match self {
            Self::Initials(_) => BrpCheckedField::Initials,
            Self::LastNamePrefix(_) => BrpCheckedField::LastNamePrefix,
            Self::LastName(_) => BrpCheckedField::LastName,
            Self::Gender(_) => BrpCheckedField::Gender,
            Self::DateOfBirth(_) => BrpCheckedField::DateOfBirth,
            Self::PlaceOfResidence(_) => BrpCheckedField::PlaceOfResidence,
        }
    }

    /// The value as the committee is shown it, in the format the rest of the
    /// interface uses.
    pub fn display(&self, locale: Locale) -> String {
        match self {
            Self::Initials(initials) => initials.to_string(),
            Self::LastNamePrefix(prefix) => prefix.to_string(),
            Self::LastName(last_name) => last_name.to_string(),
            // The same wording the candidate's own gender row shows, rather
            // than the BRP's own code.
            Self::Gender(Gender::Male) => trans!("common.gender.male", locale),
            Self::Gender(Gender::Female) => trans!("common.gender.female", locale),
            Self::DateOfBirth(date) => date.format(DEFAULT_DATE_FORMAT).to_string(),
            Self::PlaceOfResidence(place) => place.to_string(),
        }
    }
}

/// One thing the BRP check found for a single candidate.
///
/// Findings are shown next to the candidate's data and are deliberately not
/// turned into omissions: the committee first confirms a difference and may
/// correct it ambtshalve, and only what remains becomes a verzuim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrpFinding {
    /// The candidate's value differs from the value in the BRP.
    Mismatch { brp_value: BrpValue },
    /// The BRP value could not be interpreted, so nothing was compared. Kept
    /// apart from [`Self::Mismatch`] so a parsing problem is never reported as
    /// a data difference.
    ///
    /// This is the one value that stays a string: it is a value no type of
    /// this application accepts, which is the whole point of the variant.
    Unparsable {
        field: BrpCheckedField,
        brp_value: String,
    },
    /// The BRP holds no value for a field that was requested.
    MissingInBrp { field: BrpCheckedField },
    /// No person in the BRP has this burgerservicenummer.
    BsnUnknown,
    /// More than one person in the BRP matched this burgerservicenummer.
    BsnNotUnique,
    /// The candidate has no burgerservicenummer, so no lookup was possible.
    BsnMissing,
    /// The candidate is recorded as deliberately having no burgerservicenummer.
    BsnNoneConfirmed,
    /// The lookup by burgerservicenummer came up empty, but a search on the
    /// candidate's other personal details found exactly one person. Everything
    /// else is compared against them, which the committee has to be told.
    BsnMatchedByPersonalDetails { bsn: Bsn },
    /// The lookup by burgerservicenummer came up empty and the candidate's
    /// other personal details match more than one person, so there is nobody
    /// to compare against.
    PersonalDetailsNotUnique,
    /// The BRP records a date of death for this candidate. `None` when it
    /// records the death without a full date.
    Deceased { date_of_death: Option<NaiveDate> },
    /// The BRP records no Dutch nationality for this candidate.
    NotDutch,
    /// The BRP records this candidate as excluded from the right to vote.
    ExcludedFromSuffrage,
    /// The candidate lives abroad, so the BRP holds no `woonplaats`.
    ResidenceAbroad,
    /// The candidate's BRP residence is a location rather than an address (a
    /// briefadres, for instance), so the BRP holds no `woonplaats`.
    ResidenceWithoutAddress,
    /// The candidate's residence is unknown in the BRP.
    ResidenceUnknown,
}

impl BrpFinding {
    /// The candidate-detail field this finding belongs to, or `None` when it is
    /// about the candidate as a whole.
    pub fn field(&self) -> Option<BrpCheckedField> {
        match self {
            Self::Mismatch { brp_value } => Some(brp_value.field()),
            Self::Unparsable { field, .. } | Self::MissingInBrp { field } => Some(*field),
            Self::ResidenceAbroad | Self::ResidenceWithoutAddress | Self::ResidenceUnknown => {
                Some(BrpCheckedField::PlaceOfResidence)
            }
            Self::BsnUnknown
            | Self::BsnNotUnique
            | Self::BsnMissing
            | Self::BsnNoneConfirmed
            | Self::BsnMatchedByPersonalDetails { .. }
            | Self::PersonalDetailsNotUnique => Some(BrpCheckedField::Bsn),
            Self::Deceased { .. } => Some(BrpCheckedField::DateOfBirth),
            Self::NotDutch | Self::ExcludedFromSuffrage => None,
        }
    }

    /// The value the BRP holds where this application can type it.
    ///
    /// [`Self::Unparsable`] is absent on purpose: a value no type of this
    /// application accepts is not one to offer while correcting.
    pub fn brp_value(&self) -> Option<&BrpValue> {
        match self {
            Self::Mismatch { brp_value } => Some(brp_value),
            _ => None,
        }
    }

    /// What the committee is shown for a finding about looking the candidate
    /// up at all, rather than about one of their fields.
    fn lookup_message(&self, locale: Locale) -> String {
        match self {
            Self::BsnUnknown => trans!("csb.brp.finding.bsn_unknown", locale),
            Self::BsnNotUnique => trans!("csb.brp.finding.bsn_not_unique", locale),
            Self::BsnMissing => trans!("csb.brp.finding.bsn_missing", locale),
            Self::BsnNoneConfirmed => trans!("csb.brp.finding.bsn_none_confirmed", locale),
            Self::BsnMatchedByPersonalDetails { bsn } => trans!(
                "csb.brp.finding.bsn_matched_by_personal_details",
                locale,
                bsn.expose()
            ),
            Self::PersonalDetailsNotUnique => {
                trans!("csb.brp.finding.personal_details_not_unique", locale)
            }
            _ => unreachable!("only the findings about the lookup itself"),
        }
    }

    /// What the committee is shown for this finding.
    pub fn message(&self, locale: Locale) -> String {
        match self {
            Self::Mismatch { brp_value } => {
                trans!(
                    "csb.brp.finding.mismatch",
                    locale,
                    brp_value.field().label(locale),
                    brp_value.display(locale)
                )
            }
            Self::Unparsable { field, brp_value } => {
                trans!(
                    "csb.brp.finding.unparsable",
                    locale,
                    field.label(locale),
                    brp_value
                )
            }
            Self::MissingInBrp { field } => {
                trans!(
                    "csb.brp.finding.missing_in_brp",
                    locale,
                    field.label(locale)
                )
            }
            Self::BsnUnknown
            | Self::BsnNotUnique
            | Self::BsnMissing
            | Self::BsnNoneConfirmed
            | Self::BsnMatchedByPersonalDetails { .. }
            | Self::PersonalDetailsNotUnique => self.lookup_message(locale),
            Self::Deceased {
                date_of_death: Some(date),
            } => {
                trans!(
                    "csb.brp.finding.deceased",
                    locale,
                    date.format(DEFAULT_DATE_FORMAT).to_string()
                )
            }
            Self::Deceased {
                date_of_death: None,
            } => {
                trans!("csb.brp.finding.deceased_date_unknown", locale)
            }
            Self::NotDutch => trans!("csb.brp.finding.not_dutch", locale),
            Self::ExcludedFromSuffrage => {
                trans!("csb.brp.finding.excluded_from_suffrage", locale)
            }
            Self::ResidenceAbroad => trans!("csb.brp.finding.residence_abroad", locale),
            Self::ResidenceWithoutAddress => {
                trans!("csb.brp.finding.residence_without_address", locale)
            }
            Self::ResidenceUnknown => trans!("csb.brp.finding.residence_unknown", locale),
        }
    }
}
