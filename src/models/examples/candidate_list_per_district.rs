use crate::models::{
    candidate_list_per_district::CandidateListPerDistrict,
    examples::candidate_list_publication::{blanco_nagelhout, correcte_partij, kiesraad_demo},
};

pub fn candidate_list_per_district_example_1() -> CandidateListPerDistrict {
    CandidateListPerDistrict {
        election_name: "de Eerste Kamer der Staten-Generaal".to_string(),
        election_date: "27-05-2027".to_string(),
        electoral_district: "Groningen".to_string(),
        lists: vec![kiesraad_demo(1), correcte_partij(2), blanco_nagelhout(3)],
        file_stem: "Kandidatenlijsten_EK2027_Groningen".to_string(),
        hash: Some("C0FF 33C0 FF33 CAFE C0FF 33C0 FF33 CAFE".to_string()),
    }
}
