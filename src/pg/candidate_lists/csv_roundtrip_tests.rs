//! Property suite for the candidate list CSV (`bin/fuzz props`): uploads of
//! plausible candidates with risky cell contents go through the import,
//! export and re-import checks of the `candidate_csv` fuzz harness.

use proptest::{char::range, collection::vec, option, prelude::*, sample::select};

use super::{CSV_HEADERS, CandidateRecordCsv, importer::fuzz::candidate_csv};
use crate::{core::Csv, test_utils::proptest_config};

/// Characters the teletex validation accepts.
fn teletex(len: std::ops::Range<usize>) -> impl Strategy<Value = String> {
    vec(
        prop_oneof![4 => range(' ', '~'), 1 => range('\u{a1}', '\u{17e}')],
        len,
    )
    .prop_map(String::from_iter)
}

/// Teletex text, often behind a formula trigger, quote or delimiter.
fn risky_text() -> impl Strategy<Value = String> {
    (
        select(vec![
            "", "", "", "-", "=", "+", "@", "'", "\"", ";", ",", " ",
        ]),
        teletex(1..16),
    )
        .prop_map(|(prefix, text)| format!("{prefix}{text}"))
}

/// A BSN passing the 11-check.
fn bsn() -> impl Strategy<Value = String> {
    vec(0u32..10, 8).prop_filter_map("no check digit", |digits| {
        let sum: u32 = digits.iter().zip((2..=9).rev()).map(|(d, w)| d * w).sum();
        let check = sum % 11;
        (check < 10 && digits[0] != 0).then(|| {
            digits
                .iter()
                .chain(std::iter::once(&check))
                .map(ToString::to_string)
                .collect()
        })
    })
}

fn date() -> impl Strategy<Value = String> {
    (1u32..29, 1u32..13, 1900u32..2008).prop_map(|(d, m, y)| format!("{d:02}-{m:02}-{y}"))
}

fn postal_code() -> impl Strategy<Value = String> {
    "[1-9][0-9]{3} ?[A-Za-z]{2}"
}

fn maybe(value: impl Strategy<Value = String>) -> impl Strategy<Value = String> {
    option::of(value).prop_map(Option::unwrap_or_default)
}

/// One upload row: plausible values in every column, risky free text.
fn row() -> impl Strategy<Value = Vec<String>> {
    let name = (
        "[A-Z]\\.([A-Z]\\.){0,3}",
        maybe(risky_text()),
        maybe(select(vec!["van", "de", "van der", "'t", "d'"]).prop_map(String::from)),
        risky_text(),
    );
    let personal = (
        risky_text(),
        select(vec!["NL", "NL", "BE", "", "nl"]).prop_map(String::from),
        prop_oneof![
            bsn(),
            Just("kandidaat heeft geen BSN".to_string()),
            Just(String::new())
        ],
        maybe(date()),
        select(vec!["m", "v", "x", "", "M"]).prop_map(String::from),
    );
    let address = (
        maybe(postal_code()),
        maybe("[1-9][0-9]{0,4}"),
        maybe(risky_text()),
        maybe(risky_text()),
        maybe(risky_text()),
    );
    let representative = option::of((
        "[A-Z]\\.",
        maybe(Just("de".to_string())),
        risky_text(),
        postal_code(),
        "[1-9][0-9]{0,3}",
        maybe(risky_text()),
        risky_text(),
        risky_text(),
    ));
    (name, personal, address, representative).prop_map(|(name, personal, address, rep)| {
        let rep = rep.map_or_else(
            || vec![String::new(); 8],
            |(a, b, c, d, e, f, g, h)| vec![a, b, c, d, e, f, g, h],
        );
        [
            vec![name.0, name.1, name.2, name.3],
            vec![personal.0, personal.1, personal.2, personal.3, personal.4],
            vec![address.0, address.1, address.2, address.3, address.4],
            rep,
        ]
        .concat()
    })
}

/// The upload as a spreadsheet would save it: no formula escaping.
fn upload(rows: &[Vec<String>], delimiter: u8) -> Vec<u8> {
    let mut writer = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .from_writer(Vec::new());
    writer.write_record(CSV_HEADERS).expect("header");
    for row in rows {
        writer.write_record(row).expect("row");
    }
    writer.into_inner().expect("flushed")
}

proptest! {
    #![proptest_config(proptest_config(256))]

    /// Whatever imports exports safely and imports again unchanged.
    #[test]
    fn imported_candidates_round_trip(
        rows in vec(row(), 1..6),
        delimiter in select(vec![b';', b',']),
    ) {
        candidate_csv(&upload(&rows, delimiter));
    }

    /// Any cell content exports as one cell, never as a formula, and reads
    /// back as written (behind the neutralising quote).
    #[test]
    fn export_cells_stay_inert(cells in vec(vec(any::<String>(), 22), 1..4)) {
        let records = cells
            .iter()
            .map(|cells| {
                let fields = CSV_HEADERS
                    .iter()
                    .zip(cells)
                    .map(|(name, value)| ((*name).to_string(), value.clone().into()))
                    .collect::<serde_json::Map<_, _>>();
                serde_json::from_value::<CandidateRecordCsv>(fields.into()).expect("record")
            })
            .collect();
        let (response, _) = Csv {
            records,
            filename: "export.csv".to_string(),
            headers: Some(CSV_HEADERS.to_vec()),
        }
        .generate_csv_response()
        .expect("export");
        let body = futures_lite::future::block_on(axum::body::to_bytes(
            response.into_body(),
            usize::MAX,
        ))
        .expect("body");

        let mut reader = csv::ReaderBuilder::new()
            .delimiter(b';')
            .from_reader(body.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&body));
        let read: Vec<csv::StringRecord> = reader.records().collect::<Result<_, _>>().expect("csv");
        prop_assert_eq!(read.len(), cells.len());
        for (record, cells) in read.iter().zip(&cells) {
            prop_assert_eq!(record.len(), CSV_HEADERS.len());
            for (cell, original) in record.iter().zip(cells) {
                let neutralised = original
                    .bytes()
                    .next()
                    .is_some_and(|b| b"=+-@\t\r".contains(&b));
                let expected = if neutralised { format!("'{original}") } else { original.clone() };
                prop_assert_eq!(cell, expected.as_str());
            }
        }
    }
}
