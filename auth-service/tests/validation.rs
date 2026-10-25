//! Validation tests for SAML Response, ArtifactResponse, and Assertion processing.
//!
//! Tests are organized by the eID SAML 4.4 specification sections they cover.
//! References like "§7.6.3.5 rule 2" point to ../eid-saml-4.4-requirements.md.
//!
//! The Assertion is not signed independently; its authenticity comes from the
//! enveloping RD signature on the ArtifactResponse (verified separately). These
//! tests pass `expected_issuer: None` to focus on the other content checks; the
//! Issuer binding has dedicated tests.

use auth_service::{
    bindings::soap::unwrap_soap,
    keys::KeyPair,
    saml::{
        constants::*,
        crypto::sign,
        loa::MINIMUM_LOA,
        validation::{
            ValidateArtifactResponseOpts, ValidateAssertionOpts, ValidateResponseOpts,
            validate_artifact_response_at, validate_assertion_at, validate_response_at,
        },
        xml_parser::parse,
    },
    types::{EndpointUrl, EntityId, MessageId},
};
use chrono::Duration;

mod common;
use common::{
    inline_signature, load_key, soap_envelope as soap_wrap, ts, validate_artifact_response,
    validate_assertion, validate_response,
};

const RD_ENTITY_ID: &str = "urn:test:rd";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Remove the first `<tag ...>...</tag>` element (with its children) from `xml`.
fn strip_element(xml: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let start = xml.find(&open).expect("element present");
    let end = xml[start..]
        .find(&close)
        .map(|i| start + i + close.len())
        .expect("close tag present");
    let mut out = xml.to_string();
    out.replace_range(start..end, "");
    out
}

