//! Example inputs for models H 3-1 and H 3-2.

use super::{brief_candidates, model_data, standard_districts, van_smit};
use crate::{
    core::{ElectionType, ModelLocale},
    models::{
        h3::H3,
        inputs::{ElectoralDistricts, NameAuthorisation},
    },
    structs::list_designation::ListDesignation,
};

fn name_authorisation(legal_name: &str, last_name: &str, initials: &str) -> NameAuthorisation {
    NameAuthorisation {
        last_name: last_name.to_string(),
        initials: initials.to_string(),
        legal_name: legal_name.to_string(),
    }
}

pub fn h3_1_example_1() -> H3 {
    H3 {
        common: model_data(
            "Tweede Kamer der Staten-Generaal 2027",
            ElectionType::Tk,
            "Een Andere Partij (EAP)",
            brief_candidates("Ede"),
            ModelLocale::Nl,
        ),
        electoral_districts: standard_districts(),
        list_designation: ListDesignation::default(),
        list_submitter: van_smit(),
        name_authorisations: vec![name_authorisation("EEN ANDERE PARTIJ", "Altena", "J.K.S.")],
    }
}

pub fn h3_1_example_2() -> H3 {
    H3 {
        common: model_data(
            "Earste Keamerferkiezings fan de Steaten-Generaal 2027",
            ElectionType::Ek,
            "Test Partij (TP)",
            brief_candidates("Berlin"),
            ModelLocale::Fry,
        ),
        electoral_districts: ElectoralDistricts::All,
        list_designation: ListDesignation::default(),
        list_submitter: van_smit(),
        name_authorisations: vec![name_authorisation("TEST PARTIJ", "Akwasi", "O.")],
    }
}

pub fn h3_1_example_3() -> H3 {
    H3 {
        common: model_data(
            "het algemeen bestuur van het waterschap Rijn en IJssel",
            ElectionType::Ws,
            "Een Andere Partij (EAP)",
            brief_candidates("Ede"),
            ModelLocale::Nl,
        ),
        electoral_districts: ElectoralDistricts::OnlyOne,
        list_designation: ListDesignation::default(),
        list_submitter: van_smit(),
        name_authorisations: vec![name_authorisation("", "", "")],
    }
}

pub fn h3_2_example_1() -> H3 {
    H3 {
        common: model_data(
            "Tweede Kamer der Staten-Generaal 2027",
            ElectionType::Tk,
            "EAP/Test Partij (EAP/TP)",
            brief_candidates("Ede"),
            ModelLocale::Nl,
        ),
        electoral_districts: standard_districts(),
        list_designation: ListDesignation::Combined {
            registrations: Vec::new(),
        },
        list_submitter: van_smit(),
        name_authorisations: vec![
            name_authorisation("EEN ANDERE PARTIJ", "Altena", "J.K.S."),
            name_authorisation("TEST PARTIJ", "Akwasi", "O."),
        ],
    }
}

pub fn h3_2_example_2() -> H3 {
    H3 {
        common: model_data(
            "Earste Keamerferkiezings fan de Steaten-Generaal 2027",
            ElectionType::Ek,
            "EAP/Test Partij (EAP/TP)",
            brief_candidates("Berlin"),
            ModelLocale::Fry,
        ),
        electoral_districts: ElectoralDistricts::All,
        list_designation: ListDesignation::Combined {
            registrations: Vec::new(),
        },
        list_submitter: van_smit(),
        name_authorisations: vec![
            name_authorisation("EEN ANDERE PARTIJ", "Altena", "J.K.S."),
            name_authorisation("TEST PARTIJ", "Akwasi", "O."),
        ],
    }
}

pub fn h3_2_example_3() -> H3 {
    H3 {
        common: model_data(
            "het algemeen bestuur van het waterschap Rijn en IJssel",
            ElectionType::Ws,
            "EAP/Test Partij (EAP/TP)",
            brief_candidates("Ede"),
            ModelLocale::Nl,
        ),
        electoral_districts: ElectoralDistricts::OnlyOne,
        list_designation: ListDesignation::Combined {
            registrations: Vec::new(),
        },
        list_submitter: van_smit(),
        name_authorisations: vec![
            name_authorisation("", "", ""),
            name_authorisation("", "", ""),
        ],
    }
}
