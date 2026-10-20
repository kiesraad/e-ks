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

## Handling findings

A harness panics only when an invariant breaks, never on input that is
merely rejected, so every failure is a bug: in the code, or in the harness.

### 1. Reproduce and shrink

libFuzzer writes the input to `<crate>/fuzz/artifacts/<target>/`; replay it
with `bin/fuzz repro <target> <file>`, and shrink it with
`cargo +nightly fuzz tmin <target> <file>` (run from the crate's directory).
proptest shrinks on its own, prints the input and stores it under
`proptest-regressions/`; commit that file, so the case is replayed first on
every run.

### 2. Decide whose bug it is

- **The harness or its model is wrong** (an invariant that is too strict, a
  model that misreads the spec): fix the harness, with a comment saying why.
  Never loosen an invariant only to silence a failure.
- **A known, analysed difference that is not a bug** (so far only parser
  differences in `saml_xml_parser_diff`): write the analysis down in
  `<crate>/fuzz/findings/<target>/README.md`, check in the reproducers next to
  it, and allowlist the class in the harness. Strict mode
  (`AUTH_SERVICE_FUZZ_STRICT=1`) still reports it.
- **A bug in the code**: continue below.

### 3. Weigh the impact

A bug that crosses a security boundary (authentication, signature checks,
another group's data, CSRF, an XML or CSV injection) is a security issue. This
repository is public: do not push an ignored test, steering or issue that
describes it before the fix is merged. Fix it in one change that includes the
regression test, and report it as `SECURITY.md` describes if you are not on
the team. Everything else (a 500, a lost value, an export that does not import
again) can be recorded first and fixed later.

### 4. Record it

Write a regression test named after the behaviour it expects, next to the
suite that found it, marked `#[ignore = "finding: <what goes wrong>"]`, with
the shrunk input in its doc comment (`Shrunk from the fuzzer: ...`). It fails
until the bug is fixed. `bin/fuzz findings` lists the open findings, `bin/fuzz
findings run` runs them.

Then steer the generator or harness around the bug, so fuzzing keeps finding
other bugs instead of this one. Mark every steering with a comment naming the
test (`Finding, see <test name>: ...`), so it can be found again. Steer as
narrowly as possible: every input you steer away from is no longer fuzzed.

### 5. Fix it

Once the fix lands, remove the `ignore` and every steering comment that names
the test (search for the test's name), and run the affected target or suite
for a while: input that was steered away from has not been fuzzed yet. For a
libFuzzer crash, consider adding the input to `<crate>/fuzz/seeds/<target>/`.
For an analysed allowlisted class that the code now rejects, drop the
allowlist and move its README entry to a "Fixed" section.

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
| `fuzzing_tests` (auth-service) | the two SAML harnesses | Tricky but benign XML both parsers must agree on; the analysed parser differences are classified as such; a fixed sweep of XSW mutations never yields a forged identity, and the unmutated message is accepted (otherwise every rejection proves nothing). |
| `fuzzing::tests`, `<module>::fuzz::tests` | the cargo-fuzz harnesses | Every harness passes its checked-in seeds, and the WebAuthn model accepts the canonical assertion: a broken or vacuous harness shows up in `bin/fuzz props`, not as a crash on the first input or a run that checks nothing. |
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