/// Build a complete, RD-signed `ArtifactResponse` SOAP envelope wrapping a valid
/// `Response` + outer RD `Assertion`, signed (enveloped) with `signing_key`. The
/// `<Signature>` is inserted after the first `</saml:Issuer>`, making it a direct
/// child of the ArtifactResponse, the only signature the SP verifies.
fn signed_artifact_response_soap(signing_key: &KeyPair) -> String {
    // Model real TVS: namespaces are declared once on the ArtifactResponse root,
    // and the inner Response/Assertion inherit them without redeclaring. (Strip
    // the Assertion's own xmlns:saml so it, too, inherits.)
    let now = ts(Duration::zero());
    // The outer RD Assertion carries the eID LoA (the eIDAS Substantial URL the
    // RD emits, per §10.3) and our claims.
    let outer = AssertionBuilder {
        authn_class_ref: Some("http://eidas.europa.eu/LoA/substantial".into()),
        ..AssertionBuilder::default()
    }
    .build()
    .replace(&format!(r#" xmlns:saml="{NS_SAML}""#), "");
    // Inject an <Advice> with the original AD (eIDAS) assertion as the LAST child,
    // carrying conflicting Recipient / InResponseTo and a scheme-specific LoA. It
    // must be ignored: claims come from the outer Assertion only.
    let advice = format!(
        r#"<saml:Advice><saml:Assertion ID="_inner" Version="2.0" IssueInstant="{now}"><saml:Issuer>urn:test:eidas-ad</saml:Issuer><saml:Subject><saml:SubjectConfirmation Method="{bearer}"><saml:SubjectConfirmationData NotOnOrAfter="{scd}" Recipient="https://pp2.toegang.overheid.nl/foam/saml/acs" InResponseTo="_tvs-internal-id"/></saml:SubjectConfirmation></saml:Subject><saml:AuthnStatement AuthnInstant="{now}"><saml:AuthnContext><saml:AuthnContextClassRef>http://eidas.europa.eu/LoA/substantial</saml:AuthnContextClassRef></saml:AuthnContext></saml:AuthnStatement></saml:Assertion></saml:Advice>"#,
        bearer = SUBJECT_CONFIRMATION_BEARER,
        scd = ts(Duration::minutes(2)),
    );
    let assertion = outer.replace("</saml:Assertion>", &format!("{advice}</saml:Assertion>"));
    let response = format!(
        r#"<samlp:Response ID="_resp1" Version="2.0" IssueInstant="{now}" Destination="{ACS_URL}"><saml:Issuer>{RD_ENTITY_ID}</saml:Issuer><samlp:Status><samlp:StatusCode Value="{STATUS_SUCCESS}"/></samlp:Status>{assertion}</samlp:Response>"#
    );
    // The RD-signed enveloping signature is inline, right after the
    // ArtifactResponse's own Issuer (a direct child, the only signature the SP
    // verifies); the signer fills the digest/signature.
    let sig = inline_signature("_art1", &signing_key.cert_base64);
    let artifact_response = format!(
        r#"<samlp:ArtifactResponse xmlns:samlp="{NS_SAMLP}" xmlns:saml="{NS_SAML}" ID="_art1" Version="2.0" IssueInstant="{now}"><saml:Issuer>{RD_ENTITY_ID}</saml:Issuer>{sig}<samlp:Status><samlp:StatusCode Value="{STATUS_SUCCESS}"/></samlp:Status>{response}</samlp:ArtifactResponse>"#
    );
    let signed = sign(&artifact_response, &signing_key.key_pem).expect("signing must succeed");
    soap_wrap(&signed)
}

const DV_ENTITY_ID: &str = "urn:test:dv";
const ACS_URL: &str = "https://dv.example.com/acs";

/// The same two as the domain types the validator options take.
fn dv_entity_id() -> EntityId {
    EntityId::from_static(DV_ENTITY_ID)
}

fn rd_entity_id() -> EntityId {
    EntityId::from_static(RD_ENTITY_ID)
}

fn acs_url() -> EndpointUrl {
    EndpointUrl::from_metadata(ACS_URL, "ACS").expect("test ACS URL")
}

/// Build a minimal but well-formed Assertion XML.
///
/// Every parameter can be overridden; `None` omits the element entirely.
struct AssertionBuilder {
    issuer: String,
    name_id: Option<String>,
    subject_method: String,
    scd_recipient: String,
    scd_not_on_or_after: String,
    conditions_not_before: String,
    conditions_not_on_or_after: String,
    audiences: Vec<String>,
    authn_class_ref: Option<String>,
    auth_authority: Option<String>,
    service_uuid: Option<String>,
    extra_attributes: String,
}

impl Default for AssertionBuilder {
    fn default() -> Self {
        Self {
            issuer: "urn:test:rd".into(),
            name_id: Some("user-123".into()),
            subject_method: SUBJECT_CONFIRMATION_BEARER.into(),
            scd_recipient: ACS_URL.into(),
            scd_not_on_or_after: ts(Duration::minutes(2)),
            conditions_not_before: ts(-Duration::minutes(5)),
            conditions_not_on_or_after: ts(Duration::minutes(5)),
            audiences: vec![DV_ENTITY_ID.into()],
            authn_class_ref: Some(
                "urn:oasis:names:tc:SAML:2.0:ac:classes:PasswordProtectedTransport".into(),
            ),
            auth_authority: Some("urn:test:ad".into()),
            service_uuid: Some("f847dc11-ac24-47b2-84a8-a057440ce56d".into()),
            extra_attributes: String::new(),
        }
    }
}

impl AssertionBuilder {
    fn build(&self) -> String {
        let name_id = self
            .name_id
            .as_ref()
            .map(|v| format!(r#"<saml:NameID Format="{NAMEID_TRANSIENT}">{v}</saml:NameID>"#))
            .unwrap_or_default();

        let audiences: String = self
            .audiences
            .iter()
            .map(|a| format!("<saml:Audience>{a}</saml:Audience>"))
            .collect();

        let authn_class = self
            .authn_class_ref
            .as_ref()
            .map(|v| format!("<saml:AuthnContextClassRef>{v}</saml:AuthnContextClassRef>"))
            .unwrap_or_default();

        let auth_authority = self
            .auth_authority
            .as_ref()
            .map(|v| format!("<saml:AuthenticatingAuthority>{v}</saml:AuthenticatingAuthority>"))
            .unwrap_or_default();

        let service_uuid_attr = self
            .service_uuid
            .as_ref()
            .map(|v| {
                format!(
                    r#"<saml:Attribute Name="{EID_SERVICE_UUID}"><saml:AttributeValue>{v}</saml:AttributeValue></saml:Attribute>"#
                )
            })
            .unwrap_or_default();

        format!(
            r#"<saml:Assertion xmlns:saml="{NS_SAML}" ID="_a1" Version="2.0" IssueInstant="{now}">
<saml:Issuer>{issuer}</saml:Issuer>
<saml:Subject>
  {name_id}
  <saml:SubjectConfirmation Method="{method}">
    <saml:SubjectConfirmationData NotOnOrAfter="{scd_noa}" Recipient="{recipient}"/>
  </saml:SubjectConfirmation>
</saml:Subject>
<saml:Conditions NotBefore="{cond_nb}" NotOnOrAfter="{cond_noa}">
  <saml:AudienceRestriction>{audiences}</saml:AudienceRestriction>
</saml:Conditions>
<saml:AuthnStatement AuthnInstant="{now}">
  <saml:AuthnContext>{authn_class}{auth_authority}</saml:AuthnContext>
</saml:AuthnStatement>
<saml:AttributeStatement>{service_uuid_attr}{extra}</saml:AttributeStatement>
</saml:Assertion>"#,
            now = ts(Duration::zero()),
            issuer = self.issuer,
            method = self.subject_method,
            scd_noa = self.scd_not_on_or_after,
            recipient = self.scd_recipient,
            cond_nb = self.conditions_not_before,
            cond_noa = self.conditions_not_on_or_after,
            extra = self.extra_attributes,
        )
    }
}

fn validate(xml: &str) -> common::AssertionResult {
    validate_assertion(
        xml,
        ValidateAssertionOpts {
            dv_entity_id: &dv_entity_id(),
            expected_recipient: Some(&acs_url()),
            expected_issuer: None,
            private_keys: &[],
            minimum_loa: None,
            expected_service_uuid: None,
        },
    )
}

/// Like `validate`, but enforces the production `MINIMUM_LOA`.
fn validate_with_loa(xml: &str) -> common::AssertionResult {
    validate_assertion(
        xml,
        ValidateAssertionOpts {
            dv_entity_id: &dv_entity_id(),
            expected_recipient: Some(&acs_url()),
            expected_issuer: None,
            private_keys: &[],
            minimum_loa: Some(MINIMUM_LOA),
            expected_service_uuid: None,
        },
    )
}

// ===========================================================================
// §7.6.2: Response validation
// ===========================================================================

/// §7.6.2: A successful Response MUST contain an Assertion.
#[test]
fn response_success_with_assertion() {
    let xml = format!(
        r#"<samlp:Response xmlns:samlp="{NS_SAMLP}" Version="2.0" IssueInstant="{now}"><samlp:Status><samlp:StatusCode Value="{STATUS_SUCCESS}"/></samlp:Status><saml:Assertion xmlns:saml="{NS_SAML}" ID="_a1">data</saml:Assertion></samlp:Response>"#,
        now = ts(Duration::zero())
    );
    let r = validate_response(&xml);
    assert!(r.valid, "errors: {:?}", r.errors);
    assert!(r.assertion_xml.is_some());
}

/// §7.6.2: A successful Response without an Assertion is invalid.
#[test]
fn response_success_without_assertion() {
    let xml = format!(
        r#"<samlp:Response xmlns:samlp="{NS_SAMLP}"><samlp:Status><samlp:StatusCode Value="{STATUS_SUCCESS}"/></samlp:Status></samlp:Response>"#
    );
    let r = validate_response(&xml);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.contains("No Assertion")));
}

/// §7.6.2 / §7.8: Error status codes must be reported.
#[test]
fn response_responder_status() {
    let xml = format!(
        r#"<samlp:Response xmlns:samlp="{NS_SAMLP}"><samlp:Status><samlp:StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:Responder"/></samlp:Status></samlp:Response>"#
    );
    let r = validate_response(&xml);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.contains("Responder")));
}

/// §7.8.1: Secondary StatusCode (e.g. AuthnFailed) must be reported.
#[test]
fn response_nested_status_code() {
    let xml = format!(
        r#"<samlp:Response xmlns:samlp="{NS_SAMLP}"><samlp:Status><samlp:StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:Responder"><samlp:StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:AuthnFailed"/></samlp:StatusCode></samlp:Status></samlp:Response>"#
    );
    let r = validate_response(&xml);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.contains("AuthnFailed")));
}

/// §7.8.3: StatusMessage is included when authentication is cancelled.
#[test]
fn response_status_message_reported() {
    let xml = format!(
        r#"<samlp:Response xmlns:samlp="{NS_SAMLP}"><samlp:Status><samlp:StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:Responder"><samlp:StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:AuthnFailed"/></samlp:StatusCode><samlp:StatusMessage>Authentication cancelled</samlp:StatusMessage></samlp:Status></samlp:Response>"#
    );
    let r = validate_response(&xml);
    assert!(!r.valid);
    assert!(
        r.errors
            .iter()
            .any(|e| e.contains("Authentication cancelled")),
        "errors: {:?}",
        r.errors
    );
}

/// §7.6.2: Malformed XML must be rejected.
#[test]
fn response_malformed_xml() {
    let r = validate_response("<<<not xml");
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.contains("XML parse error")));
}

// ===========================================================================
// §7.6.1: ArtifactResponse validation
// ===========================================================================

