//! Example input for model candidate list publication

use crate::{
    ElectoralDistrict,
    models::{
        candidate_list_publication::{CandidateListPublication, NumberedList, PublishedCandidate},
        inputs::{DistrictLists, ValidList},
    },
    structs::csb::HearingDetails,
};

fn candidate(name: &str, locality: &str, position: usize) -> PublishedCandidate {
    PublishedCandidate {
        position,
        name: name.to_string(),
        locality: locality.to_string(),
    }
}

pub(in crate::models::examples) fn kiesraad_demo(number: usize) -> NumberedList {
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

pub(in crate::models::examples) fn correcte_partij(number: usize) -> NumberedList {
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

pub(in crate::models::examples) fn blanco_nagelhout(number: usize) -> NumberedList {
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
pub fn candidate_list_publication_example_1() -> CandidateListPublication {
    CandidateListPublication {
        election_name: "de Eerste Kamer der Staten-Generaal".to_string(),
        election_date: super::date(2027, 5, 27),
        valid_lists: vec![
            DistrictLists {
                electoral_district: ElectoralDistrict::Groningen.title().to_string(),
                lists: vec![kiesraad_demo(1), correcte_partij(2), blanco_nagelhout(3)],
            },
            DistrictLists {
                electoral_district: ElectoralDistrict::Bonaire.title().to_string(),
                lists: vec![kiesraad_demo(1), blanco_nagelhout(3)],
            },
        ],
        session_location: "'s-Gravenhage".to_string(),
        public_session: HearingDetails {
            date_time: super::date_time(super::date(2027, 5, 3), 17, 0),
            chair: "M.C. Voorzitter".to_string(),
            members: Vec::new(),
        },
    }
}
