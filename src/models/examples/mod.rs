//! Example inputs for the PDF models, one file per model. These drive the
//! `renders_every_example_input` round-trip test and the `pdf_diff`
//! development tool.

mod h1;
mod h3;
mod h4;
mod h9;
mod i1;
mod i4;
mod omission_letter;

pub use h1::{h1_example_1, h1_example_2, h1_example_3};
pub use h3::{
    h3_1_example_1, h3_1_example_2, h3_1_example_3, h3_2_example_1, h3_2_example_2, h3_2_example_3,
};
pub use h4::{h4_example_1, h4_example_2, h4_example_3};
pub use h9::{h9_example_1, h9_example_2, h9_example_3};
pub use i1::{i1_example_1, i1_example_2};
pub use i4::{i4_example_1, i4_example_2};
pub use omission_letter::{omission_letter_example_1, omission_letter_example_2};

use textris_pdf::build::Textris;

use super::{
    Pdf,
    inputs::{Candidate, ElectoralDistricts, ModelData, Person, PostalAddress},
};
use crate::{
    AppError,
    core::{ElectionType, ModelLocale},
};

/// A named example input, renderable to a PDF. `name` matches the former JSON
/// file stem (e.g. `model-h1-example-1`) so the `pdf_diff` baseline lines up.
#[derive(Debug)]
pub struct Example {
    pub name: &'static str,
    pub filename: String,
    document: Textris,
}

impl Example {
    /// Render this example to accessible PDF bytes.
    pub fn render(self) -> Result<Vec<u8>, AppError> {
        Ok(self.document.render(super::fonts())?)
    }

    /// Export this example as Word (`.docx`) bytes.
    pub fn to_docx(&self) -> Result<Vec<u8>, AppError> {
        self.document.to_docx().map_err(AppError::DocxError)
    }
}

fn example<T: Pdf>(name: &'static str, model: T) -> Example {
    Example {
        name,
        filename: model.filename(),
        document: model.document().expect("build example document"),
    }
}

/// Every example input, in a stable order.
pub fn examples() -> Vec<Example> {
    vec![
        example("model-h1-example-1", h1_example_1()),
        example("model-h1-example-2", h1_example_2()),
        example("model-h1-example-3", h1_example_3()),
        example("model-h3-1-example-1", h3_1_example_1()),
        example("model-h3-1-example-2", h3_1_example_2()),
        example("model-h3-1-example-3", h3_1_example_3()),
        example("model-h3-2-example-1", h3_2_example_1()),
        example("model-h3-2-example-2", h3_2_example_2()),
        example("model-h3-2-example-3", h3_2_example_3()),
        example("model-h4-example-1", h4_example_1()),
        example("model-h4-example-2", h4_example_2()),
        example("model-h4-example-3", h4_example_3()),
        example("model-h9-example-1", h9_example_1()),
        example("model-h9-example-2", h9_example_2()),
        example("model-h9-example-3", h9_example_3()),
        example("model-i1-example-1", i1_example_1()),
        example("model-i1-example-2", i1_example_2()),
        example("model-i4-example-1", i4_example_1()),
        example("model-i4-example-2", i4_example_2()),
        example("verzuimbrief-example-1", omission_letter_example_1()),
        example("verzuimbrief-example-2", omission_letter_example_2()),
    ]
}

// --- building blocks shared by the model examples ----------------------------

const SHA_HASH: &str = "F381 3DE7 96D3 8033 FAF5 8D2C E694 61F0";
const EVENT_ID: usize = 42;

/// The common model data; every example shares the event version and hash.
fn model_data(
    election_name: &str,
    election_type: ElectionType,
    appellation: &str,
    candidates: Vec<Candidate>,
    locale: ModelLocale,
) -> ModelData {
    ModelData {
        election_name: election_name.to_string(),
        election_type,
        appellation: appellation.to_string(),
        candidates,
        locale,
        event_id: EVENT_ID,
        sha_hash: SHA_HASH.to_string(),
    }
}

fn date(year: i32, month: u32, day: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(year, month, day).expect("valid example date")
}

fn candidate(
    last_name: &str,
    initials: &str,
    dob: chrono::NaiveDate,
    locality: &str,
    position: usize,
) -> Candidate {
    Candidate {
        last_name: last_name.to_string(),
        initials: initials.to_string(),
        date_of_birth: dob,
        locality: locality.to_string(),
        position,
    }
}

fn postal_address(street_address: &str, postal_code: &str, locality: &str) -> PostalAddress {
    PostalAddress {
        street_address: street_address.to_string(),
        postal_code: postal_code.to_string(),
        locality: locality.to_string(),
    }
}

/// The list submitter shared by all H-model examples (no postal address).
fn van_smit() -> Person {
    Person {
        last_name: "van Smit".to_string(),
        initials: "G.H.".to_string(),
        postal_address: PostalAddress::default(),
    }
}

/// The list submitter with a postal address (used by H 1).
fn van_smit_with_address() -> Person {
    Person {
        postal_address: postal_address("Grotestraat 3", "3000AA", "Rotterdam"),
        ..van_smit()
    }
}

/// The recurring three electoral districts used by the `Some` examples.
fn standard_districts() -> ElectoralDistricts {
    ElectoralDistricts::Some(vec![
        "Provincie Utrecht".to_string(),
        "Gemeente Rotterdam".to_string(),
        "Gemeenten Berg en Dal, Beuningen, Buren, Culemborg, Druten, Heumen, Maasdriel, \
         Neder-Betuwe, Nijmegen, Tiel, West Betuwe, West Maas en Waal, Wijchen, Zaltbommel"
            .to_string(),
    ])
}

/// The five candidates with full first names, used by H 1 (example 1) and H 4.
fn full_candidates() -> Vec<Candidate> {
    vec![
        candidate("Abels", "A. (Astrid) (v)", date(1976, 2, 1), "Ede", 1),
        candidate("Akwasi", "M. (Maria) (v)", date(1997, 12, 25), "Ede", 2),
        candidate(
            "Altena",
            "J. (Jeroen) (m)",
            date(2001, 10, 30),
            "'s-Gravenhage",
            3,
        ),
        candidate(
            "Bronwaßer",
            "I.E. (Ingeborg) (v)",
            date(2005, 3, 5),
            "Rotterdam",
            4,
        ),
        candidate(
            "Jansen-de Groot",
            "C. (Christine) (v)",
            date(1960, 5, 9),
            "Amsterdam",
            5,
        ),
    ]
}

/// Three candidates with initials only, used by H 3 and H 9. The first
/// candidate's locality varies between examples.
fn brief_candidates(first_locality: &str) -> Vec<Candidate> {
    vec![
        candidate("Akwasi", "M. (v)", date(1997, 12, 25), first_locality, 1),
        candidate("Altena", "J. (m)", date(2001, 10, 30), "'s-Gravenhage", 2),
        candidate("Bronwaßer", "I.E. (v)", date(2005, 3, 5), "Rotterdam", 3),
    ]
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(ToString::to_string).collect()
}