/// §7.6.1: ArtifactResponse root element must be ArtifactResponse.
#[test]
fn artifact_response_wrong_root_element() {
    let soap = soap_wrap(
        r#"<samlp:NotAnArtifactResponse xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol" ID="_1"/>"#,
    );
    let r = validate_artifact_response(&soap, &[], None);
    assert!(!r.valid);
    assert!(
        r.errors
            .iter()
            .any(|e| e.contains("Expected ArtifactResponse")),
        "errors: {:?}",
        r.errors
    );
}

/// §7.6.1: InResponseTo must match the original ArtifactResolve ID.
#[test]
fn artifact_response_in_response_to_mismatch() {
    let soap = soap_wrap(&format!(
        r#"<samlp:ArtifactResponse xmlns:samlp="{NS_SAMLP}" ID="_1" InResponseTo="_wrong"><samlp:Status><samlp:StatusCode Value="{STATUS_SUCCESS}"/></samlp:Status></samlp:ArtifactResponse>"#
    ));
    let r = validate_artifact_response(&soap, &[], Some(&MessageId::parse("_expected").unwrap()));
    assert!(!r.valid);
    assert!(
        r.errors.iter().any(|e| e.contains("InResponseTo mismatch")),
        "errors: {:?}",
        r.errors
    );
}

/// §7.6.1: ArtifactResponse @Version is mandatory and MUST be 2.0.
#[test]
fn artifact_response_missing_version_rejected() {
    let soap = soap_wrap(&format!(
        r#"<samlp:ArtifactResponse xmlns:samlp="{NS_SAMLP}" ID="_1"><samlp:Status><samlp:StatusCode Value="{STATUS_SUCCESS}"/></samlp:Status></samlp:ArtifactResponse>"#
    ));
    let r = validate_artifact_response(&soap, &[], None);
    assert!(!r.valid);
    assert!(
        r.errors
            .iter()
            .any(|e| e.contains("missing the required @Version")),
        "errors: {:?}",
        r.errors
    );
}

/// §7.6.1: ArtifactResponse with error status must be rejected.
#[test]
fn artifact_response_error_status() {
    let soap = soap_wrap(&format!(
        r#"<samlp:ArtifactResponse xmlns:samlp="{NS_SAMLP}" ID="_1"><samlp:Status><samlp:StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:Requester"/></samlp:Status></samlp:ArtifactResponse>"#
    ));
    let r = validate_artifact_response(&soap, &[], None);
    assert!(!r.valid);
    assert!(
        r.errors
            .iter()
            .any(|e| e.contains("ArtifactResponse status")),
        "errors: {:?}",
        r.errors
    );
}

/// §7.6.1: A successful ArtifactResponse without inner Response is an error.
/// Note: the "No Response" check only triggers when no other errors are present
/// (e.g. signature is valid). With an unsigned test document the signature error
/// masks this, so we only assert response_xml is None.
#[test]
fn artifact_response_success_without_inner_response() {
    let soap = soap_wrap(&format!(
        r#"<samlp:ArtifactResponse xmlns:samlp="{NS_SAMLP}" ID="_1"><samlp:Status><samlp:StatusCode Value="{STATUS_SUCCESS}"/></samlp:Status></samlp:ArtifactResponse>"#
    ));
    let r = validate_artifact_response(&soap, &[], None);
    assert!(!r.valid);
    assert!(r.response_xml.is_none(), "should not extract a Response");
}

/// §7.6.1: Invalid SOAP envelope must be rejected.
#[test]
fn artifact_response_invalid_soap() {
    let r = validate_artifact_response("<not-soap/>", &[], None);
    assert!(!r.valid);
    assert!(
        r.errors.iter().any(|e| e.contains("SOAP")),
        "errors: {:?}",
        r.errors
    );
}

/// §7.6.1: Malformed XML inside SOAP body must be rejected.
#[test]
fn artifact_response_malformed_xml() {
    let soap = soap_wrap("<<<not xml");
    let r = validate_artifact_response(&soap, &[], None);
    assert!(!r.valid);
    assert!(
        r.errors.iter().any(|e| e.contains("XML parse error")),
        "errors: {:?}",
        r.errors
    );
}

// ===========================================================================
// §7.6.3.5 rule 1: Issuer binding (the Assertion is authenticated by the
// enveloping RD signature on the ArtifactResponse; here we bind the Issuer).
// ===========================================================================

/// Issuer matching the expected RD EntityID passes the binding check.
#[test]
fn assertion_issuer_matching_rd_accepted() {
    let xml = AssertionBuilder::default().build(); // issuer = "urn:test:rd"
    let r = validate_assertion(
        &xml,
        ValidateAssertionOpts {
            dv_entity_id: &dv_entity_id(),
            expected_recipient: Some(&acs_url()),
            expected_issuer: Some(&EntityId::from_static("urn:test:rd")),
            private_keys: &[],
            minimum_loa: None,
            expected_service_uuid: None,
        },
    );
    assert!(
        !r.errors.iter().any(|e| e.contains("Issuer")),
        "no issuer error expected, got: {:?}",
        r.errors
    );
}

/// Issuer not matching the expected RD EntityID is rejected.
#[test]
fn assertion_issuer_mismatch_rejected() {
    let xml = AssertionBuilder::default().build(); // issuer = "urn:test:rd"
    let r = validate_assertion(
        &xml,
        ValidateAssertionOpts {
            dv_entity_id: &dv_entity_id(),
            expected_recipient: Some(&acs_url()),
            expected_issuer: Some(&EntityId::from_static("urn:test:some-other-ad")),
            private_keys: &[],
            minimum_loa: None,
            expected_service_uuid: None,
        },
    );
    assert!(
        r.errors.iter().any(|e| e.contains("Issuer mismatch")),
        "expected issuer mismatch error, got: {:?}",
        r.errors
    );
}

// ===========================================================================
// End-to-end: a genuinely RD-signed ArtifactResponse must flow through the
// whole chain (envelope signature with RD key, then Response status, then
// Assertion with Issuer bound to the RD EntityID), proving the Assertion is
// authenticated by the enveloping RD signature, with no per-AD signing key
// involved.
// ===========================================================================

