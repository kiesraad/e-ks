//! Example inputs for model I 4.

use super::strings;
use crate::models::i4::{
    CorrectedAppellation, DistrictLists, I4, NumberedOnDistricts, NumberedOnVotes, OmissionGroup,
    PublicSession, RemovedAppellation, RemovedCandidate, RemovedCandidates, ValidList,
    ValidListCandidate,
};

fn omission_group(
    appellation: &str,
    electoral_district: &str,
    descriptions: &[&str],
) -> OmissionGroup {
    OmissionGroup {
        appellation: appellation.to_string(),
        electoral_district: electoral_district.to_string(),
        omission_descriptions: strings(descriptions),
    }
}

fn valid_list_candidate(
    last_name: &str,
    initials: &str,
    locality: &str,
    position: usize,
) -> ValidListCandidate {
    ValidListCandidate {
        last_name: last_name.to_string(),
        initials: initials.to_string(),
        locality: locality.to_string(),
        position,
    }
}

fn i4_public_session() -> PublicSession {
    PublicSession {
        location: "'s-Gravenhage".to_string(),
        date: "3 mei 2027".to_string(),
        time: "17:00 uur".to_string(),
        chair: "M.C. Voorzitter".to_string(),
        members: strings(&["A. Lid", "B. Lid", "C. Lid", "D. Lid", "E. Lid", "F. Lid"]),
    }
}

fn i4_found_omissions() -> Vec<OmissionGroup> {
    vec![
        omission_group(
            "De Geconstateerde Partij",
            "kieskring 20 (Bonaire)",
            &[
                "Ten aanzien van kandidaat nr. 3 J. Altena ontbreekt de verklaring dat deze instemt met kandidaatstelling op de lijst.",
                "Ten aanzien van kandidaat nr. 12 G. Braber ontbreekt de verklaring dat deze instemt met kandidaatstelling op de lijst.",
            ],
        ),
        omission_group(
            "Kiesraad Demo 3",
            "alle kieskringen",
            &[
                "Ten aanzien van kandidaat nr. 3 J. Altena ontbreekt de verklaring dat deze instemt met kandidaatstelling op de lijst.",
            ],
        ),
    ]
}

fn i4_recovered_omissions() -> Vec<OmissionGroup> {
    vec![
        omission_group(
            "De Herstelde Partij",
            "kieskring 20 (Bonaire)",
            &[
                "Ten aanzien van kandidaat nr. 3 J. Altena ontbreekt de verklaring dat deze instemt met kandidaatstelling op de lijst.",
                "Ten aanzien van kandidaat nr. 12 G. Braber ontbreekt de verklaring dat deze instemt met kandidaatstelling op de lijst.",
            ],
        ),
        omission_group(
            "Kiesraad Demo 4",
            "alle kieskringen",
            &[
                "Ten aanzien van kandidaat nr. 3 J. Altena ontbreekt de verklaring dat deze instemt met kandidaatstelling op de lijst.",
            ],
        ),
    ]
}

fn i4_invalid_lists() -> Vec<OmissionGroup> {
    vec![
        omission_group(
            "De Ongeldige Partij",
            "kieskring 20 (Bonaire)",
            &[
                "Bij de lijst zijn niet voldoende geldige verklaringen van ondersteuning ingeleverd voor de kieskring Bonaire.",
            ],
        ),
        omission_group(
            "Kiesraad Demo 5",
            "alle kieskringen",
            &[
                "Bij de lijst zijn niet voldoende geldige verklaringen van ondersteuning ingeleverd voor alle kieskringen.",
                "Voor de lijst is geen bewijs van betaling van de waarborgsom ingeleverd.",
            ],
        ),
    ]
}

fn i4_removed_candidates() -> Vec<RemovedCandidates> {
    vec![
        RemovedCandidates {
            appellation: "De Geschrapte Kandidaten Partij".to_string(),
            electoral_district: "kieskring 20 (Bonaire)".to_string(),
            candidates: vec![RemovedCandidate {
                name: "Vermeulen, H. (Henk) (m)".to_string(),
                reasons: strings(&[
                    "Ten aanzien van kandidaat nr. 24 H. Vermeulen ontbreekt de verklaring dat deze instemt met kandidaatstelling op de lijst. De verklaring van de kandidaat wordt geacht te ontbreken omdat geen kopie van een geldig identiteitsbewijs is ingeleverd.",
                ]),
            }],
        },
        RemovedCandidates {
            appellation: "Kiesraad Demo 6".to_string(),
            electoral_district: "alle kieskringen".to_string(),
            candidates: vec![
                RemovedCandidate {
                    name: "Meerman, K.S. (Kevin) (m)".to_string(),
                    reasons: strings(&[
                        "Ten aanzien van kandidaat nr. 2 K.S Meerman ontbreekt de verklaring dat deze instemt met kandidaatstelling op de lijst.",
                    ]),
                },
                RemovedCandidate {
                    name: "Olympos, T. (Thanatos) (m)".to_string(),
                    reasons: strings(&[
                        "Ten aanzien van kandidaat nr. 9 T. Olympos ontbreekt de verklaring dat deze instemt met kandidaatstelling op de lijst.",
                    ]),
                },
            ],
        },
    ]
}

fn i4_removed_appellations() -> Vec<RemovedAppellation> {
    vec![RemovedAppellation {
        appellation: "De Geschrapte Aanduiding Partij".to_string(),
        electoral_district: "kieskring 20 (Bonaire)".to_string(),
        first_candidate_name: "Nagelhout, H. (Hubertus) (m)".to_string(),
        reasons: strings(&[
            "De aanduiding stemt niet overeen met de bij het Centraal Stembureau geregistreerde naam van de politieke groepering.",
        ]),
    }]
}

