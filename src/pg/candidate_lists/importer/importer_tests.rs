//! Property tests for the candidate list CSV import: an exported list imports
//! back as the same candidates (the round trip `Csv::generate_csv_response`
//! documents), and no uploaded file makes the import panic.

use proptest::{collection::vec, prelude::*, sample::select};

use super::{collect_persons, ensure_expected_headers, parse_records};
use crate::{
    Locale,
    candidate_lists::{CSV_HEADERS, CandidateRecordCsv},
    core::Csv,
    structs::{
        common::{BsnOrNoneConfirmed, DutchAddress},
        persons::Person,
    },
    test_support::strategies::{hostile_string, initials, person},
};

/// A candidate as the PG forms produce one: initials are required, and a
/// representative has initials but no first name.
fn form_person() -> impl Strategy<Value = Person> {
    (person(), initials(), initials()).prop_map(|(mut person, initials, representative)| {
        person.name.initials = Some(initials);
        if let Some(r) = &mut person.representative {
            r.name.first_name = None;
            r.name.initials = Some(representative);
        }
        person
    })
}

/// Whether an exported cell of `person` starts with a character the export
/// neutralises as a spreadsheet formula (by prefixing `'`).
fn exports_formula_trigger(person: &Person) -> bool {
    let address = |a: &DutchAddress| {
        vec![
            a.street_name.as_ref().map(ToString::to_string),
            a.house_number_addition.as_ref().map(ToString::to_string),
            a.locality.as_ref().map(ToString::to_string),
        ]
    };
    let mut cells = vec![
        Some(person.name.last_name.to_string()),
        person.name.first_name.as_ref().map(ToString::to_string),
        person
            .personal_data
            .place_of_residence
            .as_ref()
            .map(ToString::to_string),
    ];
    cells.extend(address(&person.address));
    if let Some(r) = &person.representative {
        cells.push(Some(r.name.last_name.to_string()));
        cells.extend(address(&r.address));
    }
    cells
        .into_iter()
        .flatten()
        .any(|cell| cell.starts_with(['=', '+', '-', '@']))
}

/// Whether the import takes `a` and `b` for the same person: the same BSN,
/// or no BSN and the same initials and last name.
fn same_identity(a: &Person, b: &Person) -> bool {
    let bsn = |p: &Person| match &p.personal_data.bsn {
        Some(BsnOrNoneConfirmed::Bsn(bsn)) => Some(bsn.to_exposed_string()),
        _ => None,
    };
    match (bsn(a), bsn(b)) {
        (Some(x), Some(y)) => x == y,
        (None, None) => a.name.initials == b.name.initials && a.name.last_name == b.name.last_name,
        _ => false,
    }
}

/// What an exported candidate imports back as: only the address that
/// applies is exported, and the BAG flags are derived again.
fn round_tripped(person: &Person) -> Person {
    let mut person = person.clone();
    if person.needs_representative() {
        person.address = DutchAddress::default();
    } else {
        person.representative = None;
    }
    without_derived(person)
}

fn without_derived(mut person: Person) -> Person {
    person.address.known_in_bag = None;
    if let Some(representative) = &mut person.representative {
        representative.address.known_in_bag = None;
    }
    person
}

fn export(persons: &[Person]) -> Vec<u8> {
    let csv = Csv {
        records: persons
            .iter()
            .cloned()
            .map(CandidateRecordCsv::from)
            .collect(),
        filename: "kandidaten.csv".to_string(),
        headers: Some(CSV_HEADERS.to_vec()),
    };
    let (response, _) = csv.generate_csv_response().expect("export");
    futures_lite::future::block_on(axum::body::to_bytes(response.into_body(), usize::MAX))
        .expect("body")
        .to_vec()
}