#[test]
fn signed_artifact_response_full_chain_succeeds() {
    let rd_key = load_key("rd-signing-1");
    let soap = signed_artifact_response_soap(&rd_key);

    // The handler parses the SOAP response exactly once and navigates the single
    // tree: ArtifactResponse, then Response, then Assertion. The inner elements
    // inherit their namespaces from the ArtifactResponse and are never re-parsed
    // as standalone fragments.
    let doc = parse(&soap).unwrap();
    let root = doc.document_element();
    let art_node = unwrap_soap(&doc, root).expect("SOAP body unwrapped");

    // Step 3: verify the ArtifactResponse envelope against the RD signing key.
    let mut errors = Vec::new();
    let response_node = validate_artifact_response_at(
        &doc,
        art_node,
        &ValidateArtifactResponseOpts {
            trusted_keys: std::slice::from_ref(&rd_key),
            expected_in_response_to: None,
            expected_issuer: Some(&rd_entity_id()),
        },
        &mut errors,
    )
    .expect("response extracted");
    assert!(
        errors.is_empty(),
        "ArtifactResponse must verify: {errors:?}"
    );

    // Step 4: inner Response status is Success and carries an Assertion.
    let mut resp_errors = Vec::new();
    let assertion_node = validate_response_at(
        &doc,
        response_node,
        &ValidateResponseOpts {
            expected_destination: None,
            expected_issuer: None,
        },
        &mut resp_errors,
    )
    .expect("assertion extracted");
    assert!(
        resp_errors.is_empty(),
        "Response must be valid: {resp_errors:?}"
    );

    // Step 5: claims come from the OUTER assertion (Recipient = our ACS, eID LoA
    // >= minimum, Issuer bound to the RD); the conflicting <Advice> is ignored.
    let mut assn_errors = Vec::new();
    let claims = validate_assertion_at(
        &doc,
        assertion_node,
        &ValidateAssertionOpts {
            dv_entity_id: &dv_entity_id(),
            expected_recipient: Some(&acs_url()),
            expected_issuer: Some(&rd_entity_id()),
            private_keys: &[],
            minimum_loa: Some(MINIMUM_LOA),
            expected_service_uuid: None,
        },
        &mut assn_errors,
    );
    assert!(
        assn_errors.is_empty(),
        "Assertion must validate: {assn_errors:?}"
    );
    let claims = claims.expect("claims present");
    assert_eq!(
        claims.service_uuid.as_deref(),
        Some("f847dc11-ac24-47b2-84a8-a057440ce56d")
    );
}

#[test]
fn signed_artifact_response_issuer_mismatch_rejected() {
    // eID §7.6.1: a validly RD-signed ArtifactResponse whose Issuer is bound to a
    // different EntityID than the pinned RD is rejected.
    let rd_key = load_key("rd-signing-1");
    let soap = signed_artifact_response_soap(&rd_key);

    let doc = parse(&soap).unwrap();
    let root = doc.document_element();
    let art_node = unwrap_soap(&doc, root).expect("SOAP body unwrapped");

    let mut errors = Vec::new();
    validate_artifact_response_at(
        &doc,
        art_node,
        &ValidateArtifactResponseOpts {
            trusted_keys: std::slice::from_ref(&rd_key),
            expected_in_response_to: None,
            expected_issuer: Some(&EntityId::from_static("urn:test:not-the-rd")),
        },
        &mut errors,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("ArtifactResponse Issuer mismatch")),
        "expected issuer mismatch, got: {errors:?}"
    );
}

#[test]
fn signed_artifact_response_untrusted_signer_rejected() {
    // The ArtifactResponse is RD-signed, but we present an unrelated trusted key
    // (a different fixture cert): the envelope signature must NOT verify.
    let rd_key = load_key("rd-signing-1");
    let other_key = load_key("dv-signing-1");
    let soap = signed_artifact_response_soap(&rd_key);

    let art = validate_artifact_response(&soap, std::slice::from_ref(&other_key), None);
    assert!(
        !art.valid,
        "ArtifactResponse signed by RD must not verify against an unrelated key"
    );
}

// ===========================================================================
// §7.6.3.3: SubjectConfirmation
// ===========================================================================

/// §7.6.3.3: `SubjectConfirmationData/@NotBefore` MUST NOT be used. A sender
/// adding it is not following the profile, and honouring it would widen the
/// ~2-minute bearer window, so it fails closed.
#[test]
fn assertion_subject_confirmation_not_before_rejected() {
    let xml = AssertionBuilder::default().build().replace(
        r#"<saml:SubjectConfirmationData NotOnOrAfter="#,
        &format!(
            r#"<saml:SubjectConfirmationData NotBefore="{}" NotOnOrAfter="#,
            ts(-Duration::minutes(1))
        ),
    );
    let errs = validate(&xml).errors;
    assert!(
        errs.iter().any(|e| e.contains("@NotBefore")),
        "expected the forbidden-NotBefore rejection, got: {errs:?}"
    );
}

/// §7.6.3.3: SubjectConfirmation Method MUST be bearer.
#[test]
fn assertion_non_bearer_method() {
    let xml = AssertionBuilder {
        subject_method: "urn:oasis:names:tc:SAML:2.0:cm:holder-of-key".into(),
        ..Default::default()
    }
    .build();
    let errs = validate(&xml).errors;
    assert!(
        errs.iter().any(|e| e.contains("bearer")),
        "expected bearer error, got: {errs:?}"
    );
}

/// §7.6.3.5 rule 2: Recipient MUST match the DV's ACS URL.
#[test]
fn assertion_recipient_mismatch() {
    let xml = AssertionBuilder {
        scd_recipient: "https://evil.example.com/acs".into(),
        ..Default::default()
    }
    .build();
    let errs = validate(&xml).errors;
    assert!(
        errs.iter().any(|e| e.contains("Recipient mismatch")),
        "expected recipient error, got: {errs:?}"
    );
}

/// §7.6.3.5 rule 2: Empty expected_recipient skips Recipient check.
#[test]
fn assertion_empty_expected_recipient_skips_check() {
    let xml = AssertionBuilder {
        scd_recipient: "https://other.example.com/acs".into(),
        ..Default::default()
    }
    .build();
    let r = validate_assertion(
        &xml,
        ValidateAssertionOpts {
            dv_entity_id: &dv_entity_id(),
            expected_recipient: None,
            expected_issuer: None,
            private_keys: &[],
            minimum_loa: None,
            expected_service_uuid: None,
        },
    );
    let errs = r.errors;
    assert!(
        !errs.iter().any(|e| e.contains("Recipient")),
        "should not check recipient when expected is empty, got: {errs:?}"
    );
}

/// §7.6.3.5 rule 3: SubjectConfirmationData NotOnOrAfter in the past must be rejected.
#[test]
fn assertion_subject_confirmation_expired() {
    let xml = AssertionBuilder {
        scd_not_on_or_after: ts(-Duration::hours(1)),
        ..Default::default()
    }
    .build();
    let errs = validate(&xml).errors;
    assert!(
        errs.iter()
            .any(|e| e.contains("SubjectConfirmation") && e.contains("expired")),
        "expected SubjectConfirmation expiry, got: {errs:?}"
    );
}

/// §7.6.3.5 rule 3: SubjectConfirmationData NotOnOrAfter in the future is valid.
#[test]
fn assertion_subject_confirmation_valid_time() {
    let xml = AssertionBuilder {
        scd_not_on_or_after: ts(Duration::minutes(2)),
        ..Default::default()
    }
    .build();
    let errs = validate(&xml).errors;
    assert!(
        !errs
            .iter()
            .any(|e| e.contains("SubjectConfirmation") && e.contains("expired")),
        "should not be expired, got: {errs:?}"
    );
}

