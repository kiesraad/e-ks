//! Shared locale tooling used by the eks build script, the eks test suite,
//! and the update_locales development binary.

mod collect_locale_files;
mod find_used_keys;
mod load_locales;
mod naive_yaml_parse;

pub use collect_locale_files::collect_locale_files;
pub use find_used_keys::{UsedKeys, find_used_keys};
pub use load_locales::load_locales;

/// Creates a fresh temporary directory for a single test.
#[cfg(test)]
pub(crate) fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("eks-locales-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}