/// Header lines with known, unknown and repeated columns, then rows of
/// hostile cells, with either delimiter.
fn csv_like() -> impl Strategy<Value = Vec<u8>> {
    let column = prop_oneof![
        4 => select(CSV_HEADERS.to_vec()).prop_map(str::to_string),
        1 => hostile_string(8),
    ];
    (
        vec(column, 0..8),
        vec(vec(hostile_string(12), 0..8), 0..6),
        select(vec![",", ";"]),
        any::<bool>(),
    )
        .prop_map(|(header, rows, delimiter, bom)| {
            let mut out = if bom {
                "\u{feff}".to_string()
            } else {
                String::new()
            };
            out.push_str(&header.join(delimiter));
            for row in rows {
                out.push('\n');
                out.push_str(&row.join(delimiter));
            }
            out.into_bytes()
        })
}

proptest! {
    #![proptest_config(crate::test_support::strategies::parsed_config(200))]

    /// Narrowed to cells the export does not neutralise as formulas: see
    /// `formula_like_cells_import_unchanged`.
    #[test]
    fn exported_candidates_import_unchanged(
        persons in vec(form_person().prop_filter("formula-like cell", |p| !exports_formula_trigger(p)), 1..5)
    ) {
        let mut distinct: Vec<Person> = Vec::new();
        for person in persons {
            if !distinct.iter().any(|p| same_identity(p, &person)) {
                distinct.push(person);
            }
        }
        let bytes = export(&distinct);

        let ignored = ensure_expected_headers(&bytes, Locale::En);
        prop_assert!(matches!(&ignored, Ok(columns) if columns.is_empty()), "{:?}", ignored);
        let records = parse_records(&bytes, usize::MAX, Locale::En);
        let records = match records {
            Ok(records) => records,
            Err(error) => return Err(TestCaseError::fail(format!("{error:?}"))),
        };
        let imported = match collect_persons(records, Vec::new(), Locale::En) {
            Ok(imported) => imported,
            Err(error) => return Err(TestCaseError::fail(format!("{error:?}"))),
        };

        prop_assert_eq!(imported.len(), distinct.len());
        for (prepared, original) in imported.into_iter().zip(&distinct) {
            let expected = round_tripped(original);
            let actual = without_derived(Person {
                id: expected.id,
                updated_at: expected.updated_at,
                ..prepared.person
            });
            prop_assert_eq!(actual, expected);
        }
    }

    #[test]
    fn any_upload_imports_without_panicking(
        bytes in prop_oneof![vec(any::<u8>(), 0..300), csv_like()]
    ) {
        let _ = ensure_expected_headers(&bytes, Locale::Nl);
        if let Ok(records) = parse_records(&bytes, usize::MAX, Locale::Nl) {
            let _ = collect_persons(records, Vec::new(), Locale::Nl);
        }
    }
}

/// Export one candidate and import it back.
fn round_trip(person: &Person) -> Result<Person, String> {
    let bytes = export(std::slice::from_ref(person));
    let records = parse_records(&bytes, usize::MAX, Locale::En).map_err(|e| format!("{e:?}"))?;
    let mut imported =
        collect_persons(records, Vec::new(), Locale::En).map_err(|e| format!("{e:?}"))?;
    Ok(imported.remove(0).person)
}

fn sample() -> Person {
    crate::test_utils::sample_person(crate::structs::persons::PersonId::new())
}

#[test]
#[ignore = "finding: the export prefixes `'` to a cell starting with `-`, which the house number addition parser rejects on import"]
fn formula_like_house_number_addition_imports() {
    let mut person = sample();
    person.address.house_number_addition = Some("-".parse().expect("addition"));
    let imported = round_trip(&person).expect("import");
    assert_eq!(
        imported.address.house_number_addition,
        person.address.house_number_addition
    );
}

#[test]
#[ignore = "finding: the export prefixes `'` to a cell starting with `=`, `+`, `-` or `@`, and the import keeps it"]
fn formula_like_cells_import_unchanged() {
    let mut person = sample();
    person.name.last_name = "-a".parse().expect("last name");
    let imported = round_trip(&person).expect("import");
    assert_eq!(imported.name.last_name, person.name.last_name);
}
