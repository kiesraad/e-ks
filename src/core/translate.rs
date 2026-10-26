//! Translation macro definitions backed by generated locale strings.
//! Used in rust sources with the `trans!` macro, and in templates with the `|trans` filter.
include!(concat!(env!("OUT_DIR"), "/locales.rs"));

/// Translate a key for the given locale and format with optional arguments.
#[macro_export]
macro_rules! trans {
    ($key:tt, $locale:expr $(, $args:expr)* $(,)?) => {{
        match $locale {
            $crate::Locale::En => format!($crate::translate::t_en!($key) $(, $args)*),
            $crate::Locale::Nl => format!($crate::translate::t_nl!($key) $(, $args)*),
        }
    }};
}

#[cfg(test)]
mod tests {
    use eks_locales::find_used_keys;

    use crate::translate::{LOCALE_EN, LOCALE_NL};

    #[test]
    fn test_unused_translation_keys() {
        let used_keys = find_used_keys(std::path::Path::new("./"));

        for key in LOCALE_NL.keys() {
            assert!(
                used_keys.is_used(key),
                "Translation key '{key}' (in locales/nl) is not used in any template or source file"
            );
        }

        for key in LOCALE_EN.keys() {
            assert!(
                used_keys.is_used(key),
                "Translation key '{key}' (in locales/en) is not used in any template or source file"
            );
        }

        for key in used_keys.translated {
            assert!(
                LOCALE_NL.contains_key(&key),
                "Translation key '{key}' is used in a template or source file, but missing in locales/nl"
            );

            assert!(
                LOCALE_EN.contains_key(&key),
                "Translation key '{key}' is used in a template or source file, but missing in locales/en"
            );
        }
    }

    #[test]
    fn all_documents_separated_by_non_breaking_space() {
        const FAULTY_DOCUMENTS: [&str; 7] = ["I 4", "I 1", "H 1", "H 3-1", "H 3-2", "H 4", "H 9"];
        let mut failures = Vec::new();

        for (locale_name, locale) in [("nl", &LOCALE_NL), ("en", &LOCALE_EN)] {
            for (key, value) in locale.entries() {
                for document in FAULTY_DOCUMENTS {
                    if value.contains(document) {
                        failures.push(format!(
                            " locales/{locale_name}: [{key}] contains document '{document}'"
                        ))
                    }
                }
            }
        }

        assert!(
            failures.is_empty(),
            "Translation values contain documents using non-breaking space. \
             Use \\u00A0 instead of a space in the YAML file to mark where the \
             word can not be separated:\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn test_no_long_words_without_soft_hyphen() {
        // Long words should be broken up by soft hyphens (\u00AD in YAML)
        // Soft hyphens are only rendered by the browser when it wants to split up the text
        const MAX_WORD_LENGTH: usize = 20;

        let mut failures = Vec::new();

        for (locale_name, locale) in [("nl", &LOCALE_NL), ("en", &LOCALE_EN)] {
            for (key, value) in locale.entries() {
                for word in value.split(|c: char| !c.is_alphabetic()) {
                    if word.chars().count() >= MAX_WORD_LENGTH {
                        failures.push(format!(
                            "  locales/{locale_name}: [{key}] \"{word}\" ({} chars)",
                            word.chars().count()
                        ));
                    }
                }
            }
        }

        assert!(
            failures.is_empty(),
            "Translation values contain words of {MAX_WORD_LENGTH}+ characters. \
             Use \\u00AD in the YAML file to mark where the word can be hyphenated:\n{}",
            failures.join("\n")
        );
    }
}
