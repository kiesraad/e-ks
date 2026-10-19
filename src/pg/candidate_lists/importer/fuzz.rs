//! Fuzz harness for the candidate list CSV import (see `crate::fuzzing`).

use std::{
    pin::pin,
    task::{Context, Poll, Waker},
};

use http_body_util::BodyExt;

use super::{collect_persons, ensure_expected_headers, parse_records};
use crate::{
    Locale, MAX_CANDIDATES,
    candidate_lists::{CSV_HEADERS, CandidateRecordCsv},
    core::Csv,
    structs::persons::Person,
};

/// Spreadsheet formula triggers (see `escape_csv_formula`).
const FORMULA_TRIGGERS: &[u8] = b"=+-@\t\r";

/// Imports `data` as an upload; whatever imports must export without
/// formula cells or column shifts and import again as the same candidates.
pub fn candidate_csv(data: &[u8]) {
    let Some(persons) = import(data) else {
        return;
    };
    let records = as_records(&persons);
    let exported = export(&persons);
    check_cells(&exported, records.len());

    let reimported = import(&undo_formula_quotes(&exported)).unwrap_or_else(|| {
        panic!(
            "an export does not import: {}",
            String::from_utf8_lossy(&exported)
        )
    });
    let reimported = as_records(&reimported);
    assert!(
        reimported.len() == records.len()
            && reimported
                .iter()
                .zip(&records)
                .all(|(a, b)| same_record(a, b)),
        "export changed the candidates:\n{reimported:?}\n{records:?}"
    );
}

/// Equal, but where the value itself started with a quote and a trigger:
/// the undo below cannot tell that quote from the export's.
fn same_record(again: &serde_json::Value, before: &serde_json::Value) -> bool {
    let (Some(again), Some(before)) = (again.as_object(), before.as_object()) else {
        return again == before;
    };
    again.len() == before.len()
        && again.iter().all(|(name, value)| {
            let original = before.get(name);
            original == Some(value)
                || matches!((value.as_str(), original.and_then(|v| v.as_str())),
                    (Some(value), Some(original))
                        if original.strip_prefix('\'') == Some(value) && starts_formula(value))
        })
}

fn starts_formula(cell: &str) -> bool {
    cell.bytes()
        .next()
        .is_some_and(|byte| FORMULA_TRIGGERS.contains(&byte))
}

/// The export with the neutralising quote taken off formula cells again.
///
/// Finding: the import keeps that quote, so a value starting with a trigger
/// comes back changed, or fails validation (see `importer_tests::
/// formula_like_cells_import_unchanged` and
/// `formula_like_house_number_addition_imports`). Undone here, so fuzzing
/// keeps checking everything else.
fn undo_formula_quotes(exported: &[u8]) -> Vec<u8> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b';')
        .has_headers(false)
        .from_reader(exported.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(exported));
    let mut writer = csv::WriterBuilder::new()
        .delimiter(b';')
        .from_writer(Vec::new());
    for record in reader.records() {
        let record = record.expect("export parses as CSV");
        let cells = record.iter().map(|cell| match cell.strip_prefix('\'') {
            Some(rest) if starts_formula(rest) => rest,
            _ => cell,
        });
        writer.write_record(cells).expect("in memory");
    }
    writer.into_inner().expect("in memory")
}

/// The import steps before the event is stored, as `import_candidate_list_csv`
/// runs them.
fn import(data: &[u8]) -> Option<Vec<Person>> {
    ensure_expected_headers(data, Locale::Nl).ok()?;
    let mut records = parse_records(data, MAX_CANDIDATES + 1, Locale::Nl).ok()?;
    records.truncate(MAX_CANDIDATES);
    let prepared = collect_persons(records, Vec::new(), Locale::Nl).ok()?;
    Some(
        prepared
            .into_iter()
            .map(|prepared| prepared.person)
            .collect(),
    )
}

/// The candidates as the export writes them.
fn as_records(persons: &[Person]) -> Vec<serde_json::Value> {
    persons
        .iter()
        .map(|person| {
            serde_json::to_value(CandidateRecordCsv::from(person.clone())).expect("serializable")
        })
        .collect()
}

/// The download the export page serves for `persons`.
fn export(persons: &[Person]) -> Vec<u8> {
    let (response, _) = Csv {
        records: persons
            .iter()
            .cloned()
            .map(CandidateRecordCsv::from)
            .collect(),
        filename: "export.csv".to_string(),
        headers: Some(CSV_HEADERS.to_vec()),
    }
    .generate_csv_response()
    .expect("export");
    // The body is in memory, so collecting it completes on the first poll.
    let mut collect = pin!(response.into_body().collect());
    match collect
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(body) => body.expect("in-memory body").to_bytes().to_vec(),
        Poll::Pending => panic!("in-memory body pending"),
    }
}

/// Every row has exactly the header's columns and no cell starts a formula.
fn check_cells(exported: &[u8], rows: usize) {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b';')
        .has_headers(false)
        .from_reader(exported.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(exported));
    let mut count = 0;
    for record in reader.records() {
        let record = record.expect("export parses as CSV");
        assert_eq!(record.len(), CSV_HEADERS.len(), "column shift: {record:?}");
        for cell in &record {
            assert!(!starts_formula(cell), "formula cell {cell:?}");
        }
        count += 1;
    }
    assert_eq!(count, rows + 1, "rows lost or added");
}
