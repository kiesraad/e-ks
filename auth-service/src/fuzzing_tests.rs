//! Hand-written regression tests for the fuzz harnesses. Compiled only under
//! `cfg(all(test, fuzzing))`, so plain `cargo test` never builds them; run with
//! `RUSTFLAGS="--cfg fuzzing" cargo test -p auth-service fuzzing`.

use crate::fuzzing::{
    parser_diff::{self, ID_WHITESPACE, LEADING_COLON, Verdict, compare, known_finding},
    saml_xsw, xml_parser_diff, xsw,
};

// ---------------------------------------------------------------------------
// parser_diff: the two parsers must agree on these tricky-but-benign inputs.
// ---------------------------------------------------------------------------

/// Inputs both parsers must lower to the same canonical tree. Each probes one of
/// the normalisations the harness folds in (comments, CDATA, refs, namespace
/// redeclaration, xml:id, BOM, CRLF, attribute whitespace, DOCTYPE position).
const AGREE: &[&str] = &[
    "<r>a<!--c-->b</r>", // comment inside text
    "<r>pre<!--x-->post</r>",
    "<r>a<![CDATA[x]]>b</r>",                            // CDATA
    "<r><![CDATA[a<b&c]]></r>",                          // CDATA with markup chars
    "<r>&amp;&lt;&gt;&quot;&apos;</r>",                  // predefined entities
    "<r>&#65;&#x42;&#0067;</r>",                         // char refs (dec/hex/padded)
    "<r a=\"&#65;\"/>",                                  // char ref in attribute
    "<r a=\"x y\tz\nw\"/>",                              // attribute whitespace norm
    "<r a=\"&#9;&#10;&#13;\"/>",                         // referenced whitespace kept
    "\u{feff}<r/>",                                      // BOM
    "<r>\u{feff}x</r>",                                  // in-content BOM char
    "<r>a\r\nb\rc\nd</r>",                               // CRLF / CR in text
    "<r><![CDATA[a\r\nb\rc]]></r>",                      // CRLF / CR in CDATA
    "<r xmlns=\"urn:a\"><s xmlns=\"\"/></r>",            // default-ns redeclaration
    "<r xmlns:p=\"urn:a\"><p:s xmlns:p=\"urn:b\"/></r>", // prefix redeclaration
    "<r xmlns=\"\"/>",                                   // empty default namespace
    "<r xml:id=\"a\" ID=\"b\" AssertionID=\"c\"/>",      // multiple ID attributes
    "<?xml version=\"1.0\"?><!--c--><r><?pi d?></r>",    // prolog comment + PI
    "<r xml:lang=\"nl\">x</r>",                          // reserved xml: prefix
    "<samlp:Response xmlns:samlp=\"urn:oasis:names:tc:SAML:2.0:protocol\" ID=\"_r\"><samlp:Status/></samlp:Response>",
];

#[test]
fn tricky_inputs_agree() {
    for xml in AGREE {
        let v = compare(xml);
        assert_eq!(
            v,
            Verdict::Agree,
            "{xml:?}: expected the parsers to agree, got {v:?}"
        );
    }
}

/// A DOCTYPE is an acceptance asymmetry, not a divergence: roxmltree (so the
/// app and signature-verification path) rejects DTDs outright, while uppsala
/// parses them. Production rejects at the first roxmltree parse, so uppsala
/// accepting more is safe. An internal entity subset is the same.
#[test]
fn doctype_is_a_safe_acceptance_asymmetry() {
    for xml in [
        "<!DOCTYPE r><r/>",
        "<!DOCTYPE r [<!ENTITY x \"y\">]><r>&x;</r>",
        "<!DOCTYPE r SYSTEM \"r.dtd\"><r/>",
    ] {
        assert_eq!(
            compare(xml),
            Verdict::OnlyUppsala,
            "{xml:?}: DOCTYPE must be an uppsala-only acceptance"
        );
    }
}

