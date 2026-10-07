//! Example input for model OSV 3-2.

use crate::{ElectoralDistrict, models::candidate_list_summary::CandidateListSummary};

/// Two districts; De Correcte Partij has no list in Bonaire, so the blank
/// list moves up to number 2 there.
pub fn candidate_list_summary_example_1() -> CandidateListSummary {
    CandidateListSummary {
        election_name: "de Eerste Kamer der Staten-Generaal".to_string(),
        election_date: "24 mei 2027".to_string(),
        electoral_districts: ElectoralDistrict::ek_districts().to_vec(),
        lists: [
            (
                "Geen Stel Beweging".to_string(),
                ElectoralDistrict::ek_districts()
                    .into_iter()
                    .map(|&district| vec![district])
                    .collect(),
            ),
            (
                "Groep Wisselend".to_string(),
                vec![
                    vec![
                        ElectoralDistrict::Gelderland,
                        ElectoralDistrict::Groningen,
                        ElectoralDistrict::ZuidHolland,
                    ],
                    vec![
                        ElectoralDistrict::NoordBrabant,
                        ElectoralDistrict::Buitenland,
                    ],
                    vec![ElectoralDistrict::Saba],
                ],
            ),
            (
                "Lijst Fibonacci".to_string(),
                vec![vec![
                    ElectoralDistrict::Groningen,
                    ElectoralDistrict::Fryslan,
                    ElectoralDistrict::Drenthe,
                    ElectoralDistrict::Flevoland,
                    ElectoralDistrict::NoordHolland,
                    ElectoralDistrict::Bonaire,
                ]],
            ),
            (
                "Bonaire Bonaire".to_string(),
                vec![vec![ElectoralDistrict::Bonaire]],
            ),
        ]
        .into(),
    }
}
