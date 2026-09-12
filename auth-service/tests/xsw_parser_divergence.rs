//! The signed bytes are parsed three times: by `run_chain`'s tree, again inside
//! `verify_xml_signature`, and once more by the crypto backend. All three now go
//! through `uppsala`, so cross-parser divergence is gone by construction (see
//! `saml::xml_parser`); what remains is that these are still three *separate*
//! parses, with a slice-and-reparse in between, of bytes an attacker shaped. Any
//! construct that survives one of those steps differently is a signature
//! wrapping vector: the digest covers one thing and the claims come from another.
//!
//! Every case here must either be rejected or yield exactly the genuine claims.
//! Accepting an attacker-chosen NameID is the failure this suite exists to catch.

mod common;
use common::{
    RD, SAML, SAMLP, SUCCESS, inline_signature as sig_template, load_key, response, run_chain,
    soap_envelope as soap, ts,
};

use auth_service::{keys::KeyPair, saml::crypto::sign};
use chrono::Duration;

const GENUINE: &str = "GENUINE-USER";
const ATTACKER: &str = "ATTACKER-CHOSEN-VICTIM";

/// A genuine RD-signed ArtifactResponse, plus its unsigned template.
///
/// `art_attrs` is spliced into the ArtifactResponse start tag and `extra_body`
/// after the Status, so a case can add ID attributes or sibling elements before
/// the RD signs (a signed document) or after (a tampered one).
fn artifact_response(art_attrs: &str, extra_body: &str, name_id: &str) -> String {
    let now = ts(Duration::zero());
    let body = response("1", name_id);
    let sig = sig_template("_art1", &load_key("rd-signing-1").cert_base64);
    format!(
        r#"<samlp:ArtifactResponse xmlns:samlp="{SAMLP}" xmlns:saml="{SAML}" ID="_art1" Version="2.0" IssueInstant="{now}"{art_attrs}><saml:Issuer>{RD}</saml:Issuer>{sig}<samlp:Status><samlp:StatusCode Value="{SUCCESS}"/></samlp:Status>{extra_body}{body}</samlp:ArtifactResponse>"#
    )
}

fn signed_artifact_response(rd_key: &KeyPair, art_attrs: &str, extra_body: &str) -> String {
    let xml = artifact_response(art_attrs, extra_body, GENUINE);
    sign(&xml, &rd_key.key_pem).expect("RD signing")
}

/// Assert the chain either rejected `soap_xml` or read the genuine NameID from
/// it. Never the attacker's, and never a partially-trusted mixture.
fn assert_rejected_or_genuine(label: &str, soap_xml: &str, rd_key: &KeyPair) {
    let result = run_chain(soap_xml, rd_key);
    assert_ne!(
        result.accepted.as_deref(),
        Some(ATTACKER),
        "SECURITY [{label}]: attacker NameID accepted, errors: {:?}",
        result.errors
    );
    if let Some(accepted) = result.accepted.as_deref() {
        assert_eq!(
            accepted, GENUINE,
            "[{label}]: accepted an unexpected NameID, errors: {:?}",
            result.errors
        );
    }
}

/// Positive control. If this breaks, every rejection below is meaningless.
#[test]
fn baseline_signed_artifact_response_is_accepted() {
    let rd_key = load_key("rd-signing-1");
    let signed = signed_artifact_response(&rd_key, "", "");
    let result = run_chain(&soap(&signed), &rd_key);
    assert_eq!(
        result.accepted.as_deref(),
        Some(GENUINE),
        "errors: {:?}",
        result.errors
    );
}

/// Two elements carrying the referenced ID make `#_art1` ambiguous: the backend
/// could digest one while the chain reads the other.
#[test]
fn duplicate_id_value_is_rejected() {
    let rd_key = load_key("rd-signing-1");
    let signed = signed_artifact_response(&rd_key, "", "");
    let tampered = signed.replace(
        r#"<samlp:Status>"#,
        r#"<samlp:Extension ID="_art1"/><samlp:Status>"#,
    );
    assert_ne!(tampered, signed, "tampering must apply");

    let result = run_chain(&soap(&tampered), &rd_key);
    assert!(result.accepted.is_none(), "errors: {:?}", result.errors);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.contains("carry the ID") || e.contains("duplicate ID")),
        "expected an ID-uniqueness rejection, got: {:?}",
        result.errors
    );
}

