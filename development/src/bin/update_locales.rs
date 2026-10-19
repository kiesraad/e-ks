use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, HashSet},
    ffi::OsStr,
    path::Path,
};

use anyhow::{Context, Result};
use eks_locales::{UsedKeys, collect_locale_files, find_used_keys};
use saphyr::{LoadableYamlNode, Mapping, Scalar, Yaml, YamlEmitter};

/// Escape text like `\u00AD` must survive the YAML round-trip verbatim (see
/// load_locales.rs). The `\u` prefix is masked with a private-use character
/// before parsing and restored after emitting.
const ESCAPE_PREFIX: &str = "\\u";
const ESCAPE_MASK: char = '\u{E042}';

/// In-memory representation of locale YAML structures.
#[derive(Debug, Clone)]
enum LocaleNode {
    Map(BTreeMap<String, LocaleNode>),
    String(String),
}

/// Parse YAML into a structured tree while enforcing string-only leaves.
fn yaml_to_node(yaml: &Yaml, file: &Path, path: &str) -> Result<LocaleNode> {
    let display_path = if path.is_empty() { "<root>" } else { path };

    match yaml {
        Yaml::Mapping(mapping) => {
            let mut map = BTreeMap::new();

            for (key, value) in mapping {
                let key = key.as_str().with_context(|| {
                    format!("non-string key in {} at {}", file.display(), display_path)
                })?;

                let child_path = if path.is_empty() {
                    key.to_string()
                } else {
                    format!("{path}.{key}")
                };

                let node = yaml_to_node(value, file, &child_path)?;
                map.insert(key.to_string(), node);
            }

            Ok(LocaleNode::Map(map))
        }
        Yaml::Sequence(_) => anyhow::bail!(
            "arrays are not allowed in {} at {}",
            file.display(),
            display_path
        ),
        Yaml::Value(_) => {
            if let Some(value) = yaml.as_str() {
                Ok(LocaleNode::String(value.to_string()))
            } else {
                anyhow::bail!("non-string value in {} at {}", file.display(), display_path)
            }
        }
        Yaml::Tagged(_, inner) => yaml_to_node(inner, file, path),
        Yaml::Alias(_) | Yaml::BadValue | Yaml::Representation(_, _, _) => {
            anyhow::bail!(
                "unsupported value in {} at {}",
                file.display(),
                display_path
            )
        }
    }
}

/// Prune any keys that are not present in the used set.
fn retain_used(node: &mut LocaleNode, used: &HashSet<String>, prefix: &str) -> bool {
    match node {
        LocaleNode::String(_) => used.contains(prefix),
        LocaleNode::Map(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();

            for key in keys {
                let child_prefix = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };

                let keep = map
                    .get_mut(&key)
                    .map(|child| retain_used(child, used, &child_prefix))
                    .unwrap_or(false);

                if !keep {
                    map.remove(&key);
                }
            }

            !map.is_empty()
        }
    }
}

fn insert_leaf(
    map: &mut BTreeMap<String, LocaleNode>,
    key: &str,
    full_path: &str,
    file: &Path,
) -> Result<()> {
    match map.entry(key.to_string()) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(LocaleNode::String(key.to_string()));
            Ok(())
        }
        std::collections::btree_map::Entry::Occupied(entry) => {
            if matches!(entry.get(), LocaleNode::Map(_)) {
                anyhow::bail!("expected string for {} in {}", full_path, file.display());
            }
            Ok(())
        }
    }
}

fn descend_into<'a>(
    map: &'a mut BTreeMap<String, LocaleNode>,
    key: &str,
    full_path: &str,
    file: &Path,
) -> Result<&'a mut LocaleNode> {
    let entry = map
        .entry(key.to_string())
        .or_insert_with(|| LocaleNode::Map(BTreeMap::new()));
    if matches!(entry, LocaleNode::String(_)) {
        anyhow::bail!("expected mapping for {} in {}", full_path, file.display());
    }
    Ok(entry)
}

fn insert_segments(
    node: &mut LocaleNode,
    segments: &[&str],
    full_path: &str,
    file: &Path,
) -> Result<()> {
    let LocaleNode::Map(map) = node else {
        anyhow::bail!("expected mapping for {} in {}", full_path, file.display());
    };

    let key = segments[0];
    if segments.len() == 1 {
        return insert_leaf(map, key, full_path, file);
    }

    let entry = descend_into(map, key, full_path, file)?;
    insert_segments(entry, &segments[1..], full_path, file)
}

/// Ensure the full key path exists, inserting the key value for new leaf values.
fn insert_used_path(node: &mut LocaleNode, path: &str, file: &Path) -> Result<()> {
    if path.is_empty() {
        return Ok(());
    }

    let segments: Vec<&str> = path.split('.').collect();
    insert_segments(node, &segments, path, file)
}

/// Convert a LocaleNode tree back into a YAML value.
fn node_to_yaml(node: &LocaleNode) -> Yaml<'static> {
    match node {
        LocaleNode::String(value) => Yaml::Value(Scalar::String(Cow::Owned(value.clone()))),
        LocaleNode::Map(map) => {
            let mut mapping: Mapping = Mapping::new();

            for (key, value) in map {
                let key_yaml = Yaml::Value(Scalar::String(Cow::Owned(key.clone())));
                let value_yaml = node_to_yaml(value);
                mapping.insert(key_yaml, value_yaml);
            }

            Yaml::Mapping(mapping)
        }
    }
}

