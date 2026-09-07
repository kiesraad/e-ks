//! Askama wiring for the Markdown dialect templates the PDF models are
//! written in (`src/models/templates/`, parsed by
//! [`textris_pdf::markdown::parse`]).
//!
//! Interpolated data must never be able to change the document structure, so
//! every `{{ }}` value passes through the crate's escape functions:
//! [`MarkdownEscaper`] auto-escapes flow contexts (registered for the `.md`
//! extension in `askama.toml`)

use askama::filters::Safe;
use chrono::NaiveDate;
use textris_pdf::markdown;

use crate::core::constants::DEFAULT_DATE_FORMAT;

/// The `.md` auto-escaper: [`markdown::escape`] for flow contexts
/// (paragraphs, headings, list items, quotes).
#[derive(Debug, Clone, Copy)]
pub struct MarkdownEscaper;

impl askama::filters::Escaper for MarkdownEscaper {
    fn write_escaped_str<W: std::fmt::Write>(&self, mut dest: W, string: &str) -> std::fmt::Result {
        dest.write_str(&markdown::escape(string))
    }
}

/// Filters for the escaping contexts the flow auto-escaper does not cover,
/// plus the shared `display` filter for optional values.
pub mod filters {
    use chrono::Datelike;

    use super::{DEFAULT_DATE_FORMAT, NaiveDate, Safe, markdown};

    pub use crate::filters::display;

    /// Escape a value interpolated into a table cell.
    #[askama::filter_fn]
    pub fn cell<T: std::fmt::Display>(
        value: T,
        _: &dyn askama::Values,
    ) -> askama::Result<Safe<String>> {
        Ok(Safe(markdown::escape_cell(&value.to_string())))
    }

    /// Escape a value interpolated into a single-line context (a heading):
    /// newlines fold to a space, as the enclosing block ends at the line end.
    #[askama::filter_fn]
    pub fn line<T: std::fmt::Display>(
        value: T,
        _: &dyn askama::Values,
    ) -> askama::Result<Safe<String>> {
        let single_line = value.to_string().replace(['\r', '\n'], " ");
        Ok(Safe(markdown::escape(&single_line)))
    }

    /// Wrap a value in a verbatim (mono) span inside a table cell.
    #[askama::filter_fn]
    pub fn mono_cell<T: std::fmt::Display>(
        value: T,
        _: &dyn askama::Values,
    ) -> askama::Result<Safe<String>> {
        Ok(Safe(markdown::mono_cell(&value.to_string())))
    }

    /// Format a date in the model date format (`31-12-2027`).
    #[askama::filter_fn]
    pub fn date(value: &NaiveDate, _: &dyn askama::Values) -> askama::Result<String> {
        Ok(value.format(DEFAULT_DATE_FORMAT).to_string())
    }

    /// Format a date as a letter writes it, in Dutch: `16 september 2025`.
    #[askama::filter_fn]
    pub fn long_date(value: &NaiveDate, _: &dyn askama::Values) -> askama::Result<String> {
        const MONTHS: [&str; 12] = [
            "januari",
            "februari",
            "maart",
            "april",
            "mei",
            "juni",
            "juli",
            "augustus",
            "september",
            "oktober",
            "november",
            "december",
        ];
        Ok(format!(
            "{} {} {}",
            value.day(),
            MONTHS[value.month0() as usize],
            value.year()
        ))
    }

    /// An optional count as printed in a table: the value, or a dash when
    /// there is none.
    #[askama::filter_fn]
    pub fn or_dash<T: std::fmt::Display>(
        value: &Option<T>,
        _: &dyn askama::Values,
    ) -> askama::Result<String> {
        Ok(value
            .as_ref()
            .map_or_else(|| "-".to_string(), ToString::to_string))
    }

    /// Uppercase letter numbering for list labels: 1 → `A`, 26 → `Z`,
    /// 27 → `AA`, …
    #[askama::filter_fn]
    pub fn upper_alpha(index: &usize, _: &dyn askama::Values) -> askama::Result<String> {
        let mut n = *index;
        let mut out = Vec::new();
        while n > 0 {
            n -= 1;
            out.push(b'A' + (n % 26) as u8);
            n /= 26;
        }
        out.reverse();
        Ok(String::from_utf8(out).expect("ascii letters"))
    }
}

/// Bind a PDF model to one Markdown template (one per locale and variant): an
/// askama wrapper that derefs to the model, so the template reads its fields
/// and methods directly.
macro_rules! model_template {
    ($wrapper:ident, $model:ident, $path:literal) => {
        #[derive(askama::Template)]
        #[template(path = $path)]
        struct $wrapper<'a>(&'a $model);

        impl std::ops::Deref for $wrapper<'_> {
            type Target = $model;

            fn deref(&self) -> &Self::Target {
                self.0
            }
        }
    };
}
pub(super) use model_template;

#[cfg(test)]
mod tests {
    use super::filters;

    fn upper_alpha(index: usize) -> String {
        filters::upper_alpha::default()
            .execute(&index, askama::NO_VALUES)
            .unwrap()
    }

    fn line(value: &str) -> String {
        filters::line::default()
            .execute(value, askama::NO_VALUES)
            .unwrap()
            .0
    }

    fn long_date(year: i32, month: u32, day: u32) -> String {
        let date = chrono::NaiveDate::from_ymd_opt(year, month, day).unwrap();
        filters::long_date::default()
            .execute(&date, askama::NO_VALUES)
            .unwrap()
    }

    fn or_dash(value: Option<usize>) -> String {
        filters::or_dash::default()
            .execute(&value, askama::NO_VALUES)
            .unwrap()
    }

    #[test]
    fn long_date_writes_the_month_out_in_dutch() {
        assert_eq!(long_date(2025, 9, 16), "16 september 2025");
        // No zero padding on the day.
        assert_eq!(long_date(2027, 1, 1), "1 januari 2027");
        assert_eq!(long_date(2027, 12, 31), "31 december 2027");
    }

    #[test]
    fn or_dash_prints_a_dash_for_a_missing_count() {
        assert_eq!(or_dash(Some(30)), "30");
        assert_eq!(or_dash(Some(0)), "0");
        assert_eq!(or_dash(None), "-");
    }

    #[test]
    fn line_folds_newlines_to_spaces() {
        assert_eq!(line("a\nb"), "a b");
        assert_eq!(line("a\r\nb"), "a  b");
        assert_eq!(line("a\n\nb"), "a  b");
        // Trailing whitespace still folds away.
        assert_eq!(line("a\n"), "a");
    }

    #[test]
    fn line_escapes_punctuation() {
        assert_eq!(line("a # b"), "a \\# b");
        assert_eq!(line("x|y"), "x\\|y");
    }

    #[test]
    fn upper_alpha_single_letters() {
        assert_eq!(upper_alpha(1), "A");
        assert_eq!(upper_alpha(2), "B");
        assert_eq!(upper_alpha(26), "Z");
    }

    #[test]
    fn upper_alpha_double_letters() {
        assert_eq!(upper_alpha(27), "AA");
        assert_eq!(upper_alpha(28), "AB");
        assert_eq!(upper_alpha(52), "AZ");
        assert_eq!(upper_alpha(53), "BA");
        assert_eq!(upper_alpha(702), "ZZ");
    }

    #[test]
    fn upper_alpha_triple_letters() {
        assert_eq!(upper_alpha(703), "AAA");
        assert_eq!(upper_alpha(704), "AAB");
    }

    #[test]
    fn upper_alpha_zero_is_empty() {
        assert_eq!(upper_alpha(0), "");
    }
}
