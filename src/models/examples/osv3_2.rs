//! Example input for model OSV 3-2.

use crate::{
    ElectoralDistrict,
    models::{
        inputs::{DistrictLists, PublicSession, ValidList},
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

fn kiesraad_demo(number: usize) -> NumberedList {
    NumberedList {
        number,
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

fn correcte_partij(number: usize) -> NumberedList {
    NumberedList {
        number,
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

fn blanco_nagelhout(number: usize) -> NumberedList {
    NumberedList {
        number,
        list: ValidList {
            appellation: "Blanco (Nagelhout, H.)".to_string(),
            candidates: vec![candidate("Nagelhout, H. (v)", "Kralendijk", 1)],
        },
    }
}

/// Two districts; De Correcte Partij has no list in Bonaire, so the blank
/// list moves up to number 2 there.
pub fn osv3_2_example_1() -> OSV3_2 {
    OSV3_2 {
        election_name: "de Eerste Kamer der Staten-Generaal".to_string(),
        election_date: "24-05-2027".to_string(),
        valid_lists: vec![
            DistrictLists {
                electoral_district: ElectoralDistrict::Groningen.title().to_string(),
                lists: vec![kiesraad_demo(1), correcte_partij(2), blanco_nagelhout(3)],
            },
            DistrictLists {
                electoral_district: ElectoralDistrict::Bonaire.title().to_string(),
                lists: vec![kiesraad_demo(1), blanco_nagelhout(2)],
            },
        ],
        public_session: PublicSession {
            location: String::with_capacity(0),
            date: "03-05-2027".to_string(),
            time: String::with_capacity(0),
            chair: "M.C. Voorzitter".to_string(),
            members: Vec::with_capacity(0),
        },
    }
}