/// The harness never panics on inputs where a parser merely rejects, and does
/// not panic on the agreeing inputs either.
#[test]
fn harness_tolerates_rejected_and_agreeing_inputs() {
    let benign = [
        "",
        "not xml",
        "<r>",
        "<r></s>",
        "<r a=\"<\"/>",
        "<!DOCTYPE r [<!ENTITY x \"y\">]><r>&x;</r>",
    ];
    for xml in AGREE.iter().chain(benign.iter()) {
        xml_parser_diff(xml.as_bytes());
    }
    // Non-UTF-8 is skipped, not a panic.
    xml_parser_diff(&[0xff, 0xfe, 0x00]);
}

// ---------------------------------------------------------------------------
// parser_diff: the one divergence class the harness legitimately flags.
// ---------------------------------------------------------------------------

/// A leading colon is an invalid QName the two parsers read differently
/// (roxmltree drops the empty prefix; uppsala keeps `:name`/`:xmlns`). Each
/// manifestation is reported, not normalised, and classified as one finding;
/// `saml_xsw` (below) shows it yields no bypass.
#[test]
fn leading_colon_is_flagged_as_the_known_finding() {
    for xml in [
        "<:r/>",                         // element name
        "<r xmlns=\"urn:a\"><:s/></r>",  // nested element name
        "<r :a=\"1\"/>",                 // attribute name
        "<r :xmlns=\"urn:a\"/>",         // :xmlns read as a default-ns decl
        "<r :xmlns=\"urn:a\"><:s/></r>", // namespace + element name
    ] {
        match compare(xml) {
            Verdict::Diverge(d) => assert_eq!(
                known_finding(&d),
                Some(LEADING_COLON),
                "{xml:?}: unexpected divergence {d}"
            ),
            other => panic!("{xml:?}: expected a flagged divergence, got {other:?}"),
        }
    }
}

/// An ID attribute whose two reads differ only by whitespace normalisation is
/// classified as the id-whitespace finding (ID values are compared raw because
/// they resolve references; the difference is digest-bound, see the finding doc).
#[test]
fn id_whitespace_is_classified() {
    // A literal TAB in an ID: roxmltree -> space, uppsala keeps the TAB.
    let xml = "<r xmlns=\"urn:a\" ID=\"a\tb\"/>";
    match compare(xml) {
        Verdict::Diverge(d) => assert_eq!(known_finding(&d), Some(ID_WHITESPACE), "{d}"),
        other => panic!("{xml:?}: expected an id-whitespace divergence, got {other:?}"),
    }
}

/// By default the harness allowlists known, analysed classes (their reproducers
/// are checked in) so an ongoing fuzz run hunts for NEW divergences; strict mode
/// panics on everything. A genuinely new divergence always panics.
#[test]
fn known_classes_allowlisted_by_default_strict_panics() {
    use parser_diff::{Difference, compare, must_report};

    let known = match compare("<:r/>") {
        Verdict::Diverge(d) => *d,
        other => panic!("expected a divergence, got {other:?}"),
    };
    assert!(
        !must_report(&known, false),
        "a known class must not panic by default"
    );
    assert!(
        must_report(&known, true),
        "strict mode must panic even on a known class"
    );
    // A hypothetical unclassified divergence always reports.
    let new = Difference {
        path: "/0".to_string(),
        kind: parser_diff::DiffKind::ChildCount(Vec::new(), Vec::new()),
    };
    assert!(
        must_report(&new, false),
        "a NEW divergence must always panic"
    );

    // The default harness does not panic on a known-class input.
    let caught = std::panic::catch_unwind(|| xml_parser_diff("<:r/>".as_bytes()));
    assert!(caught.is_ok(), "known class must not panic by default");
}

// ---------------------------------------------------------------------------
// saml_xsw: the catalogue of wrapping attempts must all be rejected-or-genuine.
// ---------------------------------------------------------------------------

