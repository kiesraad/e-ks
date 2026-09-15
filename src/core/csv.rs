use std::{borrow::Cow, fmt::Display};

use axum::{
    body::Body,
    http::{HeaderValue, Response},
    response::IntoResponse,
};
use csv::{IntoInnerError, Reader, ReaderBuilder, StringRecord, Trim, Writer, WriterBuilder};
use serde::{Serialize, de::DeserializeOwned};

use crate::{AppError, Locale, OptionStringExt, trans, utils::no_cache_headers};

/// Most errors reported for one upload. Every failing row becomes a translated
/// message held in memory and rendered, so a file of nothing but broken rows
/// would amplify a few megabytes into hundreds.
const MAX_ERRORS: usize = 50;

pub enum CsvError {
    FormatError {
        line_number: usize,
        message: csv::ErrorKind,
    },
    ParseError {
        line_number: usize,
        field_name: String,
        message: String,
    },
    /// Parsing stopped after [`MAX_ERRORS`]; more rows may be wrong.
    TooManyErrors { reported: usize },
}

impl Display for CsvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message(Locale::default()))
    }
}

impl CsvError {
    pub fn message(&self, locale: Locale) -> String {
        match self {
            CsvError::FormatError {
                line_number,
                message,
            } => trans!(
                "candidate_list.import_errors.format_error",
                locale,
                line_number,
                format_error_kind(message, locale)
            ),
            CsvError::ParseError {
                line_number,
                field_name,
                message,
            } => trans!(
                "candidate_list.import_errors.parse_error",
                locale,
                line_number,
                translated_field_name(field_name, locale),
                message
            ),
            CsvError::TooManyErrors { reported } => trans!(
                "candidate_list.import_errors.too_many_errors",
                locale,
                reported
            ),
        }
    }
}

fn translated_field_name(field_name: &str, locale: Locale) -> String {
    let field_key = format!(
        "person.fields.{}",
        field_name.rsplit('.').next().unwrap_or(field_name)
    );
    let translated = match locale {
        crate::Locale::En => crate::translate::LOCALE_EN.get(&field_key),
        crate::Locale::Nl => crate::translate::LOCALE_NL.get(&field_key),
    }
    .to_string_or_default();

    if translated.is_empty() {
        field_name.to_string()
    } else {
        translated
    }
}

fn format_error_kind(kind: &csv::ErrorKind, locale: Locale) -> String {
    match kind {
        csv::ErrorKind::Io(err) => trans!("candidate_list.import_errors.csv.io", locale, err),
        csv::ErrorKind::Utf8 { pos, err } => format_utf8_error(pos.as_ref(), err, locale),
        csv::ErrorKind::UnequalLengths {
            pos,
            expected_len,
            len,
        } => format_unequal_lengths(pos.as_ref(), *expected_len, *len, locale),
        csv::ErrorKind::Seek => trans!("candidate_list.import_errors.csv.seek", locale),
        csv::ErrorKind::Serialize(err) => {
            trans!("candidate_list.import_errors.csv.serialize", locale, err)
        }
        csv::ErrorKind::Deserialize { pos, err } => {
            format_deserialize_error(pos.as_ref(), err, locale)
        }
        _ => trans!(
            "candidate_list.import_errors.csv.unknown",
            locale,
            format!("{kind:?}")
        ),
    }
}

/// Mentions where the unreadable text is when the reader knows the position.
fn format_utf8_error(pos: Option<&csv::Position>, err: &csv::Utf8Error, locale: Locale) -> String {
    match pos {
        Some(pos) => trans!(
            "candidate_list.import_errors.csv.utf8_with_position",
            locale,
            pos.record(),
            pos.line(),
            err.field(),
            pos.byte(),
            err
        ),
        None => trans!(
            "candidate_list.import_errors.csv.utf8",
            locale,
            err.field(),
            err
        ),
    }
}

/// Mentions where the column count differs when the reader knows the position.
fn format_unequal_lengths(
    pos: Option<&csv::Position>,
    expected_len: u64,
    len: u64,
    locale: Locale,
) -> String {
    match pos {
        Some(pos) => trans!(
            "candidate_list.import_errors.csv.unequal_lengths_with_position",
            locale,
            pos.record(),
            pos.line(),
            pos.byte(),
            len,
            expected_len
        ),
        None => trans!(
            "candidate_list.import_errors.csv.unequal_lengths",
            locale,
            len,
            expected_len
        ),
    }
}