/// §7.6.3 / §9.5: a missing Conditions element must fail closed (the validity
/// window is mandatory, not "nothing to check").
#[test]
fn assertion_missing_conditions_rejected() {
    let xml = strip_element(&AssertionBuilder::default().build(), "saml:Conditions");
    let errs = validate(&xml).errors;
    assert!(
        errs.iter()
            .any(|e| e.contains("missing the required Conditions")),
        "expected missing-Conditions rejection, got: {errs:?}"
    );
}

/// §7.6.3 / §7.6.3.5: a missing SubjectConfirmation must fail closed.
#[test]
fn assertion_missing_subject_confirmation_rejected() {
    let xml = strip_element(
        &AssertionBuilder::default().build(),
        "saml:SubjectConfirmation",
    );
    let errs = validate(&xml).errors;
    assert!(
        errs.iter()
            .any(|e| e.contains("missing the required SubjectConfirmation")),
        "expected missing-SubjectConfirmation rejection, got: {errs:?}"
    );
}

/// §7.6.3 / §9.5: a malformed Conditions @NotBefore must fail closed, not parse
/// to "no constraint".
#[test]
fn assertion_malformed_not_before_rejected() {
    let xml = AssertionBuilder {
        conditions_not_before: "not-a-timestamp".into(),
        ..Default::default()
    }
    .build();
    let errs = validate(&xml).errors;
    assert!(
        errs.iter().any(|e| e.contains("invalid NotBefore")),
        "expected invalid-NotBefore rejection, got: {errs:?}"
    );
}

// ===========================================================================
// §7.6.3.1 / §7.6.3.5 rule 5: AudienceRestriction
// ===========================================================================

/// §7.6.3.5 rule 5: DV's entityID MUST be in Audience.
#[test]
fn assertion_audience_missing_dv() {
    let xml = AssertionBuilder {
        audiences: vec!["urn:other:party".into()],
        ..Default::default()
    }
    .build();
    let errs = validate(&xml).errors;
    assert!(
        errs.iter()
            .any(|e| e.contains("not in AudienceRestriction")),
        "expected audience error, got: {errs:?}"
    );
}

/// §7.6.3.1: Empty audience list means no party can process the assertion.
#[test]
fn assertion_audience_empty() {
    let xml = AssertionBuilder {
        audiences: vec![],
        ..Default::default()
    }
    .build();
    let errs = validate(&xml).errors;
    assert!(
        errs.iter()
            .any(|e| e.contains("not in AudienceRestriction")),
        "expected audience error, got: {errs:?}"
    );
}

/// Replace the builder's single `<saml:AudienceRestriction>` with `body`.
fn with_conditions_body(xml: &str, body: &str) -> String {
    let open = "<saml:AudienceRestriction>";
    let close = "</saml:AudienceRestriction>";
    let start = xml.find(open).expect("restriction present");
    let end = xml[start..]
        .find(close)
        .map(|i| start + i + close.len())
        .expect("close tag");
    let mut out = xml.to_string();
    out.replace_range(start..end, body);
    out
}

/// SAML core §2.5.1.4: every AudienceRestriction must hold independently, so a
/// second one that excludes this DV rejects the assertion even though the first
/// permits it.
#[test]
fn assertion_audience_every_restriction_must_name_dv() {
    let xml = with_conditions_body(
        &AssertionBuilder::default().build(),
        &format!(
            "<saml:AudienceRestriction><saml:Audience>{DV_ENTITY_ID}</saml:Audience></saml:AudienceRestriction>\
             <saml:AudienceRestriction><saml:Audience>urn:other:party</saml:Audience></saml:AudienceRestriction>"
        ),
    );
    let errs = validate(&xml).errors;
    assert!(
        errs.iter()
            .any(|e| e.contains("not in AudienceRestriction")),
        "expected the excluding restriction to reject, got: {errs:?}"
    );
}

/// A stray `<Audience>` directly under Conditions is not an AudienceRestriction
/// and must not grant access.
#[test]
fn assertion_audience_outside_restriction_does_not_count() {
    let xml = with_conditions_body(
        &AssertionBuilder::default().build(),
        &format!("<saml:Audience>{DV_ENTITY_ID}</saml:Audience>"),
    );
    let errs = validate(&xml).errors;
    assert!(
        errs.iter()
            .any(|e| e.contains("not in AudienceRestriction")),
        "expected missing restriction error, got: {errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("unsupported Condition")),
        "expected the stray Audience to be flagged, got: {errs:?}"
    );
}

/// SAML core §2.5.1: a condition the DV does not implement makes the assertion
/// Indeterminate; it is rejected rather than ignored.
#[test]
fn assertion_unknown_condition_is_rejected() {
    let base = AssertionBuilder::default().build();
    for extra in [
        "<saml:OneTimeUse/>",
        "<saml:ProxyRestriction Count=\"0\"/>",
        "<saml:Condition xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"x:Custom\" xmlns:x=\"urn:x\"/>",
        "<x:Mandatory xmlns:x=\"urn:x\"/>",
    ] {
        let xml = with_conditions_body(
            &base,
            &format!(
                "<saml:AudienceRestriction><saml:Audience>{DV_ENTITY_ID}</saml:Audience></saml:AudienceRestriction>{extra}"
            ),
        );
        let errs = validate(&xml).errors;
        assert!(
            errs.iter().any(|e| e.contains("unsupported Condition")),
            "{extra}: expected rejection, got: {errs:?}"
        );
    }
}

/// `<Conditions>` is read only as the Assertion's own child: a copy nested
/// elsewhere neither grants an audience nor supplies the validity window.
#[test]
fn assertion_conditions_must_be_a_direct_child() {
    let xml = AssertionBuilder {
        audiences: vec!["urn:other:party".into()],
        ..Default::default()
    }
    .build()
    .replace(
        "</saml:AttributeStatement>",
        &format!(
            "<saml:Conditions><saml:AudienceRestriction><saml:Audience>{DV_ENTITY_ID}</saml:Audience></saml:AudienceRestriction></saml:Conditions></saml:AttributeStatement>"
        ),
    );
    let errs = validate(&xml).errors;
    assert!(
        errs.iter()
            .any(|e| e.contains("not in AudienceRestriction")),
        "nested Conditions must not count, got: {errs:?}"
    );

    let two = AssertionBuilder::default()
        .build()
        .replace(
            "<saml:AuthnStatement",
            "<saml:Conditions><saml:AudienceRestriction><saml:Audience>urn:other</saml:Audience></saml:AudienceRestriction></saml:Conditions><saml:AuthnStatement",
        );
    let errs = validate(&two).errors;
    assert!(
        errs.iter().any(|e| e.contains("Conditions elements")),
        "two Conditions must be rejected, got: {errs:?}"
    );
}