/// Every mutation sequence here is a known XSW shape; none may be accepted with
/// a non-genuine identity. `xsw::run` panics on a bypass, so reaching the end is
/// the assertion.
#[test]
fn xsw_catalogue_never_bypasses() {
    // Raw byte seeds chosen to decode (via Arbitrary) into non-empty mutation
    // lists; the harness explores structure, this just pins a broad sweep.
    for seed in 0u16..4096 {
        let bytes = seed.to_le_bytes();
        saml_xsw(&bytes);
        let wide = [
            bytes[0],
            bytes[1],
            bytes[0] ^ 0x5a,
            bytes[1] ^ 0xa5,
            0x01,
            0x02,
        ];
        saml_xsw(&wide);
    }
    // Empty and short inputs must be handled without panic.
    saml_xsw(&[]);
    saml_xsw(&[0]);
}

/// The fixture's baseline is accepted as genuine; if this breaks, every
/// rejection above is vacuous.
#[test]
fn xsw_baseline_is_accepted() {
    // Validating the unmutated signed SOAP goes through the lazily-built fixture
    // whose own assertion checks the genuine identity; an all-zero run that
    // produces no effective mutation exercises it.
    xsw::run(&[]);
}

// ---------------------------------------------------------------------------
// Seed corpus generation (driven by `bin/fuzz seeds`).
// ---------------------------------------------------------------------------

/// (Re)write the checked-in seed corpora from the committed fixtures. The seeds
/// are also committed, so this is idempotent; it exists so `bin/fuzz seeds` can
/// regenerate them. The diff seeds are real TVS wire samples plus a couple of
/// hand inputs exercising the folded normalisations.
#[test]
fn write_seeds() {
    use std::{fs, path::PathBuf};

    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let seeds = base.join("fuzz/seeds");
    let fixtures = base.join("tests/fixtures/tvs");

    let diff = seeds.join("saml_xml_parser_diff");
    fs::create_dir_all(&diff).expect("create diff seed dir");
    for name in [
        "artifact_response_success",
        "artifact_response_cluster",
        "artifact_response_login_cancelled",
        "artifact_response_request_denied",
        "artifact_response_digid_pre44",
    ] {
        let src = fixtures.join(format!("{name}.xml"));
        if let Ok(bytes) = fs::read(&src) {
            fs::write(diff.join(format!("{name}.xml")), bytes).expect("write diff seed");
        }
    }
    fs::write(
        diff.join("mixed_content.xml"),
        r#"<r xmlns="urn:a"><s a="x y&#9;z"><!--c--><![CDATA[d]]>&amp;</s></r>"#,
    )
    .expect("write seed");
    fs::write(
        diff.join("response.xml"),
        concat!(
            r#"<samlp:Response xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol" "#,
            r#"xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion" ID="_r">"#,
            r#"<saml:Issuer>urn:rd</saml:Issuer><samlp:Status>"#,
            r#"<samlp:StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:Success"/>"#,
            r#"</samlp:Status></samlp:Response>"#,
        ),
    )
    .expect("write seed");

    // saml_xsw seeds: byte vectors that decode (via Arbitrary) into mutation
    // lists spanning the catalogue.
    let xsw_dir = seeds.join("saml_xsw");
    fs::create_dir_all(&xsw_dir).expect("create xsw seed dir");
    let xsw_seeds: &[(&str, &[u8])] = &[
        ("wrap_assertion_extensions", &[0x01, 0x00, 0x02, 0x00]),
        ("duplicate_id", &[0x01, 0x01, 0x00, 0x01, 0x00]),
        ("split_nameid_comment", &[0x05, 0x00, 0x00]),
        ("add_signature", &[0x07, 0x00]),
        ("leading_colon_forgery", &[0x0e, 0x00]),
        (
            "many",
            &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
        ),
    ];
    for (name, bytes) in xsw_seeds {
        fs::write(xsw_dir.join(name), bytes).expect("write xsw seed");
    }
}

// ---------------------------------------------------------------------------
// Exploration helper (ignored): prints verdicts for ad-hoc inputs.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "exploration: run with --ignored --nocapture to inspect verdicts"]
fn explore() {
    for xml in AGREE {
        let v = compare(xml);
        let tag = match &v {
            Verdict::Diverge(d) => parser_diff::known_finding(d).unwrap_or("NEW"),
            _ => "",
        };
        println!("{xml:?} => {v:?} [{tag}]");
    }
}
