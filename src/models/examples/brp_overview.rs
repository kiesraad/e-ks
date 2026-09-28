//! Example inputs for the pre-submission overview.

use super::date;
use crate::models::brp_overview::{BrpOverview, CandidateDetail, OverviewCandidate};

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(ToString::to_string).collect()
}

/// The details rows in the order the candidate page lists them.
fn details(
    initials: &str,
    prefix: &str,
    last_name: &str,
    gender: &str,
    date_of_birth: &str,
    bsn: &str,
    place_of_residence: &str,
) -> Vec<CandidateDetail> {
    [
        ("Voorletters", initials),
        ("Voorvoegsel", prefix),
        ("Achternaam", last_name),
        ("Geslacht", gender),
        ("Geboortedatum", date_of_birth),
        ("Burgerservicenummer (BSN)", bsn),
        ("Woonplaats", place_of_residence),
    ]
    .into_iter()
    .map(|(label, value)| CandidateDetail {
        label: label.to_string(),
        value: value.to_string(),
    })
    .collect()
}

fn candidate(
    position: usize,
    name: &str,
    details: Vec<CandidateDetail>,
    findings: &[&str],
    problems: &[&str],
) -> OverviewCandidate {
    OverviewCandidate {
        position,
        name: name.to_string(),
        details,
        findings: strings(findings),
        problems: strings(problems),
    }
}

/// The counts follow from `candidates`, as if a list of 55 was checked.
fn brp_overview_example(complete: bool, candidates: Vec<OverviewCandidate>) -> BrpOverview {
    let candidates_with_brp_errors = candidates
        .iter()
        .filter(|candidate| !candidate.findings.is_empty())
        .count();
    BrpOverview {
        election_name: "de Eerste Kamer der Staten-Generaal".to_string(),
        appellation: "Kiesraad Demo".to_string(),
        election_code: "ek27".to_string(),
        date: date(2027, 4, 12),
        complete,
        candidates_without_brp_errors: if complete {
            55 - candidates_with_brp_errors
        } else {
            0
        },
        candidates_with_brp_errors,
        brp_error_count: candidates
            .iter()
            .map(|candidate| candidate.findings.len())
            .sum(),
        problem_count: candidates
            .iter()
            .map(|candidate| candidate.problems.len())
            .sum(),
        candidates,
    }
}

/// Candidates with BRP findings, one of them on two fields, one with only the
/// application's own warnings, and one with both.
pub fn brp_overview_example_1() -> BrpOverview {
    brp_overview_example(
        true,
        vec![
            candidate(
                1,
                "Akwasi, M. (Mia)",
                details(
                    "M.",
                    "",
                    "Akwasi",
                    "Vrouw",
                    "25-12-1997",
                    "999993653",
                    "Ede",
                ),
                &[
                    "De woonplaats verschilt van de BRP: Utrecht",
                    "De voorletters verschillen van de BRP: A.B.C.",
                ],
                &[],
            ),
            candidate(
                2,
                "Altena, J. (Jan)",
                details(
                    "J.",
                    "",
                    "Altena",
                    "Man",
                    "30-10-2001",
                    "999990627",
                    "'s-Gravenhage",
                ),
                &["Het burgerservicenummer is onbekend in de BRP"],
                &[],
            ),
            candidate(
                3,
                "Bronwaßer, I.E. (Ingeborg)",
                details("I.E.", "", "Bronwaßer", "Vrouw", "05-03-2005", "", ""),
                &[],
                &["Geen BSN opgegeven", "Geen woonplaats opgegeven"],
            ),
            candidate(
                5,
                "de Groot, C. (Christine)",
                details(
                    "C.",
                    "de",
                    "Groot",
                    "Vrouw",
                    "09-05-1900",
                    "999991772",
                    "Amsterdam",
                ),
                &["Volgens de BRP heeft deze kandidaat niet de Nederlandse nationaliteit"],
                &["De geboortedatum ligt erg lang geleden"],
            ),
        ],
    )
}

/// Nothing found yet, with the check unfinished: the "geen bevindingen"
/// fallback under the incomplete warning.
pub fn brp_overview_example_2() -> BrpOverview {
    brp_overview_example(false, Vec::new())
}
