//! Synthetic test data sourced from the same `persons.csv` the server uses for
//! fixtures. Each user run mutates names with a per-user suffix so concurrent
//! sessions don't collide on the uniqueness validators.

use anyhow::Result;
use serde::Deserialize;

const PERSONS_CSV: &str = include_str!("../../src/fixtures/persons.csv");

#[derive(Debug, Deserialize, Clone)]
pub struct PersonRow {
    pub burgerservicenummer: String,
    pub geslacht: String,
    pub voornamen: String,
    pub geslachtsnaam: String,
    pub geboortedatum: String,
    pub straat: String,
    pub huisnummer: String,
    pub postcode: String,
    pub woonplaats: String,
}

impl PersonRow {
    pub fn first_name(&self) -> &str {
        self.voornamen.split_whitespace().next().unwrap_or("")
    }

    pub fn initials(&self) -> String {
        self.voornamen
            .split_whitespace()
            .filter_map(|n| n.chars().next())
            .map(|c| format!("{c}."))
            .collect()
    }

    /// Date in the dd-mm-yyyy format the server's date parser expects, or an
    /// empty string for malformed sources. (We could send the raw CSV value
    /// untouched, but it's `yyyymmdd`, not `dd-mm-yyyy`, so without
    /// reformatting every row would fail validation in the same way and
    /// we'd learn nothing.)
    pub fn date_of_birth(&self) -> String {
        chrono::NaiveDate::parse_from_str(&self.geboortedatum, "%Y%m%d")
            .map(|d| d.format("%d-%m-%Y").to_string())
            .unwrap_or_default()
    }

    pub fn gender(&self) -> &'static str {
        match self.geslacht.as_str() {
            "M" => "male",
            "V" => "female",
            _ => "",
        }
    }
}

pub fn load_persons() -> Result<Vec<PersonRow>> {
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(PERSONS_CSV.as_bytes());
    let mut out = Vec::new();
    for row in reader.deserialize::<PersonRow>() {
        out.push(row?);
    }
    Ok(out)
}

/// Apply a per-user suffix to keep names unique across concurrent sessions.
/// (Each user has their own session/store, but using the same names everywhere
/// makes server-side logs harder to read; uniqueness within a session is also
/// enforced by `PersonalDataForm::uniqueness_errors`.)
pub fn unique_last_name(base: &str, suffix: &str) -> String {
    format!("{base}-{suffix}")
}
