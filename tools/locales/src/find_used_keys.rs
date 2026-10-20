use std::{collections::BTreeSet, ffi::OsStr};

/// Recursively collects files with the given extension under `dir`.
fn collect_files_recursively(
    dir: &std::path::Path,
    extension: &str,
    files: &mut Vec<std::path::PathBuf>,
) {
    let entries = std::fs::read_dir(dir).expect("Failed to read source directory");
    for entry in entries {
        let entry = entry.expect("Failed to read source directory entry");
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursively(&path, extension, files);
        } else if path.extension().and_then(OsStr::to_str) == Some(extension) {
            files.push(path);
        }
    }
}

/// Adds the first capture group of every `re` match in `files` to `keys`.
fn collect_captured_keys(
    keys: &mut BTreeSet<String>,
    re: &regex::Regex,
    files: &[std::path::PathBuf],
) {
    for file in files {
        let haystack = std::fs::read_to_string(file).expect("Failed to read source file");

        for capture in re.captures_iter(&haystack) {
            if let Some(key) = capture.get(1) {
                keys.insert(key.as_str().to_string());
            }
        }
    }
}

/// Translation keys referenced by templates and Rust sources.
#[derive(Debug, Default)]
pub struct UsedKeys {
    /// Keys passed to `|trans` or `trans!`, which must exist in the locales.
    pub translated: BTreeSet<String>,
    /// Key-shaped string literals in Rust sources, such as keys returned by a
    /// function and translated in a template. These keep an existing key in
    /// use, but are not necessarily keys.
    pub literals: BTreeSet<String>,
}

impl UsedKeys {
    pub fn is_used(&self, key: &str) -> bool {
        self.translated.contains(key) || self.literals.contains(key)
    }
}

/// Finds translation keys used in templates and Rust sources under `path`.
///
/// Keys are collected using simple regex scans.
pub fn find_used_keys(path: &std::path::Path) -> UsedKeys {
    let mut used_keys = UsedKeys::default();

    let template_re = regex::Regex::new(r#""([\w\.]+)"\|trans"#).expect("Invalid template regex");
    let mut template_files = Vec::new();
    // Scan every template root listed in `askama.toml` (`src/pg` and `src/csb`).
    for templates_dir in ["pg", "csb"] {
        let templates_dir = path.join("src").join(templates_dir);
        collect_files_recursively(&templates_dir, "html", &mut template_files);
    }
    collect_captured_keys(&mut used_keys.translated, &template_re, &template_files);

    let source_re =
        regex::Regex::new(r#"trans!\s*\(\s*"([\w\.]+)""#).expect("Invalid source regex");
    let literal_re = regex::Regex::new(r#""(\w+(?:\.\w+)+)""#).expect("Invalid literal regex");
    let mut source_files = Vec::new();
    collect_files_recursively(&path.join("src"), "rs", &mut source_files);
    collect_captured_keys(&mut used_keys.translated, &source_re, &source_files);
    collect_captured_keys(&mut used_keys.literals, &literal_re, &source_files);

    used_keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::temp_dir;

    fn write(path: std::path::PathBuf, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).expect("create dir");
        std::fs::write(path, contents).expect("write file");
    }

    #[test]
    fn finds_translated_keys_and_key_literals() {
        let dir = temp_dir();
        write(
            dir.join("src/csb/page.html"),
            r#"{{ "csb.title"|trans }} {{ item.label_key()|trans }}"#,
        );
        write(dir.join("src/pg/page.html"), r#"{{ "common.save"|trans }}"#);
        write(
            dir.join("src/kind.rs"),
            r#"
            let title = trans!("csb.heading", locale);
            fn label_key() -> &'static str { "csb.kind.added" }
            let file = "index.html";
            let word = "plain";
            "#,
        );

        let used_keys = find_used_keys(&dir);

        assert_eq!(
            used_keys.translated,
            BTreeSet::from(["common.save", "csb.heading", "csb.title"].map(String::from))
        );
        assert_eq!(
            used_keys.literals,
            BTreeSet::from(["csb.heading", "csb.kind.added", "index.html"].map(String::from))
        );
        assert!(used_keys.is_used("csb.kind.added"));
        assert!(!used_keys.is_used("plain"));
    }
}