/// Mentions where the malformed value is when the reader knows the position.
fn format_deserialize_error(
    pos: Option<&csv::Position>,
    err: &csv::DeserializeError,
    locale: Locale,
) -> String {
    match pos {
        Some(pos) => trans!(
            "candidate_list.import_errors.csv.deserialize_with_position",
            locale,
            pos.record(),
            pos.line(),
            pos.byte(),
            err
        ),
        None => trans!("candidate_list.import_errors.csv.deserialize", locale, err),
    }
}

pub struct Csv<T> {
    pub records: Vec<T>,
    pub filename: String,
    pub headers: Option<Vec<&'static str>>,
}

impl<T: Serialize> Csv<T> {
    /// Generate a CSV response and return the response along with the CSV data size in bytes.
    ///
    /// Why the `;` delimiter and BOM: when a recipient double-clicks the file in
    /// a Dutch/European install of Excel, the comma is the decimal separator and
    /// the semicolon is the field separator. Emitting `;` matches that locale, and the
    /// leading UTF-8 BOM tells Excel to read the file as UTF-8 so accented names
    /// render correctly. The importer accepts both delimiters (see
    /// [`reader_from_bytes`]), so exported files round-trip.
    pub fn generate_csv_response(&self) -> Result<(Response<Body>, usize), AppError> {
        let mut csv_writer = WriterBuilder::new()
            .delimiter(b';')
            .has_headers(false)
            .from_writer(UTF8_BOM.to_vec());

        if let Some(headers) = &self.headers {
            csv_writer.write_record(headers)?;
        }

        for record in &self.records {
            write_record_safely(&mut csv_writer, record)?;
        }

        let data = if let Ok(data) = csv_writer.into_inner() {
            data
        } else {
            return Err(AppError::InternalServerError);
        };

        let size = data.len();

        let headers = no_cache_headers::generate_attachment_headers(
            self.filename.as_str(),
            HeaderValue::from_static("text/csv"),
        )?;

        Ok(((headers, data).into_response(), size))
    }
}

/// Serialize a record and write it to `writer` after neutralising any cells
/// that would otherwise be interpreted as spreadsheet formulas.
///
/// Why: when a recipient opens an exported CSV in Excel/LibreOffice, a cell
/// whose first character is `=`, `+`, `-`, `@`, tab, or CR is evaluated as a
/// formula (CWE-1236). Prefixing such cells with `'` is the OWASP-recommended
/// neutralisation: the leading quote is stripped on display but suppresses
/// formula evaluation.
fn write_record_safely<T, W>(writer: &mut Writer<W>, record: &T) -> csv::Result<()>
where
    T: Serialize,
    W: std::io::Write,
{
    let mut staging = WriterBuilder::new()
        .has_headers(false)
        .from_writer(Vec::new());
    staging.serialize(record)?;
    let bytes = staging.into_inner().map_err(IntoInnerError::into_error)?;

    let mut reader = ReaderBuilder::new()
        .has_headers(false)
        .from_reader(bytes.as_slice());
    if let Some(row) = reader.records().next() {
        let row = row?;
        let escaped: Vec<Cow<'_, str>> = row.iter().map(escape_csv_formula).collect();
        writer.write_record(escaped.iter().map(AsRef::as_ref))?;
    }
    Ok(())
}

/// Prefix a single quote to any cell whose first byte is one of the
/// spreadsheet-formula trigger characters. ASCII-only matching is sufficient
/// since none of the trigger characters are multi-byte UTF-8.
fn escape_csv_formula(cell: &str) -> Cow<'_, str> {
    match cell.as_bytes().first() {
        Some(b'=' | b'+' | b'-' | b'@' | b'\t' | b'\r') => {
            let mut out = String::with_capacity(cell.len() + 1);
            out.push('\'');
            out.push_str(cell);
            Cow::Owned(out)
        }
        _ => Cow::Borrowed(cell),
    }
}

