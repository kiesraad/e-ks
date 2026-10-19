# Fuzzing

Two kinds of fuzzers test the code that handles untrusted input:

- **cargo-fuzz targets** (libFuzzer, coverage guided) for byte-level parsers
  and verifiers: `fuzz/` for the `eks` crate, `auth-service/fuzz/` for SAML.
- **Property suites** (proptest) for structured input, where a generator of
  plausible values and a property beat random bytes.

Neither runs with `cargo test` or `bin/check`. All fuzz code is compiled only
under the `fuzzing` cfg, which cargo-fuzz sets and `bin/fuzz` passes on, so a
normal build never sees it (not even with `--all-features`).

## Running

```sh
cargo install cargo-fuzz          # once; needs a nightly toolchain
bin/fuzz list                     # targets and property suites
bin/fuzz run webauthn_assertion   # one target, 5 minutes
bin/fuzz run candidate_csv 3600   # one target, an hour
bin/fuzz all 600                  # every target, 10 minutes each
bin/fuzz props                    # every property suite, 10000 cases each
bin/fuzz props 200                # a quick pass
bin/fuzz check                    # build the targets, lint all fuzz code
```

`FUZZ_JOBS=8 bin/fuzz run <target>` fuzzes in parallel; `FUZZ_ARGS` passes
extra libFuzzer flags. The property suites build in `target/fuzz-props`, so
they do not invalidate the normal build cache.

New inputs go to `<crate>/fuzz/corpus/<target>/` (ignored by git, keep it
between runs: it is what makes the next run start where the last one ended).
`<crate>/fuzz/seeds/<target>/` holds checked-in starting inputs; regenerate
the generated ones with `bin/fuzz seeds` after changing an event or
assertion format.

## When a fuzzer fails

libFuzzer writes the input to `<crate>/fuzz/artifacts/<target>/`; replay it
with `bin/fuzz repro <target> <file>`. proptest prints the shrunk input and
stores it under `proptest-regressions/` (commit that file: the case is then
replayed first on every run).

A harness panics only when an invariant breaks, never on input that is
merely rejected, so every failure is a bug in the code or in the harness.
Record a confirmed bug as a regression test with
`#[ignore = "finding: ..."]` until it is fixed, and steer the generator or
harness around it (with a comment naming that test) so fuzzing keeps finding
other bugs. `bin/fuzz findings` lists the open findings, `bin/fuzz findings
run` runs them; once a fix lands, drop the `ignore` and the steering.

`saml_xml_parser_diff` allows the parser differences already analysed in
`auth-service/fuzz/findings/`; set `AUTH_SERVICE_FUZZ_STRICT=1` to report
those as well.

## What is fuzzed

| Target / suite | Code | Checks |
| --- | --- | --- |
| `saml_xml_parser_diff` | `auth-service/src/saml/xml_parser.rs` vs the signature backend | The app reads XML with roxmltree, `bergshamra` verifies signatures over uppsala's parse: when both accept a document, they must see the same tree (names, namespaces, attributes, IDs, text). Known, analysed differences are listed in `auth-service/fuzz/findings/` and allowed; `AUTH_SERVICE_FUZZ_STRICT=1` flags them again. |
| `saml_xsw` | `auth-service/src/handlers/acs.rs` (`claims_from_soap`) | XML signature wrapping: structural mutations of a validly signed ArtifactResponse (moved, wrapped and duplicated assertions, ID tricks, comments in the NameID, namespace shadowing, extra signatures, DOCTYPEs) run through the real validation; whatever is accepted must yield the signed identity. |
| `proxy_protocol_v2` | `src/core/proxy_protocol.rs` | No panic on any byte stream; the outcome matches the spec, decoded independently. |
| `webauthn_assertion` | `src/auth/csb_webauthn/` | Any posted JSON: only bytes the key signed are accepted. Mutated assertions: the verdict matches a model of WebAuthn §7.2 (origin, flags, challenge, signature, ...). |
| `candidate_csv` | `src/pg/candidate_lists/importer.rs`, `src/core/csv.rs` | Any upload: no panic; what imports exports without formula cells or column shifts, and imports again unchanged. |
| `store_event_decode` | `src/store/encoding.rs` | Any payload: decoding never panics and never loses data (decode, encode, decode gives the same event). |
| `store::crypto_tests` | `src/store/crypto.rs` | Wrapped stream keys only unwrap for their own stream and election; tampered or truncated ciphertext is always rejected. |
| `store::replay_tests` | `src/store/mod.rs` | Tampered event rows (flipped, moved, swapped, removed, foreign) fail the replay or yield a prefix of the real history; batched replays with live snapshots equal one full replay. |
| `pg::candidate_lists::csv_roundtrip_tests` | candidate CSV | Plausible candidates with risky text round-trip; any cell exports as one inert cell. |
| `models::markdown_tests` | `src/models/markdown.rs` (PDF templates) | Any value, in every template context (paragraph, list item, quote, heading, table cell, mono, bold), renders with the same document structure as a harmless placeholder and comes back as exactly its documented normalisation: user data never changes a document's layout. |
| `structs::common::parser_tests`, `structs::parser_tests`, `core::parser_tests`, `hearing_tests`, `correction_tests` | every validated field type (BSN, postal code, names, initials, appellation, dates, ids, omission texts, usernames, hash prefixes, ...) | No input panics; accepted values are canonical (parsing a value's own spelling gives it back); accepted text only holds characters every output (HTML, PDF, CSV, EML) can carry, within its length bounds; BSNs pass the 11-check. |
| `models::eml::eml_tests` | `src/models/eml/` | For generated valid lists, every EML document (110a, 210, 230b, 230c) is well-formed, passes eml-nl's strict reader without errors, and carries every value through unchanged. |
| `pg::candidate_lists::importer::importer_tests` | candidate CSV | Exported candidates import back unchanged; any upload is handled without a panic. |
| `app::access_rule_tests` | the whole router | Random request sequences from two groups, a group of another election, the committee (also in paper corrections), no session and a forged cookie, over every route, with own, foreign, missing and junk ids and CSRF tokens. After every step: no 5xx, no other group's data in any response (bodies, headers, unzipped downloads), writes only in the sender's own streams, no 2xx outside the sender's area, and a POST without its session's CSRF token changes nothing. |

## Adding a fuzzer

- **cargo-fuzz target**: write `pub fn <name>(data: &[u8])` in a `fuzz.rs`
  child module of the code under test (declared `#[cfg(fuzzing)]`), re-export
  it from `src/fuzzing.rs`, and add `fuzz/fuzz_targets/<name>.rs` plus a
  `[[bin]]` entry in `fuzz/Cargo.toml`.
- **Property suite**: a `*_tests.rs` module declared under
  `#[cfg(all(test, fuzzing))]`; `bin/fuzz props` finds it by that attribute.
  Use `crate::test_utils::proptest_config(n)` so `bin/fuzz props <cases>`
  can scale it.