/// The same value under a different ID-ish attribute name is still a collision:
/// our attribute set has to match the one the backend resolves references with.
#[test]
fn duplicate_id_under_another_id_attribute_is_rejected() {
    let rd_key = load_key("rd-signing-1");
    for attr in ["Id", "id", "AssertionID", "xml:id"] {
        let signed = signed_artifact_response(&rd_key, "", "");
        let tampered = signed.replace(
            r#"<samlp:Status>"#,
            &format!(r#"<samlp:Extension {attr}="_art1"/><samlp:Status>"#),
        );
        let result = run_chain(&soap(&tampered), &rd_key);
        assert!(
            result.accepted.is_none(),
            "{attr}: a second element carrying the referenced ID must be rejected, errors: {:?}",
            result.errors
        );
    }
}

/// `xml:id` is registered as an ID by the backend twice over (its attribute
/// lookup matches by local name, so `xml:id` also answers to `id`), which makes
/// it collide with itself. Any document carrying one therefore fails closed, and
/// no `xml:id` alias can make a reference resolve away from the root.
#[test]
fn xml_id_on_the_root_fails_closed() {
    let rd_key = load_key("rd-signing-1");
    let signed = signed_artifact_response(&rd_key, "", "");
    let tampered = signed.replacen(r#"ID="_art1""#, r#"ID="_art1" xml:id="_alias""#, 1);
    assert_ne!(tampered, signed, "tampering must apply");

    let result = run_chain(&soap(&tampered), &rd_key);
    assert!(
        result.accepted.is_none(),
        "an xml:id alias on the signed root must not be accepted, errors: {:?}",
        result.errors
    );
}

/// Comments are excluded from both exclusive c14n and the parsed tree, so a
/// comment splitting an identity string must not smuggle one past either.
#[test]
fn comments_inside_identity_text_do_not_forge_a_value() {
    let rd_key = load_key("rd-signing-1");
    let cases = [
        ("Issuer", format!("<saml:Issuer>{RD}</saml:Issuer>"), format!("<saml:Issuer>{RD}<!--x-->EXTRA</saml:Issuer>")),
        (
            "NameID",
            format!(r#"<saml:NameID Format="urn:oasis:names:tc:SAML:2.0:nameid-format:transient">{GENUINE}</saml:NameID>"#),
            format!(r#"<saml:NameID Format="urn:oasis:names:tc:SAML:2.0:nameid-format:transient">{GENUINE}<!--x-->{ATTACKER}</saml:NameID>"#),
        ),
        (
            "AuthnContextClassRef",
            "<saml:AuthnContextClassRef>http://eidas.europa.eu/LoA/substantial</saml:AuthnContextClassRef>".to_string(),
            "<saml:AuthnContextClassRef>http://eidas.europa.eu/LoA/<!--x-->low</saml:AuthnContextClassRef>".to_string(),
        ),
    ];

    for (label, from, to) in cases {
        let signed = signed_artifact_response(&rd_key, "", "");
        let tampered = signed.replacen(&from, &to, 1);
        assert_ne!(tampered, signed, "[{label}] tampering must apply");
        // Tampering after signing breaks the digest, so this must be rejected;
        // the point is that it is never *silently* read differently.
        let result = run_chain(&soap(&tampered), &rd_key);
        assert!(
            result.accepted.is_none(),
            "[{label}]: comment-split identity text was accepted, errors: {:?}",
            result.errors
        );
    }
}

/// An identity wrapped in a child element is not that identity: `direct_text`
/// must not fold child text into `Issuer` / `NameID` / `AuthnContextClassRef`.
#[test]
fn identity_text_inside_a_child_element_is_not_read_as_the_identity() {
    let rd_key = load_key("rd-signing-1");
    // Signed *with* the nested shape, so the digest is valid and the only thing
    // that can reject it is the extraction rule itself.
    let now = ts(Duration::zero());
    let body = response("1", GENUINE);
    let sig = sig_template("_art1", &rd_key.cert_base64);
    let nested_issuer = format!(
        r#"<samlp:ArtifactResponse xmlns:samlp="{SAMLP}" xmlns:saml="{SAML}" ID="_art1" Version="2.0" IssueInstant="{now}"><saml:Issuer><wrap>{RD}</wrap></saml:Issuer>{sig}<samlp:Status><samlp:StatusCode Value="{SUCCESS}"/></samlp:Status>{body}</samlp:ArtifactResponse>"#
    );
    let signed = sign(&nested_issuer, &rd_key.key_pem).expect("RD signing");

    let result = run_chain(&soap(&signed), &rd_key);
    assert!(
        result.accepted.is_none(),
        "an Issuer whose text lives in a child element must not satisfy the RD binding"
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.contains("Issuer contains child elements")),
        "expected the direct-text rejection, got: {:?}",
        result.errors
    );
}

/// A DTD is the classic entity-expansion and XXE surface, and the two parsers
/// need not agree on entity handling. `allow_dtd` is off, so it never parses.
#[test]
fn doctype_with_internal_entity_is_rejected() {
    let rd_key = load_key("rd-signing-1");
    let signed = signed_artifact_response(&rd_key, "", "");
    let with_doctype = format!(
        r#"<?xml version="1.0"?><!DOCTYPE Envelope [<!ENTITY xxe "{ATTACKER}">]>{}"#,
        soap(&signed)
    );
    let result = run_chain(&with_doctype, &rd_key);
    assert!(
        result.accepted.is_none(),
        "a document with a DTD must not be processed, errors: {:?}",
        result.errors
    );
}

/// CDATA and character references are alternative spellings of text. Both
/// parsers must agree, so an identity spelled either way is never a second,
/// different identity.
#[test]
fn cdata_and_character_references_do_not_forge_an_identity() {
    let rd_key = load_key("rd-signing-1");
    let spellings = [
        format!("<![CDATA[{ATTACKER}]]>"),
        // "ATTACKER-CHOSEN-VICTIM" with the leading 'A' as a character reference.
        "&#65;TTACKER-CHOSEN-VICTIM".to_string(),
        "&#x41;TTACKER-CHOSEN-VICTIM".to_string(),
    ];
    for spelling in spellings {
        let signed = signed_artifact_response(&rd_key, "", "");
        let tampered = signed.replacen(GENUINE, &spelling, 1);
        assert_rejected_or_genuine(&format!("spelling {spelling}"), &soap(&tampered), &rd_key);
    }
}

/// Whitespace in base64 is legal and must be tolerated the same way by the
/// structural checks and the backend: either it verifies or it fails, never a
/// silent pass on a different digest.
#[test]
fn whitespace_in_signature_and_digest_values_is_not_exploitable() {
    let rd_key = load_key("rd-signing-1");
    let signed = signed_artifact_response(&rd_key, "", "");

    // Split the SignatureValue across lines, as many signers do.
    let sig_start = signed.find("<dsig:SignatureValue>").unwrap() + "<dsig:SignatureValue>".len();
    let sig_end = signed.find("</dsig:SignatureValue>").unwrap();
    let value = &signed[sig_start..sig_end];
    let wrapped: String = value
        .as_bytes()
        .chunks(64)
        .map(|c| format!("{}\n", std::str::from_utf8(c).unwrap()))
        .collect();
    let reformatted = format!(
        "{}\n{}{}",
        &signed[..sig_start],
        wrapped,
        &signed[sig_end..]
    );

    let result = run_chain(&soap(&reformatted), &rd_key);
    assert_ne!(result.accepted.as_deref(), Some(ATTACKER));
    assert_eq!(
        result.accepted.as_deref(),
        Some(GENUINE),
        "a line-wrapped SignatureValue must still verify, errors: {:?}",
        result.errors
    );
}

/// A `Signature` nested inside `Advice` sits earlier in document order than the
/// enveloping one, and the backend verifies the first it finds.
#[test]
fn signature_nested_in_advice_before_the_enveloping_one_is_rejected() {
    let rd_key = load_key("rd-signing-1");
    let signed = signed_artifact_response(&rd_key, "", "");
    let stolen_sig = {
        let start = signed.find("<dsig:Signature").unwrap();
        let end = signed.find("</dsig:Signature>").unwrap() + "</dsig:Signature>".len();
        signed[start..end].to_string()
    };
    // Place a copy of the genuine signature before the enveloping one.
    let tampered = signed.replacen(
        r#"<saml:Issuer>"#,
        &format!(r#"<saml:Advice>{stolen_sig}</saml:Advice><saml:Issuer>"#),
        1,
    );
    assert_ne!(tampered, signed, "tampering must apply");

    let result = run_chain(&soap(&tampered), &rd_key);
    assert!(result.accepted.is_none(), "errors: {:?}", result.errors);
    assert!(
        result.errors.iter().any(|e| e.contains("wrapping")),
        "expected a wrapping rejection, got: {:?}",
        result.errors
    );
}

/// The ArtifactResponse slice is verified as a standalone document. When the RD
/// declares the namespaces on the envelope instead of the element, the inherited
/// declarations are restored, and the result must be identical to the
/// self-contained form.
#[test]
fn namespaces_declared_on_the_envelope_still_verify() {
    let rd_key = load_key("rd-signing-1");
    let now = ts(Duration::zero());
    let body = response("1", GENUINE);
    let sig = sig_template("_art1", &rd_key.cert_base64);

    // The ArtifactResponse uses samlp:/saml: without declaring them; the envelope
    // does. The RD signs the whole envelope's inner element in that scope.
    let envelope = format!(
        r#"<soapenv:Envelope xmlns:soapenv="http://schemas.xmlsoap.org/soap/envelope/" xmlns:samlp="{SAMLP}" xmlns:saml="{SAML}"><soapenv:Body><samlp:ArtifactResponse ID="_art1" Version="2.0" IssueInstant="{now}"><saml:Issuer>{RD}</saml:Issuer>{sig}<samlp:Status><samlp:StatusCode Value="{SUCCESS}"/></samlp:Status>{body}</samlp:ArtifactResponse></soapenv:Body></soapenv:Envelope>"#
    );
    let signed = sign(&envelope, &rd_key.key_pem).expect("RD signing");

    let result = run_chain(&signed, &rd_key);
    assert_eq!(
        result.accepted.as_deref(),
        Some(GENUINE),
        "envelope-declared namespaces must verify via the reconstruction, errors: {:?}",
        result.errors
    );
}

/// The reconstruction must not rescue a forgery: same envelope-level namespaces,
/// but the NameID is changed after signing.
#[test]
fn namespace_reconstruction_does_not_rescue_a_tampered_document() {
    let rd_key = load_key("rd-signing-1");
    let now = ts(Duration::zero());
    let body = response("1", GENUINE);
    let sig = sig_template("_art1", &rd_key.cert_base64);
    let envelope = format!(
        r#"<soapenv:Envelope xmlns:soapenv="http://schemas.xmlsoap.org/soap/envelope/" xmlns:samlp="{SAMLP}" xmlns:saml="{SAML}"><soapenv:Body><samlp:ArtifactResponse ID="_art1" Version="2.0" IssueInstant="{now}"><saml:Issuer>{RD}</saml:Issuer>{sig}<samlp:Status><samlp:StatusCode Value="{SUCCESS}"/></samlp:Status>{body}</samlp:ArtifactResponse></soapenv:Body></soapenv:Envelope>"#
    );
    let signed = sign(&envelope, &rd_key.key_pem).expect("RD signing");
    let tampered = signed.replacen(GENUINE, ATTACKER, 1);
    assert_ne!(tampered, signed, "tampering must apply");

    let result = run_chain(&tampered, &rd_key);
    assert!(
        result.accepted.is_none(),
        "SECURITY: reconstruction accepted a tampered document, errors: {:?}",
        result.errors
    );
}