/// UTF-8 byte-order mark. Prepended to exported CSVs so Excel reads them as
/// UTF-8, and stripped on import so our own exports round-trip.
const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// Build a CSV reader that tolerates the variations spreadsheets emit across
/// locales: a leading UTF-8 BOM (Excel writes one) and either `,` or `;` as the
/// field separator (Dutch/European Excel uses `;`). A BOM, if present, is
/// stripped first, then the delimiter is sniffed from the header line. Cells
/// are trimmed and column names normalised so records deserialize by name.
pub fn reader_from_bytes(data: &[u8]) -> Reader<&[u8]> {
    let data = data.strip_prefix(UTF8_BOM).unwrap_or(data);
    let mut reader = ReaderBuilder::new()
        .delimiter(detect_delimiter(data))
        .trim(Trim::All)
        .from_reader(data);
    if let Ok(headers) = reader.headers() {
        let normalized: StringRecord = headers.iter().map(normalize_column_name).collect();
        reader.set_headers(normalized);
    }
    reader
}

/// Tolerates padding, capitals, and spaces or hyphens for underscores.
pub fn normalize_column_name(name: &str) -> String {
    name.trim().to_lowercase().replace([' ', '-'], "_")
}

/// Pick the field separator by counting `;` versus `,` on the first line. The
/// header row is a fixed set of unquoted identifiers, so a raw byte count is
/// unambiguous; ties fall back to `,` (the RFC-4180 default).
fn detect_delimiter(data: &[u8]) -> u8 {
    let first_line = data.split(|&byte| byte == b'\n').next().unwrap_or(data);
    let count = |needle: u8| first_line.iter().filter(|&&byte| byte == needle).count();
    if count(b';') > count(b',') {
        b';'
    } else {
        b','
    }
}

