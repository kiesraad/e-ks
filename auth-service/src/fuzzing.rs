//! Fuzz harnesses for the SAML login pipeline, driven by the cargo-fuzz crate
//! in `fuzz/` (which sets `cfg(fuzzing)`).
//!
//! Each harness takes raw bytes and panics only on an invariant violation,
//! never on input that is merely rejected.

// Harness code, not a request path: a panic here is the finding.
#![allow(clippy::panic, clippy::expect_used, clippy::unwrap_used)]

pub(crate) mod parser_diff;
pub(crate) mod xsw;

/// roxmltree (the app's view) and uppsala (what bergshamra-dsig verifies) must
/// agree on every document both accept (parser-differential XSW, the ruby-saml
/// CVE-2025-25291/25292 pattern).
pub fn xml_parser_diff(data: &[u8]) {
    parser_diff::run(data);
}

/// Structural XSW mutations of a genuine RD-signed ArtifactResponse must never
/// make the production validation accept a non-genuine identity.
pub fn saml_xsw(data: &[u8]) {
    xsw::run(data);
}
