use std::path::Path;

use crate::{collect_locale_files::collect_locale_files, naive_yaml_parse::naive_yaml_parse};

const SOFT_HYPHEN: &str = "\\u00AD";
const NON_BREAKING_SPACE: &str = "\\u00A0";

/// Renders a locale value as a Rust literal expression.
///
/// Soft hyphen escapes cannot appear inside a raw string, so values containing
/// them are split into a `concat!` of raw parts joined by the real character.
fn value_literal(value: &str) -> String {
    let value = value.replace(NON_BREAKING_SPACE, "\u{A0}");
    if value.contains(SOFT_HYPHEN) {
        let parts: Vec<String> = value
            .split(SOFT_HYPHEN)
            .map(|part| format!("r###\"{part}\"###"))
            .collect();
        format!("concat!({})", parts.join(r#", "\u{AD}", "#))
    } else {
        format!("r###\"{value}\"###")
    }
}

/// Generates `locales.rs` in `out_dir` from the locale trees under
/// `locales_root`, containing translation macros and PHF maps.
///
/// The output includes data for the hard-coded language list and emits
/// `cargo:rerun-if-changed` for each locale file.
pub fn load_locales(out_dir: &Path, locales_root: &Path) {
    let mut output = String::new();

    for lang in &["en", "nl"] {
        let mut map: phf_codegen::Map<String> = phf_codegen::Map::new();
        let locale_dir = locales_root.join(lang);
        println!("cargo:rerun-if-changed={}", locale_dir.display());
        let locale_files = collect_locale_files(&locale_dir);

        output.push_str(&format!(
            "/// Translate a literal key to a raw localized string for `{lang}`.\n#[macro_export]\nmacro_rules! inner_t_{lang} {{\n\n"
        ));

        for locale_path in locale_files {
            println!("cargo:rerun-if-changed={}", locale_path.display());
            let yaml = std::fs::read_to_string(&locale_path).expect("Failed to read locale file");
            let key = locale_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .expect("Locale file name is not valid UTF-8");
            let entries = naive_yaml_parse(key, &yaml);

            for (key, value) in entries {
                let value = value_literal(&value);
                output.push_str(&format!("    (\"{key}\") => {{ {value} }};\n"));
                map.entry(key, value);
            }
        }

        output.push_str(&format!(
            "($other:literal) => {{
                concat!(\"[\", $other, \"]\")
            }};\n}}\npub use inner_t_{lang} as t_{lang};\n\n"
        ));

        output.push_str(&format!(
            "pub static LOCALE_{}: phf::Map<&'static str, &'static str> = {};\n",
            lang.to_uppercase(),
            map.build()
        ));
    }

    let path = out_dir.join("locales.rs");
    std::fs::write(&path, output).expect("Failed to write locales.rs");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::temp_dir;

    /// Builds a `locales_root` with an `en` and `nl` tree and returns the
    /// generated `locales.rs` contents.
    fn generate(en: &str, nl: &str) -> String {
        let dir = temp_dir();
        let locales_root = dir.join("locales");

        for (lang, yaml) in [("en", en), ("nl", nl)] {
            let lang_dir = locales_root.join(lang);
            std::fs::create_dir_all(&lang_dir).expect("create locale dir");
            std::fs::write(lang_dir.join("messages.yml"), yaml).expect("write locale file");
        }

        load_locales(&dir, &locales_root);

        std::fs::read_to_string(dir.join("locales.rs")).expect("read generated locales")
    }

    #[test]
    fn value_literal_wraps_plain_values() {
        assert_eq!(value_literal("Hello"), "r###\"Hello\"###");
    }

    #[test]
    fn value_literal_splits_soft_hyphens() {
        assert_eq!(
            value_literal("stem\\u00ADbureau"),
            "concat!(r###\"stem\"###, \"\\u{AD}\", r###\"bureau\"###)"
        );
    }

    #[test]
    fn load_locales_emits_macros_and_maps_per_language() {
        let output = generate("greeting: \"Hello\"\n", "greeting: \"Hallo\"\n");

        assert!(output.contains("macro_rules! inner_t_en"));
        assert!(output.contains("macro_rules! inner_t_nl"));
        assert!(output.contains("pub use inner_t_en as t_en;"));
        assert!(output.contains("pub use inner_t_nl as t_nl;"));
        assert!(output.contains("(\"messages.greeting\") => { r###\"Hello\"### };"));
        assert!(output.contains("(\"messages.greeting\") => { r###\"Hallo\"### };"));
        assert!(output.contains("pub static LOCALE_EN: phf::Map<&'static str, &'static str>"));
        assert!(output.contains("pub static LOCALE_NL: phf::Map<&'static str, &'static str>"));
    }

    #[test]
    fn load_locales_emits_fallback_arm() {
        let output = generate("greeting: \"Hello\"\n", "greeting: \"Hallo\"\n");

        assert!(output.contains("($other:literal) =>"));
        assert!(output.contains("concat!(\"[\", $other, \"]\")"));
    }

    #[test]
    fn load_locales_keeps_soft_hyphen_escapes_out_of_raw_strings() {
        let output = generate(
            "word: \"stem\\u00ADbureau\"\n",
            "word: \"stem\\u00ADbureau\"\n",
        );

        assert!(output.contains("concat!(r###\"stem\"###, \"\\u{AD}\", r###\"bureau\"###)"));
        assert!(!output.contains("r###\"stem\\u00ADbureau\"###"));
    }

    #[test]
    fn load_locales_uses_the_file_stem_as_key_prefix() {
        let dir = temp_dir();
        let locales_root = dir.join("locales");

        for lang in ["en", "nl"] {
            let lang_dir = locales_root.join(lang);
            std::fs::create_dir_all(&lang_dir).expect("create locale dir");
            std::fs::write(lang_dir.join("audit_log.yml"), "title: \"Log\"\n")
                .expect("write locale file");
        }

        load_locales(&dir, &locales_root);
        let output = std::fs::read_to_string(dir.join("locales.rs")).expect("read generated");

        assert!(output.contains("(\"audit_log.title\")"));
    }
}
