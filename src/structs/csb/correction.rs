use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::{
    Locale,
    structs::{
        audit_log::FieldChange,
        common::{Appellation, DateOfBirth, Initials, LastName, LastNamePrefix, PlaceOfResidence},
        persons::{Person, PersonId},
    },
    trans,
};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub enum PersonCorrection {
    Initials(Initials),
    /// `None` clears the prefix, which has to be correctable to absent.
    LastNamePrefix(Option<LastNamePrefix>),
    LastName(LastName),
    DateOfBirth(DateOfBirth),
    PlaceOfResidence(PlaceOfResidence),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, Clone)]
enum PersonCorrectionKind {
    Initials,
    LastNamePrefix,
    LastName,
    DateOfBirth,
    PlaceOfResidence,
}

/// Representing a set of corrections on a single person.
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct PersonCorrectionDelta {
    corrections: HashMap<PersonCorrectionKind, PersonCorrection>,
}

impl PersonCorrectionDelta {
    /// Add a correction to this delta
    /// Replaces the previous [`PersonCorrection`] variant in the delta if present
    pub fn add_correction(&mut self, correction: PersonCorrection) {
        self.corrections.insert(correction.kind(), correction);
    }

    pub fn remove_correction(&mut self, correction: &PersonCorrection) {
        self.corrections.remove(&correction.kind());
    }

    pub fn apply(self, person: &mut Person) {
        self.corrections
            .into_iter()
            .for_each(|(_, correction)| correction.apply(person));
    }

    pub fn get_corrections(&self) -> HashSet<PersonCorrection> {
        self.corrections.values().cloned().collect()
    }
}

impl PersonCorrection {
    pub fn apply(self, person: &mut Person) {
        match self {
            PersonCorrection::Initials(initials) => {
                person.name.initials = initials;
            }
            PersonCorrection::LastNamePrefix(prefix) => {
                person.name.last_name_prefix = prefix;
            }
            PersonCorrection::LastName(last_name) => {
                person.name.last_name = last_name;
            }
            PersonCorrection::DateOfBirth(date_of_birth) => {
                person.personal_data.date_of_birth = Some(date_of_birth);
            }
            PersonCorrection::PlaceOfResidence(place_of_residence) => {
                person.personal_data.place_of_residence = Some(place_of_residence);
            }
        }
    }

    /// Whether applying this correction would change the person, i.e. its
    /// value differs from the one the person already has.
    pub fn changes(&self, person: &Person) -> bool {
        match self {
            PersonCorrection::Initials(initials) => &person.name.initials != initials,
            PersonCorrection::LastNamePrefix(prefix) => &person.name.last_name_prefix != prefix,
            PersonCorrection::LastName(last_name) => &person.name.last_name != last_name,
            PersonCorrection::DateOfBirth(date_of_birth) => {
                person.personal_data.date_of_birth.as_ref() != Some(date_of_birth)
            }
            PersonCorrection::PlaceOfResidence(place_of_residence) => {
                person.personal_data.place_of_residence.as_ref() != Some(place_of_residence)
            }
        }
    }

    pub fn change(&self, locale: Locale) -> FieldChange {
        let (field, new_value) = match self {
            PersonCorrection::Initials(v) => (
                trans!("audit_log.detail.fields.initials", locale),
                v.to_string(),
            ),
            PersonCorrection::LastNamePrefix(v) => (
                trans!("audit_log.detail.fields.last_name_prefix", locale),
                v.as_ref().map(ToString::to_string).unwrap_or_default(),
            ),
            PersonCorrection::LastName(v) => (
                trans!("audit_log.detail.fields.last_name", locale),
                v.to_string(),
            ),
            PersonCorrection::DateOfBirth(v) => (
                trans!("audit_log.detail.fields.date_of_birth", locale),
                v.to_string(),
            ),
            PersonCorrection::PlaceOfResidence(v) => (
                trans!("audit_log.detail.fields.place_of_residence", locale),
                v.to_string(),
            ),
        };
        FieldChange::Regular {
            field,
            old_value: String::new(),
            new_value,
        }
    }

    fn kind(&self) -> PersonCorrectionKind {
        match self {
            PersonCorrection::Initials(_) => PersonCorrectionKind::Initials,
            PersonCorrection::LastNamePrefix(_) => PersonCorrectionKind::LastNamePrefix,
            PersonCorrection::LastName(_) => PersonCorrectionKind::LastName,
            PersonCorrection::DateOfBirth(_) => PersonCorrectionKind::DateOfBirth,
            PersonCorrection::PlaceOfResidence(_) => PersonCorrectionKind::PlaceOfResidence,
        }
    }
}

impl Correction {
    pub fn change(&self, locale: Locale) -> FieldChange {
        match self {
            Correction::Appellation(v) => FieldChange::Regular {
                field: trans!("audit_log.detail.fields.appellation", locale),
                old_value: String::new(),
                new_value: v.to_string(),
            },
            Correction::Person(_, person_correction) => person_correction.change(locale),
        }
    }
}

/// "Ambtshalve" (ex officio) corrections, done by the CSB based on the BRP and other official records
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Correction {
    Appellation(Appellation),
    Person(PersonId, PersonCorrection),
}