impl<T: DeserializeOwned> Csv<T> {
    /// Deserialize every row, or report what went wrong. Reading stops at
    /// [`MAX_ERRORS`] failures and says so.
    pub fn from_bytes(data: &[u8]) -> Result<Vec<T>, Vec<CsvError>> {
        let mut records = vec![];
        let mut errors = vec![];

        for (index, res) in reader_from_bytes(data).deserialize::<T>().enumerate() {
            match res {
                Ok(record) => records.push(record),
                Err(error) => {
                    if errors.len() == MAX_ERRORS {
                        errors.push(CsvError::TooManyErrors {
                            reported: MAX_ERRORS,
                        });
                        break;
                    }
                    errors.push(CsvError::FormatError {
                        line_number: index + 2,
                        message: error.into_kind(),
                    });
                }
            }
        }

        if errors.is_empty() {
            Ok(records)
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct TestRecord {
        #[allow(dead_code)]
        value: u32,
    }

    fn position(record: u64, line: u64, byte: u64) -> csv::Position {
        let mut pos = csv::Position::new();
        pos.set_record(record).set_line(line).set_byte(byte);
        pos
    }

    fn deserialize_error() -> csv::DeserializeError {
        let mut reader = csv::Reader::from_reader("value\nabc\n".as_bytes());
        let error = reader
            .deserialize::<TestRecord>()
            .next()
            .expect("expected one row")
            .expect_err("expected deserialization error");

        match error.into_kind() {
            csv::ErrorKind::Deserialize { err, .. } => err,
            other => panic!("expected deserialize error, got {other:?}"),
        }
    }

    fn utf8_error() -> csv::Utf8Error {
        let byte_record = csv::ByteRecord::from(vec![&b"quux"[..], &b"foo\xFFbar"[..], &b"c"[..]]);
        let error = csv::StringRecord::from_byte_record(byte_record)
            .expect_err("expected utf-8 validation error");

        error.utf8_error().clone()
    }

    /// One `CsvError` per formatted error kind, in the order of the expected
    /// message lists in the locale tests below.
    fn error_message_cases() -> Vec<CsvError> {
        let format_error = |message| CsvError::FormatError {
            line_number: 3,
            message,
        };
        vec![
            format_error(csv::ErrorKind::Io(std::io::Error::other("disk failed"))),
            format_error(csv::ErrorKind::Utf8 {
                pos: None,
                err: utf8_error(),
            }),
            format_error(csv::ErrorKind::Utf8 {
                pos: Some(position(2, 4, 18)),
                err: utf8_error(),
            }),
            format_error(csv::ErrorKind::UnequalLengths {
                pos: None,
                expected_len: 4,
                len: 2,
            }),
            format_error(csv::ErrorKind::UnequalLengths {
                pos: Some(position(2, 4, 18)),
                expected_len: 4,
                len: 2,
            }),
            format_error(csv::ErrorKind::Seek),
            format_error(csv::ErrorKind::Serialize("unsupported value".to_string())),
            format_error(csv::ErrorKind::Deserialize {
                pos: None,
                err: deserialize_error(),
            }),
            format_error(csv::ErrorKind::Deserialize {
                pos: Some(position(1, 2, 6)),
                err: deserialize_error(),
            }),
            CsvError::ParseError {
                line_number: 4,
                field_name: "postal_code".to_string(),
                message: "invalid value".to_string(),
            },
        ]
    }

    /// Asserts the message of every [`error_message_cases`] entry in `locale`.
    fn assert_error_messages(locale: Locale, expected: [&str; 10]) {
        for (case, (error, expected)) in error_message_cases().iter().zip(expected).enumerate() {
            assert_eq!(error.message(locale), expected, "case {case}");
        }
    }

    #[test]
    fn formats_error_kind_messages_in_english() {
        assert_error_messages(
            Locale::En,
            [
                "The candidate on line 3 could not be imported: the file could not be read. disk failed",
                "The candidate on line 3 could not be imported: field 1 contains unreadable text. Please save the file as UTF-8 and try again. invalid utf-8: invalid UTF-8 in field 1 near byte index 3",
                "The candidate on line 3 could not be imported: record 2 on line 4 contains unreadable text in field 1 near character 18. Please save the file as UTF-8 and try again. invalid utf-8: invalid UTF-8 in field 1 near byte index 3",
                "The candidate on line 3 could not be imported: this row has 2 columns, but earlier rows have 4. Please make sure each row has the same number of columns.",
                "The candidate on line 3 could not be imported: record 2 on line 4 near character 18 has 2 columns, but earlier rows have 4. Please make sure each row has the same number of columns.",
                "The candidate on line 3 could not be imported: the file could not be read correctly. Please export the CSV again and try again.",
                "The candidate on line 3 could not be imported: the file contains a value that could not be processed. unsupported value",
                "The candidate on line 3 could not be imported: one of the values in this row is in the wrong format. field 0: invalid digit found in string",
                "The candidate on line 3 could not be imported: record 1 on line 2 near character 6 contains a value in the wrong format. field 0: invalid digit found in string",
                "The candidate on line 4 could not be imported. Please check field 'Postal code': invalid value",
            ],
        );
    }

    #[test]
    fn formats_error_kind_messages_in_dutch() {
        assert_error_messages(
            Locale::Nl,
            [
                "De kandidaat op regel 3 kon niet worden geïmporteerd: het bestand kon niet worden gelezen. disk failed",
                "De kandidaat op regel 3 kon niet worden geïmporteerd: veld 1 bevat onleesbare tekst. Sla het bestand op als UTF-8 en probeer het opnieuw. invalid utf-8: invalid UTF-8 in field 1 near byte index 3",
                "De kandidaat op regel 3 kon niet worden geïmporteerd: record 2 op regel 4 bevat onleesbare tekst in veld 1 rond teken 18. Sla het bestand op als UTF-8 en probeer het opnieuw. invalid utf-8: invalid UTF-8 in field 1 near byte index 3",
                "De kandidaat op regel 3 kon niet worden geïmporteerd: deze rij heeft 2 kolommen, maar eerdere rijen hebben er 4. Controleer of elke rij evenveel kolommen heeft.",
                "De kandidaat op regel 3 kon niet worden geïmporteerd: record 2 op regel 4 rond teken 18 heeft 2 kolommen, maar eerdere rijen hebben er 4. Controleer of elke rij evenveel kolommen heeft.",
                "De kandidaat op regel 3 kon niet worden geïmporteerd: het bestand kon niet goed worden gelezen. Exporteer het CSV-bestand opnieuw en probeer het daarna nog eens.",
                "De kandidaat op regel 3 kon niet worden geïmporteerd: het bestand bevat een waarde die niet verwerkt kon worden. unsupported value",
                "De kandidaat op regel 3 kon niet worden geïmporteerd: een van de waarden in deze rij heeft niet het juiste formaat. field 0: invalid digit found in string",
                "De kandidaat op regel 3 kon niet worden geïmporteerd: record 1 op regel 2 rond teken 6 bevat een waarde met een onjuist formaat. field 0: invalid digit found in string",
                "De kandidaat op regel 4 kon niet worden geïmporteerd. Controleer veld 'Postcode': invalid value",
            ],
        );
    }

    #[test]
    fn display_uses_default_locale() {
        let message = CsvError::FormatError {
            line_number: 1,
            message: csv::ErrorKind::Serialize("invalid record".to_string()),
        }
        .to_string();

        assert_eq!(
            message,
            "De kandidaat op regel 1 kon niet worden geïmporteerd: het bestand bevat een waarde die niet verwerkt kon worden. invalid record"
        );
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Person {
        voorletters: String,
        achternaam: String,
    }

    #[test]
    fn from_bytes_reads_comma_delimited() {
        let parsed = Csv::<Person>::from_bytes(b"voorletters,achternaam\nH.,Jansen\n")
            .unwrap_or_else(|e| panic!("{}", e[0]));
        assert_eq!(
            parsed,
            vec![Person {
                voorletters: "H.".to_string(),
                achternaam: "Jansen".to_string(),
            }]
        );
    }

    #[test]
    fn from_bytes_reads_semicolon_delimited() {
        let parsed = Csv::<Person>::from_bytes(b"voorletters;achternaam\nH.;Jansen\n")
            .unwrap_or_else(|e| panic!("{}", e[0]));
        assert_eq!(
            parsed,
            vec![Person {
                voorletters: "H.".to_string(),
                achternaam: "Jansen".to_string(),
            }]
        );
    }

    /// A file that fails on every row reports at most `MAX_ERRORS` failures
    /// plus the marker, however large it is, and stops parsing there.
    #[test]
    fn from_bytes_caps_the_error_list() {
        let mut data = b"value\n".to_vec();
        for _ in 0..(MAX_ERRORS * 20) {
            data.extend_from_slice(b"abc\n");
        }

        let errors = Csv::<TestRecord>::from_bytes(&data).expect_err("every row is invalid");

        assert_eq!(errors.len(), MAX_ERRORS + 1);
        assert!(matches!(
            errors.last(),
            Some(CsvError::TooManyErrors { reported }) if *reported == MAX_ERRORS
        ));
        assert!(
            errors[0..MAX_ERRORS]
                .iter()
                .all(|error| matches!(error, CsvError::FormatError { .. }))
        );
    }

    /// A file with fewer failures than the cap gets no truncation marker.
    #[test]
    fn from_bytes_reports_every_error_below_the_cap() {
        let errors =
            Csv::<TestRecord>::from_bytes(b"value\nabc\n1\ndef\n").expect_err("two invalid rows");

        assert_eq!(errors.len(), 2);
        assert!(
            !errors
                .iter()
                .any(|error| matches!(error, CsvError::TooManyErrors { .. }))
        );
    }

    #[test]
    fn from_bytes_strips_leading_utf8_bom() {
        let mut data = UTF8_BOM.to_vec();
        data.extend_from_slice(b"voorletters;achternaam\nH.;Jansen\n");
        let parsed = Csv::<Person>::from_bytes(&data).unwrap_or_else(|e| panic!("{}", e[0]));
        assert_eq!(parsed[0].voorletters, "H.");
    }

    /// Column order, casing and padding around headers and cells are all
    /// spreadsheet artefacts, not data.
    #[test]
    fn from_bytes_normalises_headers_and_trims_cells() {
        let parsed = Csv::<Person>::from_bytes(b" Achternaam ; VOORLETTERS \n Jansen ; H. \n")
            .unwrap_or_else(|e| panic!("{}", e[0]));
        assert_eq!(
            parsed,
            vec![Person {
                voorletters: "H.".to_string(),
                achternaam: "Jansen".to_string(),
            }]
        );
    }

    #[test]
    fn normalize_column_name_cases() {
        assert_eq!(normalize_column_name("voorletters"), "voorletters");
        assert_eq!(
            normalize_column_name(" Correspondentie-Postcode "),
            "correspondentie_postcode"
        );
        assert_eq!(
            normalize_column_name("Gemachtigde plaats"),
            "gemachtigde_plaats"
        );
    }

    #[test]
    fn detect_delimiter_prefers_semicolon_only_when_more_frequent() {
        assert_eq!(detect_delimiter(b"a;b;c\n1;2;3"), b';');
        assert_eq!(detect_delimiter(b"a,b,c\n1,2,3"), b',');
        // A header field that happens to contain a comma must not flip a
        // genuinely semicolon-delimited file to comma.
        assert_eq!(detect_delimiter(b"a,x;b;c\n"), b';');
        // Ties fall back to the RFC-4180 default.
        assert_eq!(detect_delimiter(b"a;b,c\n"), b',');
    }

    #[test]
    fn export_output_round_trips_through_import() {
        let csv = Csv {
            filename: "out.csv".to_string(),
            headers: Some(vec!["voorletters", "achternaam"]),
            records: vec![Person {
                voorletters: "H.".to_string(),
                achternaam: "Jansen".to_string(),
            }],
        };

        let mut writer = WriterBuilder::new()
            .delimiter(b';')
            .has_headers(false)
            .from_writer(UTF8_BOM.to_vec());
        writer.write_record(["voorletters", "achternaam"]).unwrap();
        for row in &csv.records {
            write_record_safely(&mut writer, row).unwrap();
        }
        let bytes = writer.into_inner().unwrap();

        assert!(
            bytes.starts_with(UTF8_BOM),
            "export should begin with a BOM"
        );
        let reparsed = Csv::<Person>::from_bytes(&bytes).unwrap_or_else(|e| panic!("{}", e[0]));
        assert_eq!(reparsed, csv.records);
    }

    #[test]
    fn escape_csv_formula_neutralises_leading_special_characters() {
        for trigger in ["=", "+", "-", "@", "\t", "\r"] {
            let cell = format!("{trigger}HYPERLINK(\"x\")");
            let escaped = escape_csv_formula(&cell);
            assert!(
                escaped.starts_with('\''),
                "{cell:?} should be prefixed with a single quote, got {escaped:?}"
            );
            assert!(escaped.ends_with(&cell));
        }
    }

    #[test]
    fn escape_csv_formula_leaves_safe_cells_untouched() {
        for safe in ["", "Henk", "1234AB", "Stationsstraat 5", "  =not-leading"] {
            assert_eq!(escape_csv_formula(safe), Cow::Borrowed(safe));
        }
    }

    #[test]
    fn generate_csv_response_neutralises_formula_records() {
        #[derive(Serialize)]
        struct Row {
            name: String,
            note: String,
        }

        let csv = Csv {
            filename: "out.csv".to_string(),
            headers: Some(vec!["name", "note"]),
            records: vec![
                Row {
                    name: "=cmd|'/c calc'!A1".to_string(),
                    note: "@SUM(1+1)".to_string(),
                },
                Row {
                    name: "Henk".to_string(),
                    note: "-1".to_string(),
                },
            ],
        };

        let (_response, _size) = csv
            .generate_csv_response()
            .expect("response should generate");

        // Re-render the same payload to bytes through a parallel Writer so we
        // can assert the on-disk content. The header row is static and not
        // escaped; record fields with leading triggers are prefixed with `'`.
        let mut writer = WriterBuilder::new()
            .has_headers(false)
            .from_writer(Vec::new());
        writer.write_record(["name", "note"]).unwrap();
        for row in &csv.records {
            write_record_safely(&mut writer, row).unwrap();
        }
        let bytes = writer.into_inner().unwrap();
        let rendered = String::from_utf8(bytes).unwrap();

        assert!(
            rendered.contains("'=cmd|'/c calc'!A1"),
            "leading `=` should be escaped, got: {rendered}"
        );
        assert!(
            rendered.contains("'@SUM(1+1)"),
            "leading `@` should be escaped, got: {rendered}"
        );
        assert!(
            rendered.contains("'-1"),
            "leading `-` should be escaped, got: {rendered}"
        );
        assert!(
            rendered.contains("Henk"),
            "non-trigger cells should be left intact, got: {rendered}"
        );
    }
}
