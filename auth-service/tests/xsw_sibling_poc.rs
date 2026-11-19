//! PoC: XML Signature Wrapping via a detached (sibling) reference.
//!
//! The RD signs an ArtifactResponse with an enveloped signature referencing the
//! ArtifactResponse @ID. An attacker who controls the response bytes (front-channel
//! LogoutResponse, or a subverted back-channel) rebuilds the document so the
//! genuine Signature becomes a *sibling* of a reconstructed copy of the genuine
//! signed element, and wraps a FORGED ArtifactResponse/Response around them as the
//! new document root. The signature's reference still resolves to the sibling
//! (whose digest is unchanged) while the validators read the forged root, so
//! the chain MUST reject the document: the §7.6.1 root-coverage check requires
//! every Reference to target the consumed root element.

mod common;
use common::{
    RD, SAML, SAMLP, SUCCESS, inline_signature as sig_template, load_key, response, run_chain,
    soap_envelope as soap, ts,
};

use auth_service::saml::crypto::sign;
use jiff::SignedDuration;

#[test]
fn sibling_wrapping_xsw() {
    let rd_key = load_key("rd-signing-1");
    let now = ts(SignedDuration::ZERO);

    // 1. Genuine ArtifactResponse, signed by the real RD key (reference -> _art1,
    //    enveloped). This is what an attacker can capture (or, for metadata/SLO, is
    //    simply public).
    let genuine_inner_body = response("1", "GENUINE-USER");
    let sig = sig_template("_art1", &rd_key.cert_base64);
    let genuine = format!(
        r#"<samlp:ArtifactResponse xmlns:samlp="{SAMLP}" xmlns:saml="{SAML}" ID="_art1" Version="2.0" IssueInstant="{now}"><saml:Issuer>{RD}</saml:Issuer>{sig}<samlp:Status><samlp:StatusCode Value="{SUCCESS}"/></samlp:Status>{genuine_inner_body}</samlp:ArtifactResponse>"#
    );
    let signed = sign(&genuine, &rd_key.key_pem).unwrap();

    // 2. Pull the now-filled <Signature> element out of the signed document.
    let sig_start = signed.find("<dsig:Signature").unwrap();
    let sig_end = signed.find("</dsig:Signature>").unwrap() + "</dsig:Signature>".len();
    let filled_sig = &signed[sig_start..sig_end];

    // 3. Rebuild inner _art1 WITHOUT its Signature child (the enveloped transform
    //    removed the signature when computing the genuine digest, so the digest over
    //    this signature-less copy is identical).
    let inner_no_sig = format!(
        r#"<samlp:ArtifactResponse xmlns:samlp="{SAMLP}" xmlns:saml="{SAML}" ID="_art1" Version="2.0" IssueInstant="{now}"><saml:Issuer>{RD}</saml:Issuer><samlp:Status><samlp:StatusCode Value="{SUCCESS}"/></samlp:Status>{genuine_inner_body}</samlp:ArtifactResponse>"#
    );

    // 4. Forged outer ArtifactResponse: the moved genuine Signature (still
    //    referencing _art1) + a FORGED Response, with the reconstructed genuine
    //    _art1 as a sibling so the reference still resolves and digests cleanly.
    let forged_body = response("F", "ATTACKER-CHOSEN-VICTIM");
    let outer = format!(
        r#"<samlp:ArtifactResponse xmlns:samlp="{SAMLP}" xmlns:saml="{SAML}" ID="_outer" Version="2.0" IssueInstant="{now}"><saml:Issuer>{RD}</saml:Issuer>{filled_sig}<samlp:Status><samlp:StatusCode Value="{SUCCESS}"/></samlp:Status>{forged_body}{inner_no_sig}</samlp:ArtifactResponse>"#
    );

    let result = run_chain(&soap(&outer), &rd_key);
    assert_ne!(
        result.accepted.as_deref(),
        Some("ATTACKER-CHOSEN-VICTIM"),
        "SECURITY: sibling-wrapping XSW accepted the attacker NameID"
    );
    // Pin *why* it was rejected: the §7.6.1 root-coverage check, not an
    // incidental fixture problem. Without this the test would still pass if the
    // chain broke for an unrelated reason.
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.contains("does not target the signed root element")),
        "expected the Reference/root-coverage rejection, got: {:?}",
        result.errors
    );
}

/// Positive control: the same builder, unwrapped, must be accepted. Guards the
/// test above from silently passing because its fixture rotted.
#[test]
fn baseline_genuine_artifact_response_is_accepted() {
    let rd_key = load_key("rd-signing-1");
    let now = ts(SignedDuration::ZERO);
    let genuine_inner_body = response("1", "GENUINE-USER");
    let sig = sig_template("_art1", &rd_key.cert_base64);
    let genuine = format!(
        r#"<samlp:ArtifactResponse xmlns:samlp="{SAMLP}" xmlns:saml="{SAML}" ID="_art1" Version="2.0" IssueInstant="{now}"><saml:Issuer>{RD}</saml:Issuer>{sig}<samlp:Status><samlp:StatusCode Value="{SUCCESS}"/></samlp:Status>{genuine_inner_body}</samlp:ArtifactResponse>"#
    );
    let signed = sign(&genuine, &rd_key.key_pem).unwrap();

    let result = run_chain(&soap(&signed), &rd_key);
    assert_eq!(
        result.accepted.as_deref(),
        Some("GENUINE-USER"),
        "genuine document must be accepted, errors: {:?}",
        result.errors
    );
}