/// Collect leaf paths from a locale tree so we can diff against used keys.
fn collect_leaf_paths(node: &LocaleNode) -> BTreeSet<String> {
    fn walk(node: &LocaleNode, prefix: &str, out: &mut BTreeSet<String>) {
        match node {
            LocaleNode::String(_) => {
                if !prefix.is_empty() {
                    out.insert(prefix.to_string());
                }
            }
            LocaleNode::Map(map) => {
                for (key, value) in map {
                    let child_prefix = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    walk(value, &child_prefix, out);
                }
            }
        }
    }

    let mut out = BTreeSet::new();
    walk(node, "", &mut out);
    out
}

#[derive(Default)]
struct LocaleStats {
    files_processed: usize,
    files_changed: usize,
    total_added: usize,
    total_removed: usize,
}

fn parse_locale_root(file: &Path) -> Result<LocaleNode> {
    let input = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read locale file {}", file.display()))?;

    if input.contains(ESCAPE_MASK) {
        anyhow::bail!(
            "locale file {} contains reserved character U+E042",
            file.display()
        );
    }

    let input = input.replace(ESCAPE_PREFIX, ESCAPE_MASK.to_string().as_str());

    let docs = Yaml::load_from_str(&input)
        .with_context(|| format!("failed to parse YAML in {}", file.display()))?;

    if docs.len() != 1 {
        anyhow::bail!(
            "expected exactly one YAML document in {}, found {}",
            file.display(),
            docs.len()
        );
    }

    let node = yaml_to_node(&docs[0], file, "")?;
    match node {
        LocaleNode::Map(_) => Ok(node),
        LocaleNode::String(_) => {
            anyhow::bail!("expected mapping at root of {}", file.display());
        }
    }
}

/// The used keys of this file: every translated key, plus the literals that
/// already are a key. Other literals are not keys, so they are never added.
fn used_keys_for_file(
    used_keys: &UsedKeys,
    existing: &BTreeSet<String>,
    basename: &str,
) -> HashSet<String> {
    let prefix = format!("{basename}.");
    let strip = |key: &String| key.strip_prefix(&prefix).map(str::to_string);
    let translated = used_keys.translated.iter().filter_map(strip);
    let literals = used_keys
        .literals
        .iter()
        .filter_map(strip)
        .filter(|key| existing.contains(key));
    translated.chain(literals).collect()
}

fn diff_key_sets(
    existing: &BTreeSet<String>,
    used: &HashSet<String>,
) -> (BTreeSet<String>, BTreeSet<String>) {
    let added: BTreeSet<String> = used
        .iter()
        .filter(|k| !existing.contains(*k))
        .cloned()
        .collect();
    let removed: BTreeSet<String> = existing
        .iter()
        .filter(|k| !used.contains(*k))
        .cloned()
        .collect();
    (added, removed)
}

fn print_key_changes(action: &str, keys: &BTreeSet<String>, basename: &str, file: &Path) {
    for key in keys {
        let full_key = if key.is_empty() {
            basename.to_string()
        } else {
            format!("{basename}.{key}")
        };
        println!("{action} {} ({})", full_key, file.display());
    }
}

fn write_locale_file(node: &LocaleNode, file: &Path) -> Result<()> {
    let yaml_out = node_to_yaml(node);
    let mut output = String::new();
    YamlEmitter::new(&mut output)
        .dump(&yaml_out)
        .with_context(|| format!("failed to emit YAML for {}", file.display()))?;

    let output = output.replace(ESCAPE_MASK, ESCAPE_PREFIX);
    let mut output = output.strip_prefix("---\n").unwrap_or(&output).to_string();
    output.push('\n');

    std::fs::write(file, output)
        .with_context(|| format!("failed to write locale file {}", file.display()))
}

fn process_locale_file(file: &Path, used_keys: &UsedKeys, stats: &mut LocaleStats) -> Result<()> {
    stats.files_processed += 1;
    let basename = file
        .file_stem()
        .and_then(OsStr::to_str)
        .context("Failed to get locale file stem")?;

    let mut node = parse_locale_root(file)?;

    let existing_keys = collect_leaf_paths(&node);
    let used_set = used_keys_for_file(used_keys, &existing_keys, basename);
    let (added_keys, removed_keys) = diff_key_sets(&existing_keys, &used_set);

    if !added_keys.is_empty() || !removed_keys.is_empty() {
        stats.files_changed += 1;
    }

    print_key_changes("remove", &removed_keys, basename, file);
    print_key_changes("add", &added_keys, basename, file);

    stats.total_added += added_keys.len();
    stats.total_removed += removed_keys.len();

    retain_used(&mut node, &used_set, "");

    for used_key in &used_set {
        insert_used_path(&mut node, used_key, file)?;
    }

    write_locale_file(&node, file)
}

fn main() -> Result<()> {
    let used_keys = find_used_keys(Path::new("."));
    let mut stats = LocaleStats::default();

    for lang in &["en", "nl"] {
        let locale_dir = Path::new("locales").join(lang);
        for file in collect_locale_files(&locale_dir) {
            process_locale_file(&file, &used_keys, &mut stats)?;
        }
    }

    println!(
        "Finished processing locale files: processed {} file(s), {} changed, {} added, {} removed.",
        stats.files_processed, stats.files_changed, stats.total_added, stats.total_removed
    );

    Ok(())
}
