//! Example input for the candidate list overview.

use crate::{
    ElectoralDistrict,
    models::{
        candidate_list_overview::{CandidateListOverview, OverviewGroup},
        established_lists::ListSets,
    },
};

fn group(number: usize, appellation: &str, batches: Vec<Vec<ElectoralDistrict>>) -> OverviewGroup {
    OverviewGroup {
        number,
        appellation: appellation.to_string(),
        sets: ListSets::new(batches)
            .expect("example districts are valid contests")
            .expect("example group has a district"),
    }
}

/// Every list type: a lijstengroep with a different list per district, one
/// with several stels, a set of equal lists and a standalone list.
pub fn candidate_list_summary_example_1() -> CandidateListOverview {
    CandidateListOverview {
        election_name: "de Eerste Kamer der Staten-Generaal".to_string(),
        election_date: "24 mei 2027".to_string(),
        electoral_districts: ElectoralDistrict::ek_districts().to_vec(),
        groups: vec![
            group(
                1,
                "Geen Stel Beweging",
                ElectoralDistrict::ek_districts()
                    .iter()
                    .map(|&district| vec![district])
                    .collect(),
            ),
            group(
                2,
                "Groep Wisselend",
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
            group(
                3,
                "Lijst Fibonacci",
                vec![vec![
                    ElectoralDistrict::Groningen,
                    ElectoralDistrict::Fryslan,
                    ElectoralDistrict::Drenthe,
                    ElectoralDistrict::Flevoland,
                    ElectoralDistrict::NoordHolland,
                    ElectoralDistrict::Bonaire,
                ]],
            ),
            group(4, "Bonaire Bonaire", vec![vec![ElectoralDistrict::Bonaire]]),
        ],
    }
}