/// §7.6.3.1: DV's entityID present among multiple audiences is valid.
#[test]
fn assertion_audience_multiple_including_dv() {
    let xml = AssertionBuilder {
        audiences: vec!["urn:test:lc".into(), DV_ENTITY_ID.into()],
        ..Default::default()
    }
    .build();
    let errs = validate(&xml).errors;
    assert!(
        !errs.iter().any(|e| e.contains("AudienceRestriction")),
        "DV in audience list should pass, got: {errs:?}"
    );
}

// ===========================================================================
// §9.5: Time validity (Conditions)
// ===========================================================================

/// §7.6.3 (cardinality 1): the Assertion `@IssueInstant` is mandatory, and a
/// far-future value must not pass (nothing else would ever expire it).
#[test]
fn assertion_issue_instant_required_and_future_bounded() {
    let missing = AssertionBuilder::default()
        .build()
        .replace(&format!(r#" IssueInstant="{}""#, ts(Duration::zero())), "");
    let errs = validate(&missing).errors;
    assert!(
        errs.iter()
            .any(|e| e.contains("Assertion @IssueInstant is missing")),
        "{errs:?}"
    );

    let future = AssertionBuilder::default().build().replace(
        &format!(
            r#"ID="_a1" Version="2.0" IssueInstant="{}""#,
            ts(Duration::zero())
        ),
        &format!(
            r#"ID="_a1" Version="2.0" IssueInstant="{}""#,
            ts(Duration::hours(2))
        ),
    );
    let errs = validate(&future).errors;
    assert!(errs.iter().any(|e| e.contains("in the future")), "{errs:?}");
}

/// §7.6.3 (cardinality 1): AuthnStatement and its `@AuthnInstant` are mandatory.
#[test]
fn assertion_requires_authn_statement_with_instant() {
    let no_instant = AssertionBuilder::default().build().replace(
        &format!(
            r#"<saml:AuthnStatement AuthnInstant="{}">"#,
            ts(Duration::zero())
        ),
        "<saml:AuthnStatement>",
    );
    let errs = validate(&no_instant).errors;
    assert!(
        errs.iter()
            .any(|e| e.contains("AuthnStatement @AuthnInstant is missing")),
        "{errs:?}"
    );

    let no_stmt = strip_element(&AssertionBuilder::default().build(), "saml:AuthnStatement");
    let errs = validate(&no_stmt).errors;
    assert!(
        errs.iter()
            .any(|e| e.contains("missing the required AuthnStatement")),
        "{errs:?}"
    );
}

/// §9.5: Assertion with NotBefore in the future must be rejected.
#[test]
fn assertion_conditions_not_yet_valid() {
    let xml = AssertionBuilder {
        conditions_not_before: ts(Duration::hours(1)),
        ..Default::default()
    }
    .build();
    let errs = validate(&xml).errors;
    assert!(
        errs.iter().any(|e| e.contains("not yet valid")),
        "expected NotBefore error, got: {errs:?}"
    );
}

/// §9.5: Assertion with NotOnOrAfter in the past must be rejected.
#[test]
fn assertion_conditions_expired() {
    let xml = AssertionBuilder {
        conditions_not_on_or_after: ts(-Duration::hours(1)),
        ..Default::default()
    }
    .build();
    let errs = validate(&xml).errors;
    assert!(
        errs.iter()
            .any(|e| e.contains("Assertion") && e.contains("expired")),
        "expected Assertion expired error, got: {errs:?}"
    );
}

/// §9.5: NotBefore within clock skew tolerance should be accepted.
#[test]
fn assertion_conditions_not_before_within_skew() {
    // NotBefore is 10 seconds in the future, skew is 30 seconds, so valid.
    let xml = AssertionBuilder {
        conditions_not_before: ts(Duration::seconds(10)),
        ..Default::default()
    }
    .build();
    let errs = validate(&xml).errors;
    assert!(
        !errs.iter().any(|e| e.contains("not yet valid")),
        "within skew should be accepted, got: {errs:?}"
    );
}

/// §9.5: NotOnOrAfter within clock skew tolerance should be accepted.
#[test]
fn assertion_conditions_not_on_or_after_within_skew() {
    // NotOnOrAfter is 10 seconds in the past, skew is 30 seconds, so still valid.
    let xml = AssertionBuilder {
        conditions_not_on_or_after: ts(-Duration::seconds(10)),
        ..Default::default()
    }
    .build();
    let errs = validate(&xml).errors;
    assert!(
        !errs.iter().any(|e| e.contains("expired")),
        "within skew should be accepted, got: {errs:?}"
    );
}

/// §9.5: Both NotBefore and NotOnOrAfter violations must be reported.
#[test]
fn assertion_conditions_both_violations() {
    let xml = AssertionBuilder {
        conditions_not_before: ts(Duration::hours(2)),
        conditions_not_on_or_after: ts(-Duration::hours(2)),
        ..Default::default()
    }
    .build();
    let errs = validate(&xml).errors;
    assert!(
        errs.iter().any(|e| e.contains("not yet valid")),
        "expected NotBefore error, got: {errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("expired")),
        "expected NotOnOrAfter error, got: {errs:?}"
    );
}

// ===========================================================================
// §7.6.3.4: Claim extraction (NameID, AuthnContext, ServiceUUID)
// ===========================================================================

/// §7.6.3.2: NameID is extracted from Subject.
#[test]
fn assertion_extracts_name_id() {
    let xml = AssertionBuilder::default().build();
    let r = validate(&xml);
    assert!(
        r.errors.is_empty(),
        "valid assertion should have no errors: {:?}",
        r.errors
    );
    let claims = r.claims.expect("claims present when no errors");
    assert_eq!(claims.name_id.as_str(), "user-123");
}

/// §7.6.3: a Subject NameID whose Format is not the transient URI is rejected.
#[test]
fn assertion_non_transient_name_id_rejected() {
    let xml = AssertionBuilder::default().build().replace(
        NAMEID_TRANSIENT,
        "urn:oasis:names:tc:SAML:2.0:nameid-format:persistent",
    );
    let errs = validate(&xml).errors;
    assert!(
        errs.iter().any(|e| e.contains("Subject NameID Format")),
        "expected transient-format rejection, got: {errs:?}"
    );
}

/// §7.6.3 / SAML core §2.2.2: a Subject NameID with no Format attribute is
/// tolerated (Format defaults to "unspecified"). The TVS preprod IdP omits it,
/// and the acting identity comes from the decrypted EncryptedID, not this NameID.
#[test]
fn assertion_name_id_without_format_tolerated() {
    let xml = AssertionBuilder::default()
        .build()
        .replace(&format!(r#" Format="{NAMEID_TRANSIENT}""#), "");
    let errs = validate(&xml).errors;
    assert!(
        !errs.iter().any(|e| e.contains("Subject NameID Format")),
        "absent NameID Format should be tolerated, got: {errs:?}"
    );
}

/// §7.6.3: a missing Assertion @Version fails closed.
#[test]
fn assertion_missing_version_rejected() {
    let xml = AssertionBuilder::default()
        .build()
        .replace(r#" Version="2.0""#, "");
    let errs = validate(&xml).errors;
    assert!(
        errs.iter()
            .any(|e| e.contains("missing the required @Version")),
        "expected missing-Version rejection, got: {errs:?}"
    );
}

/// §7.6.3 (cardinality 1): a missing Subject NameID fails closed.
#[test]
fn assertion_missing_name_id_rejected() {
    let xml = AssertionBuilder {
        name_id: None,
        ..Default::default()
    }
    .build();
    let errs = validate(&xml).errors;
    assert!(
        errs.iter()
            .any(|e| e.contains("missing the required Subject NameID")),
        "expected missing-NameID rejection, got: {errs:?}"
    );
}

/// §7.6.3.4: ServiceUUID attribute is extracted from AttributeStatement.
#[test]
fn assertion_extracts_service_uuid() {
    let xml = AssertionBuilder::default().build();
    let r = validate(&xml);
    assert!(r.errors.is_empty(), "valid assertion: {:?}", r.errors);
    let claims = r.claims.expect("claims present when no errors");
    assert_eq!(
        claims.service_uuid.as_deref(),
        Some("f847dc11-ac24-47b2-84a8-a057440ce56d")
    );
}

/// §7.6.3: Missing AuthnContextClassRef is not an error when no minimum is required.
#[test]
fn assertion_missing_authn_context_no_error() {
    let xml = AssertionBuilder {
        authn_class_ref: None,
        auth_authority: None,
        ..Default::default()
    }
    .build();
    let errs = validate(&xml).errors;
    assert!(errs.is_empty(), "got: {errs:?}");
}

/// §7.6.3.2 / TVS T6: an AuthnContextClassRef below `MINIMUM_LOA` must be
/// rejected. With `MINIMUM_LOA = Low` (Midden), the default builder's
/// `PasswordProtectedTransport` (Basic) is too low.
#[test]
fn assertion_loa_below_minimum_rejected() {
    let xml = AssertionBuilder::default().build();
    let errs = validate_with_loa(&xml).errors;
    assert!(
        errs.iter().any(|e| e.contains("LoA too low")),
        "expected 'LoA too low' error, got: {errs:?}"
    );
}

/// §7.6.3.2: an AuthnContextClassRef equal to `MINIMUM_LOA` is accepted.
#[test]
fn assertion_loa_equal_minimum_accepted() {
    let xml = AssertionBuilder {
        authn_class_ref: Some(
            "urn:oasis:names:tc:SAML:2.0:ac:classes:MobileTwoFactorContract".into(),
        ),
        ..Default::default()
    }
    .build();
    let errs = validate_with_loa(&xml).errors;
    assert!(
        !errs.iter().any(|e| e.contains("LoA")),
        "Low (Midden) should satisfy MINIMUM_LOA, got: {errs:?}"
    );
}

/// §7.6.3.2: an AuthnContextClassRef above `MINIMUM_LOA` is accepted.
/// Uses the eIDAS URL spelling the RD actually emits (eID §10.3).
#[test]
fn assertion_loa_above_minimum_accepted() {
    let xml = AssertionBuilder {
        authn_class_ref: Some("http://eidas.europa.eu/LoA/high".into()),
        ..Default::default()
    }
    .build();
    let errs = validate_with_loa(&xml).errors;
    assert!(
        !errs.iter().any(|e| e.contains("LoA")),
        "High should satisfy MINIMUM_LOA, got: {errs:?}"
    );
}

/// §7.6.3.2 / §10.3: the eIDAS Substantial URL the RD emits maps to
/// `Substantial` (above the `Low` minimum) and is accepted: this is the value a
/// real TVS eIDAS login sends.
#[test]
fn assertion_loa_eidas_substantial_accepted() {
    let xml = AssertionBuilder {
        authn_class_ref: Some("http://eidas.europa.eu/LoA/substantial".into()),
        ..Default::default()
    }
    .build();
    let errs = validate_with_loa(&xml).errors;
    assert!(
        !errs.iter().any(|e| e.contains("LoA")),
        "eIDAS Substantial should satisfy MINIMUM_LOA, got: {errs:?}"
    );
}

/// A LoA URI not listed in eID §10.3 is rejected.
#[test]
fn assertion_loa_unrecognized_uri_rejected() {
    let xml = AssertionBuilder {
        authn_class_ref: Some("urn:bogus:loa".into()),
        ..Default::default()
    }
    .build();
    let errs = validate_with_loa(&xml).errors;
    assert!(
        errs.iter().any(|e| e.contains("Unrecognized LoA")),
        "expected 'Unrecognized LoA' error, got: {errs:?}"
    );
}

/// §7.6.3.2: a missing AuthnContextClassRef IS an error when a minimum is required.
#[test]
fn assertion_missing_authn_context_with_minimum_rejected() {
    let xml = AssertionBuilder {
        authn_class_ref: None,
        auth_authority: None,
        ..Default::default()
    }
    .build();
    let errs = validate_with_loa(&xml).errors;
    assert!(
        errs.iter()
            .any(|e| e.contains("missing the required AuthnContextClassRef")),
        "expected missing-AuthnContextClassRef error, got: {errs:?}"
    );
}

// ===========================================================================
// §7.6.3: Multiple simultaneous errors
// ===========================================================================

/// All validation errors must be collected, not short-circuited.
#[test]
fn assertion_collects_all_errors() {
    let xml = AssertionBuilder {
        subject_method: "urn:wrong".into(),
        scd_recipient: "https://evil.example.com".into(),
        scd_not_on_or_after: ts(-Duration::hours(1)),
        conditions_not_before: ts(Duration::hours(1)),
        conditions_not_on_or_after: ts(-Duration::hours(1)),
        audiences: vec!["urn:other".into()],
        ..Default::default()
    }
    .build();
    let errs = validate(&xml).errors;

    assert!(
        errs.iter().any(|e| e.contains("bearer")),
        "missing bearer: {errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("Recipient mismatch")),
        "missing recipient: {errs:?}"
    );
    assert!(
        errs.iter()
            .any(|e| e.contains("SubjectConfirmation") && e.contains("expired")),
        "missing scd expiry: {errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("not yet valid")),
        "missing NotBefore: {errs:?}"
    );
    assert!(
        errs.iter()
            .any(|e| e.contains("Assertion") && e.contains("expired")),
        "missing conditions expiry: {errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("AudienceRestriction")),
        "missing audience: {errs:?}"
    );
}

/// §7.6.3.5 rule 7: an invalid assertion records errors and yields no claims.
#[test]
fn assertion_invalid_produces_no_claims() {
    let xml = AssertionBuilder {
        audiences: vec!["urn:wrong".into()],
        ..Default::default()
    }
    .build();
    let r = validate(&xml);
    assert!(!r.errors.is_empty());
    assert!(
        r.claims.is_none(),
        "invalid assertion must not produce claims"
    );
}

// ===========================================================================
// §7.6.3: Malformed input
// ===========================================================================

/// Malformed XML must be rejected.
#[test]
fn assertion_malformed_xml() {
    let r = validate("<<<bad xml");
    assert!(r.claims.is_none());
    assert!(r.errors.iter().any(|e| e.contains("XML parse error")));
}

/// Empty string must be rejected.
#[test]
fn assertion_empty_string() {
    let r = validate("");
    assert!(r.claims.is_none());
    assert!(r.errors.iter().any(|e| e.contains("XML parse error")));
}

// ===========================================================================
// §7.6.2 / §7.8: Response status-code variants (second-level codes, cancel)
// ===========================================================================

/// §7.8.1: urn:…:status:Requester error code.
#[test]
fn response_requester_status() {
    let xml = format!(
        r#"<samlp:Response xmlns:samlp="{NS_SAMLP}"><samlp:Status><samlp:StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:Requester"/></samlp:Status></samlp:Response>"#
    );
    let r = validate_response(&xml);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.contains("Requester")));
}

/// §7.8.2: NoAuthnContext secondary status code.
#[test]
fn response_no_authn_context_status() {
    let xml = format!(
        r#"<samlp:Response xmlns:samlp="{NS_SAMLP}"><samlp:Status><samlp:StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:Responder"><samlp:StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:NoAuthnContext"/></samlp:StatusCode></samlp:Status></samlp:Response>"#
    );
    let r = validate_response(&xml);
    assert!(!r.valid);
    assert!(
        r.errors.iter().any(|e| e.contains("NoAuthnContext")),
        "errors: {:?}",
        r.errors
    );
}

/// §7.8.2: RequestDenied secondary status code.
#[test]
fn response_request_denied_status() {
    let xml = format!(
        r#"<samlp:Response xmlns:samlp="{NS_SAMLP}"><samlp:Status><samlp:StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:Responder"><samlp:StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:RequestDenied"/></samlp:StatusCode></samlp:Status></samlp:Response>"#
    );
    let r = validate_response(&xml);
    assert!(!r.valid);
    assert!(
        r.errors.iter().any(|e| e.contains("RequestDenied")),
        "errors: {:?}",
        r.errors
    );
}

/// §7.6.2: a non-Success status invalidates the Response even when an
/// Assertion is present.
#[test]
fn response_error_status_rejected() {
    let xml = format!(
        r#"<samlp:Response xmlns:samlp="{NS_SAMLP}"><samlp:Status><samlp:StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:Responder"/></samlp:Status><saml:Assertion xmlns:saml="{NS_SAML}" ID="_a1">data</saml:Assertion></samlp:Response>"#
    );
    let r = validate_response(&xml);
    assert!(!r.valid);
    // Assertion XML is still extracted even on error (it's used for diagnostics),
    // but the result is not valid
    assert!(r.errors.iter().any(|e| e.contains("Responder")));
}

// ===========================================================================
// §7.6.1: inner-Response extraction from an ArtifactResponse
// ===========================================================================

/// §7.6.1 (quoting SAML-bindings §3.6.6): "Even if the ArtifactResponse's Status
/// indicates Success, it may still not contain a Response if the artifact
/// requester is not authorized or the artifact is no longer valid." That case
/// must be an error, not a silently empty success.
#[test]
fn artifact_response_success_without_response_is_rejected() {
    let soap = soap_wrap(&format!(
        r#"<samlp:ArtifactResponse xmlns:samlp="{NS_SAMLP}" xmlns:saml="{NS_SAML}" ID="_ar1" Version="2.0" IssueInstant="{now}"><saml:Issuer>{RD_ENTITY_ID}</saml:Issuer><samlp:Status><samlp:StatusCode Value="{STATUS_SUCCESS}"/></samlp:Status></samlp:ArtifactResponse>"#,
        now = ts(Duration::zero())
    ));
    let art = validate_artifact_response(&soap, &[], None);
    assert!(
        art.errors
            .iter()
            .any(|e| e.contains("reports Success but carries no Response")),
        "expected the Success-without-Response rejection, got: {:?}",
        art.errors
    );
}

/// §7.6.1: two Response children are ambiguous; we consume only the first, so an
/// appended second one must be rejected rather than ignored.
#[test]
fn artifact_response_with_two_responses_is_rejected() {
    let inner = format!(
        r#"<samlp:Response ID="_r{{n}}" Version="2.0" IssueInstant="{now}"><samlp:Status><samlp:StatusCode Value="{STATUS_SUCCESS}"/></samlp:Status></samlp:Response>"#,
        now = ts(Duration::zero())
    );
    let soap = soap_wrap(&format!(
        r#"<samlp:ArtifactResponse xmlns:samlp="{NS_SAMLP}" xmlns:saml="{NS_SAML}" ID="_ar1" Version="2.0" IssueInstant="{now}"><saml:Issuer>{RD_ENTITY_ID}</saml:Issuer><samlp:Status><samlp:StatusCode Value="{STATUS_SUCCESS}"/></samlp:Status>{first}{second}</samlp:ArtifactResponse>"#,
        now = ts(Duration::zero()),
        first = inner.replace("{n}", "1"),
        second = inner.replace("{n}", "2"),
    ));
    let art = validate_artifact_response(&soap, &[], None);
    assert!(
        art.errors.iter().any(|e| e.contains("2 Response elements")),
        "expected the multiple-Response rejection, got: {:?}",
        art.errors
    );
}

/// §7.6.1: Successful ArtifactResponse extracts inner Response XML preserving
/// namespace prefixes (required for signature verification on the inner Response).
#[test]
fn artifact_response_extracts_inner_response() {
    let inner_response = format!(
        r#"<samlp:Response xmlns:samlp="{NS_SAMLP}" ID="_r1"><samlp:Status><samlp:StatusCode Value="{STATUS_SUCCESS}"/></samlp:Status></samlp:Response>"#
    );
    let soap = soap_wrap(&format!(
        r#"<samlp:ArtifactResponse xmlns:samlp="{NS_SAMLP}" ID="_ar1"><samlp:Status><samlp:StatusCode Value="{STATUS_SUCCESS}"/></samlp:Status>{inner_response}</samlp:ArtifactResponse>"#
    ));
    // Signature will fail, but Response extraction should succeed
    let r = validate_artifact_response(&soap, &[], None);
    assert!(
        r.response_xml.is_some(),
        "inner Response must be extracted, errors: {:?}",
        r.errors
    );
    let resp = r.response_xml.unwrap();
    assert!(
        resp.contains("samlp:Response"),
        "must preserve namespace prefix"
    );
}
