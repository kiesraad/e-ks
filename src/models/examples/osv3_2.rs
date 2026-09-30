//! Example input for model OSV 3-2.

use crate::{
    ElectoralDistrict,
    models::{
        inputs::{DistrictLists, ValidList},
        osv3_2::{NumberedList, OSV3_2, PublishedCandidate},
    },
};

fn candidate(name: &str, locality: &str, position: usize) -> PublishedCandidate {
    PublishedCandidate {
        position,
        name: name.to_string(),
        locality: locality.to_string(),
    }
}

fn kiesraad_demo() -> NumberedList {
    NumberedList {
        number: 1,
        list: ValidList {
            appellation: "Kiesraad Demo".to_string(),
            candidates: vec![
                candidate("Kierkegaard, G.J. (Geertruda Johanna) (v)", "Ede", 1),
                candidate("Meerman, J. (Jan) (m)", "'s-Gravenhage", 2),
                candidate("van der Precise, I.E. (v)", "Rotterdam", 3),
            ],
        },
    }
}

fn correcte_partij() -> NumberedList {
    NumberedList {
        number: 2,
        list: ValidList {
            appellation: "De Correcte Partij".to_string(),
            candidates: vec![
                candidate("Akwasi, M. (v)", "Ede", 1),
                candidate("Altena, J. (m)", "'s-Gravenhage", 2),
                candidate("Bronwaßer, I.E. (v)", "Rotterdam", 3),
            ],
        },
    }
}

fn blanco_nagelhout() -> NumberedList {
    NumberedList {
        number: 3,
        list: ValidList {
            appellation: "Blanco (Nagelhout, H.)".to_string(),
            candidates: vec![candidate("Nagelhout, H. (v)", "Kralendijk", 1)],
        },
    }
}

/// Two districts; De Correcte Partij has no list in Bonaire, so list 2 is
/// missing there.
pub fn osv3_2_example_1() -> OSV3_2 {
    OSV3_2 {
        election_name: "de Eerste Kamer der Staten-Generaal".to_string(),
        election_date: "24 mei 2027".to_string(),
        valid_lists: vec![
            DistrictLists {
                electoral_district: ElectoralDistrict::Groningen.title().to_string(),
                lists: vec![kiesraad_demo(), correcte_partij(), blanco_nagelhout()],
            },
            DistrictLists {
                electoral_district: ElectoralDistrict::Bonaire.title().to_string(),
                lists: vec![kiesraad_demo(), blanco_nagelhout()],
            },
        ],
    }
}
