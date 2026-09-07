//! Synthetic test data sourced from the same `persons.csv` the server uses for
//! fixtures. Each user run mutates names with a per-user suffix so concurrent
//! sessions don't collide on the uniqueness validators.

use std::sync::OnceLock;

use anyhow::Result;
use serde::Deserialize;

const PERSONS_CSV: &str = include_str!("../../src/fixtures/persons.csv");

/// The app's own last-name prefix table (RvIG table 36), read out of its
/// source file rather than copied, so the two cannot drift.
const LAST_NAME_PREFIX_SOURCE: &str = include_str!("../../src/structs/common/last_name_prefix.rs");

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

    /// The CSV writes the last name the way it appears on a candidate list,
    /// prefix included; the form takes the two in separate fields. See
    /// [`split_last_name_prefix`].
    pub fn last_name_parts(&self) -> (Option<&str>, &str) {
        split_last_name_prefix(&self.geslachtsnaam)
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

/// Split a written-out last name into its prefix and the name itself, taking
/// the longest prefix the value starts with (prefixes such as "voor in 't"
/// span several words). Mirrors the app's `split_last_name_prefix`, which the
/// fixture loader uses for the same CSV.
///
/// This has to happen client-side: `LastName` rejects a value whose first word
/// is a prefix, so posting "de Goede" as `last_name` fails validation and 29
/// of the 105 fixture rows would never be created.
pub fn split_last_name_prefix(value: &str) -> (Option<&str>, &str) {
    let value = value.trim();
    let table = last_name_prefixes();
    match value
        .match_indices(' ')
        .filter(|(index, _)| table.contains(&&value[..*index]))
        .map(|(index, _)| index)
        .next_back()
    {
        Some(index) => (Some(&value[..index]), value[index..].trim_start()),
        None => (None, value),
    }
}

/// The prefix table, parsed out of the app's source once. The literals hold no
/// escapes and no `"`, so splitting on the quote is enough; the length check
/// turns a change in the table's shape into a loud failure rather than a
/// silently empty table.
fn last_name_prefixes() -> &'static [&'static str] {
    static PREFIXES: OnceLock<Vec<&'static str>> = OnceLock::new();
    PREFIXES.get_or_init(|| {
        let body = LAST_NAME_PREFIX_SOURCE
            .split_once("const LAST_NAME_PREFIXES")
            .and_then(|(_, rest)| rest.split_once("= ["))
            .and_then(|(_, rest)| rest.split_once("];"))
            .map(|(body, _)| body)
            .expect("app still declares LAST_NAME_PREFIXES as an array literal");
        let prefixes: Vec<&str> = body.split('"').skip(1).step_by(2).collect();
        assert!(
            prefixes.len() > 300,
            "extracted only {} last-name prefixes from the app source: the table's shape changed",
            prefixes.len()
        );
        prefixes
    })
}

/// Apply a per-user suffix to keep names unique across concurrent sessions.
/// (Each user has their own session/store, but using the same names everywhere
/// makes server-side logs harder to read; uniqueness within a session is also
/// enforced by `PersonalDataForm::uniqueness_errors`.)
pub fn unique_last_name(base: &str, suffix: &str) -> String {
    format!("{base}-{suffix}")
}
