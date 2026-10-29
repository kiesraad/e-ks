//! Example inputs for model H 1.

use super::{
    candidate, date, full_candidates, model_data, postal_address, standard_districts,
    van_smit_with_address,
};
use crate::{
    core::{ElectionType, ModelLocale},
    models::{
        h1::H1,
        inputs::{Candidate, ElectoralDistricts, Person},
    },
    structs::list_designation::ListDesignation,
};

/// The substitute submitter shared by all three examples.
fn bronwasser() -> Person {
    Person {
        last_name: "Bronwaßer".to_string(),
        initials: "I.E.".to_string(),
        postal_address: postal_address("Grotestraat 3", "3000AA", "Rotterdam"),
    }
}

/// Three candidates with full first names, used by examples 2 and 3.
fn full_candidates_short() -> Vec<Candidate> {
    vec![
        candidate("Akwasi", "M. (Maria) (v)", date(1997, 12, 25), "Ede", 1),
        candidate(
            "Altena",
            "J. (Jeroen) (m)",
            date(2001, 10, 30),
            "'s-Gravenhage",
            2,
        ),
        candidate(
            "Bronwaßer",
            "I.E. (Ingeborg) (v)",
            date(2005, 3, 5),
            "Rotterdam",
            3,
        ),
    ]
}

pub fn h1_example_1() -> H1 {
    H1 {
        common: model_data(
            "de Eerste Kamer der Staten-Generaal",
            ElectionType::Ek,
            "Test Partij (TP)",
            full_candidates(),
            ModelLocale::Nl,
        ),
        electoral_districts: standard_districts(),
        previously_seated: true,
        list_designation: ListDesignation::default(),
        list_submitter: van_smit_with_address(),
        substitute_submitters: vec![
            bronwasser(),
            Person {
                last_name: "Jansen-de Groot".to_string(),
                initials: "C.".to_string(),
                postal_address: postal_address("Molenweg 37", "4000CC", "Amsterdam"),
            },
        ],
    }
}

pub fn h1_example_2() -> H1 {
    H1 {
        common: model_data(
            "de Tweede Kamer der Staten-Generaal",
            ElectionType::Tk,
            "EAP/Test Partij (EAP/TP)",
            full_candidates_short(),
            ModelLocale::Fry,
        ),
        electoral_districts: ElectoralDistricts::All,
        previously_seated: false,
        list_designation: ListDesignation::Combined {
            registrations: Vec::new(),
        },
        list_submitter: van_smit_with_address(),
        substitute_submitters: vec![bronwasser()],
    }
}

pub fn h1_example_3() -> H1 {
    H1 {
        common: model_data(
            "het algemeen bestuur van het waterschap Rijn en IJssel",
            ElectionType::Ws,
            "",
            full_candidates_short(),
            ModelLocale::Nl,
        ),
        electoral_districts: ElectoralDistricts::OnlyOne,
        previously_seated: false,
        list_designation: ListDesignation::Blank,
        list_submitter: van_smit_with_address(),
        substitute_submitters: vec![bronwasser()],
    }
}
