use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::structs::{
    common::{Appellation, DateOfBirth, Initials, LastName, LastNamePrefix, PlaceOfResidence},
    persons::{Person, PersonId},
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

/// "Ambtshalve" (ex officio) corrections, done by the CSB based on the BRP and other official records
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Correction {
    Appellation(Appellation),
    Person(PersonId, PersonCorrection),
}