fn i4_corrected_appellations() -> Vec<CorrectedAppellation> {
    vec![CorrectedAppellation {
        first_candidate_name: "Nagelhout, H. (Hubertus) (m)".to_string(),
        electoral_district: "alle kieskringen".to_string(),
        submitted_appellation: "AAP".to_string(),
        edited_appellation: "De Aangepaste Aanduiding Partij (AAP)".to_string(),
    }]
}

fn i4_valid_lists() -> Vec<DistrictLists> {
    let correcte_partij = || ValidList {
        appellation: "De Correcte Partij".to_string(),
        candidates: vec![
            valid_list_candidate("Akwasi", "M. (v)", "Ede", 1),
            valid_list_candidate("Altena", "J. (m)", "'s-Gravenhage", 2),
            valid_list_candidate("Bronwaßer", "I.E. (v)", "Rotterdam", 3),
        ],
    };
    let kiesraad_demo = || ValidList {
        appellation: "Kiesraad Demo".to_string(),
        candidates: vec![
            valid_list_candidate("Nagelhout", "M. (v)", "Ede", 1),
            valid_list_candidate("Meerman", "J. (m)", "'s-Gravenhage", 2),
            valid_list_candidate("Precise", "I.E. (v)", "Rotterdam", 3),
        ],
    };
    vec![
        DistrictLists {
            electoral_district: "Lelystad".to_string(),
            lists: vec![
                correcte_partij(),
                kiesraad_demo(),
                ValidList {
                    appellation: "Blanco (Nagelhout, H.)".to_string(),
                    candidates: vec![valid_list_candidate("Nagelhout", "H. (v)", "Ede", 1)],
                },
            ],
        },
        DistrictLists {
            electoral_district: "Amsterdam".to_string(),
            lists: vec![correcte_partij(), kiesraad_demo()],
        },
    ]
}

fn i4_numbered_based_on_votes() -> Vec<NumberedOnVotes> {
    vec![
        NumberedOnVotes {
            position: Some(1),
            appellation: "Kiesraad Demo".to_string(),
            previous_votes: 37092,
        },
        NumberedOnVotes {
            position: Some(2),
            appellation: "De Herstelde Partij".to_string(),
            previous_votes: 1621,
        },
    ]
}

/// Assemble an I 4 example; only the numbering-by-districts, objections and the
/// response to objections differ between the two examples.
fn i4_example(
    numbered_based_on_districts: Vec<NumberedOnDistricts>,
    objections: Option<Vec<String>>,
    response_objections: Option<String>,
) -> I4 {
    I4 {
        election_name: "de Eerste Kamer der Staten-Generaal".to_string(),
        election_date: "24 mei 2027".to_string(),
        public_session: i4_public_session(),
        found_omissions: i4_found_omissions(),
        recovered_omissions: i4_recovered_omissions(),
        invalid_lists: i4_invalid_lists(),
        removed_candidates: i4_removed_candidates(),
        removed_appellations: i4_removed_appellations(),
        corrected_appellations: i4_corrected_appellations(),
        valid_lists: i4_valid_lists(),
        numbered_based_on_votes: i4_numbered_based_on_votes(),
        numbered_based_on_districts,
        objections,
        response_objections,
    }
}

pub fn i4_example_1() -> I4 {
    i4_example(
        vec![
            NumberedOnDistricts {
                position: Some(3),
                appellation: "De Correcte Partij".to_string(),
                districts: 2,
            },
            NumberedOnDistricts {
                position: Some(4),
                appellation: "Blanco (Nagelhout, H.)".to_string(),
                districts: 1,
            },
        ],
        Some(strings(&[
            "Namens De Ongeldige Partij is bezwaar gemaakt tegen het proces van het verkrijgen van ondersteuningsverklaringen. De partij stelt vele belemmeringen te hebben ervaren bij gemeenten en te weinig mogelijkheden te hebben ervaren bij verzuimherstel. Gesteld wordt dat dit in strijd is met de algemene beginselen van behoorlijk bestuur, zoals het beginsel van opgewekt vertrouwen en fair play. Partijen moet een redelijke kans geboden worden op herstel van verzuimen. De bezwaarmaker verzet zich tegen het strikt toepassen van termijnen voor kiezers die de partij wilden ondersteunen en verzoekt alsnog extra tijd voor herstel van verzuimen.",
            "Een bezwaarmaker namens de partij Kiesraad Demo 5 sluit zich aan bij het voorgaande bezwaar voor wat betreft het ondervinden van belemmeringen bij de ondersteuningsverklaringen.",
            "Namens De Herstelde Partij wordt de Kiesraad bedankt voor al het werk en de hulp bij het proces. De partij heeft dat als zeer prettig ervaren, maar het zou fijn zijn als het systeem wordt aangepast.",
        ])),
        Some("Reactie van de Kiesraad op de bezwaren:\nWat betreft de opmerkingen die door een aantal bezwaarmakers zijn gemaakt over het proces van het verkrijgen van ondersteuningsverklaringen stelt de Kiesraad dit ook heel vervelend te vinden. Het gaat hier over een proces onder de verantwoordelijkheid van de gemeenten. De Kiesraad geeft gemeenten informatie en instrueert hen.".to_string()),
    )
}

pub fn i4_example_2() -> I4 {
    i4_example(
        vec![
            NumberedOnDistricts {
                position: None,
                appellation: "De Correcte Partij".to_string(),
                districts: 2,
            },
            NumberedOnDistricts {
                position: None,
                appellation: "Blanco (Nagelhout, H.)".to_string(),
                districts: 2,
            },
        ],
        None,
        None,
    )
}
