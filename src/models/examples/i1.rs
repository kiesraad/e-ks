//! Example inputs for model I 1.

use super::strings;
use crate::models::{
    i1::{I1, SubmittedList},
    inputs::{DistrictLists, OmissionGroup, PublicSession},
};

fn i1_session() -> PublicSession {
    PublicSession {
        location: "'s-Gravenhage".to_string(),
        date: "05-04-2027".to_string(),
        time: "16:00 uur".to_string(),
        chair: "M.C. Voorzitter".to_string(),
        members: strings(&["A. Lid", "B. Lid", "C. Lid", "D. Lid", "E. Lid", "F. Lid"]),
    }
}

fn submitted_list(
    appellation: &str,
    first_candidate_name: &str,
    candidate_count: usize,
) -> SubmittedList {
    SubmittedList {
        appellation: appellation.to_string(),
        first_candidate_name: first_candidate_name.to_string(),
        candidate_count,
    }
}

/// Two districts; "De Correcte Partij" submitted in both, the blank list only
/// in Bonaire.
fn i1_submitted_lists() -> Vec<DistrictLists<SubmittedList>> {
    let correcte_partij = || submitted_list("De Correcte Partij", "Akwasi, M. (Maria)", 30);
    let kiesraad_demo = || submitted_list("Kiesraad Demo", "Nagelhout, M. (Marieke)", 12);

    vec![
        DistrictLists {
            electoral_district: "1 (Groningen)".to_string(),
            lists: vec![correcte_partij(), kiesraad_demo()],
        },
        DistrictLists {
            electoral_district: "13 (Bonaire)".to_string(),
            lists: vec![
                correcte_partij(),
                kiesraad_demo(),
                submitted_list("Blanco (Nagelhout, H.)", "Nagelhout, H. (Hubertus)", 1),
            ],
        },
    ]
}

fn i1_found_omissions() -> Vec<OmissionGroup> {
    vec![
        OmissionGroup {
            appellation: "De Geconstateerde Partij".to_string(),
            electoral_district: "kieskring 20 (Bonaire)".to_string(),
            omission_descriptions: strings(&[
                "Ten aanzien van kandidaat nr. 3 J. Altena ontbreekt de verklaring dat deze instemt met kandidaatstelling op de lijst.",
                "Ten aanzien van kandidaat nr. 12 G. Braber ontbreekt de verklaring dat deze instemt met kandidaatstelling op de lijst.",
            ]),
        },
        OmissionGroup {
            appellation: "Kiesraad Demo 3".to_string(),
            electoral_district: "alle kieskringen".to_string(),
            omission_descriptions: strings(&[
                "Bij de lijst zijn niet voldoende geldige verklaringen van ondersteuning ingeleverd voor alle kieskringen.",
            ]),
        },
    ]
}

/// Both I 1 examples share the election and the session; only the omissions
/// found vary.
fn i1_example(found_omissions: Vec<OmissionGroup>) -> I1 {
    I1 {
        election_name: "de Eerste Kamer der Staten-Generaal".to_string(),
        election_date: "24-05-2027".to_string(),
        session: i1_session(),
        submitted_lists: i1_submitted_lists(),
        found_omissions,
    }
}

pub fn i1_example_1() -> I1 {
    i1_example(i1_found_omissions())
}

/// No omissions found: the "geen verzuimen" fallback.
pub fn i1_example_2() -> I1 {
    i1_example(Vec::new())
}
