//! Example inputs for the omission letter ("verzuimbrief").

use super::{date, postal_address};
use crate::models::{
    inputs::Person,
    omission_letter::{
        CandidateOmissions, DeclarationsOfSupport, DistrictOmissions, LetterOmission,
        OmissionLetter,
    },
};

fn omission(description: &str, help_text: &str) -> LetterOmission {
    LetterOmission {
        description: description.to_string(),
        help_text: Some(help_text.to_string()),
    }
}

/// The "Handtekening ontbreekt" preset.
fn missing_signature(position: usize, name: &str) -> LetterOmission {
    omission(
        &format!(
            "Ten aanzien van kandidaat nr. {position} {name} ontbreekt de verklaring dat deze \
             instemt met kandidaatstelling op de lijst. De verklaring wordt geacht te ontbreken \
             omdat de handtekening ontbreekt.",
        ),
        "Dit verzuim is te herstellen door een nieuwe ondertekende instemmingsverklaring (model \
         H 9) van de betreffende kandidaat in te leveren.",
    )
}

/// The "Kopie ID ontbreekt" preset.
fn missing_id_copy(position: usize, name: &str) -> LetterOmission {
    omission(
        &format!(
            "Ten aanzien van kandidaat nr. {position} {name} ontbreekt de verklaring dat deze \
             instemt met kandidaatstelling op de lijst. De verklaring van de kandidaat wordt \
             geacht te ontbreken omdat geen kopie van een geldig identiteitsbewijs is ingeleverd.",
        ),
        "Dit verzuim is te herstellen door van de kandidaat alsnog een kopie van een geldig \
         identiteitsbewijs in te leveren.",
    )
}

fn candidate(position: usize, name: &str, omissions: Vec<LetterOmission>) -> CandidateOmissions {
    CandidateOmissions {
        position: Some(position),
        name: name.to_string(),
        omissions,
    }
}

/// An all-districts section (deposit, two candidates) plus a partial one.
fn omission_groups() -> Vec<DistrictOmissions> {
    let akwasi = "Akwasi, M. (Mia)";
    let altena = "Altena, J. (Jan)";
    vec![
        DistrictOmissions {
            electoral_districts: "alle kieskringen".to_string(),
            covers_all_districts: true,
            omissions: vec![omission(
                "Voor de lijst is geen bewijs van betaling van de volledige waarborgsom \
                 ingeleverd.",
                "Dit verzuim is te herstellen door voor de lijst alsnog de waarborgsom te \
                 betalen en door een bewijs van betaling van de waarborgsom in te leveren.",
            )],
            candidates: vec![
                candidate(
                    2,
                    akwasi,
                    vec![missing_signature(2, akwasi), missing_id_copy(2, akwasi)],
                ),
                candidate(3, altena, vec![missing_id_copy(3, altena)]),
            ],
        },
        DistrictOmissions {
            electoral_districts: "kieskring 1 (Groningen), 13 (Bonaire)".to_string(),
            covers_all_districts: false,
            candidates: Vec::new(),
            omissions: vec![omission(
                "Bij de lijst zijn niet voldoende geldige verklaringen van ondersteuning \
                 ingeleverd voor de kieskringen waarvoor de lijst is ingediend. Een overzicht van \
                 het aantal ontbrekende ondersteuningsverklaringen staat in de bijlage.",
                "Dit verzuim is te herstellen door voor iedere kieskring waarin u wilt deelnemen \
                 alsnog voldoende (ten minste 30 per kieskring in Europees Nederland, voor \
                 kieskring Bonaire 10) ondertekende ondersteuningsverklaringen (model H 4) in te \
                 leveren.",
            )],
        },
    ]
}

/// A district that fell short, one that is complete, and one with no counts.
fn declarations_of_support() -> Vec<DeclarationsOfSupport> {
    vec![
        DeclarationsOfSupport {
            electoral_district: "1. Groningen".to_string(),
            submitted: Some(24),
            approved: Some(22),
            still_required: Some(8),
        },
        DeclarationsOfSupport {
            electoral_district: "9. Amsterdam".to_string(),
            submitted: Some(31),
            approved: Some(31),
            still_required: None,
        },
        DeclarationsOfSupport {
            electoral_district: "13. Bonaire".to_string(),
            submitted: None,
            approved: None,
            still_required: Some(10),
        },
    ]
}

/// Only the omissions and the appendix vary between the examples.
fn omission_letter_example(
    omission_groups: Vec<DistrictOmissions>,
    declarations_of_support: Vec<DeclarationsOfSupport>,
) -> OmissionLetter {
    OmissionLetter {
        election_name: "de Eerste Kamer der Staten-Generaal".to_string(),
        location: "'s-Gravenhage".to_string(),
        date: date(2027, 4, 25),
        addressee: Person {
            last_name: "van Smit".to_string(),
            initials: "G.H.".to_string(),
            postal_address: postal_address("Grotestraat 3", "3000 AA", "Rotterdam"),
        },
        appellation: "Kiesraad Demo".to_string(),
        election_code: "ek27".to_string(),
        omission_groups,
        recovery_deadline_date: date(2027, 4, 29),
        recovery_deadline_time: "17:00".to_string(),
        recovery_address: "het secretariaat van het centraal stembureau te 's-Gravenhage"
            .to_string(),
        chair: String::new(),
        secretary: String::new(),
        declarations_of_support,
    }
}

pub fn omission_letter_example_1() -> OmissionLetter {
    omission_letter_example(omission_groups(), declarations_of_support())
}

/// No omissions found and no appendix: the "geen verzuimen" fallback.
pub fn omission_letter_example_2() -> OmissionLetter {
    omission_letter_example(Vec::new(), Vec::new())
}
