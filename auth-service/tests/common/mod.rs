//! Shared scaffolding for the auth-service integration/security tests.
//!
//! A subdirectory module (`common/mod.rs`) is deliberately used so cargo does
//! not compile this as its own test binary. Each test binary pulls in only a
//! subset of these helpers, so unused items are expected per binary.
#![allow(dead_code)]

use auth_service::{
    bindings::soap::unwrap_soap,
    keys::{CertificateBase64, CertificatePem, KeyPair, PrivateKeyPem, key_pair_paths},
    saml::{
        constants::{NS_SAML, NS_SAMLP, NS_SOAP, STATUS_SUCCESS, SUBJECT_CONFIRMATION_BEARER},
        loa::MINIMUM_LOA,
        validation::{
            Claims, ValidateArtifactResponseOpts, ValidateAssertionOpts, ValidateResponseOpts,
            validate_artifact_response_at, validate_assertion_at, validate_response_at,
        },
        xml_parser::parse,
    },
    types::{EndpointUrl, EntityId, MessageId},
};
use chrono::{Duration, Utc};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Test entity identifiers (re-using the library's SAML constants where one
// already exists, so the test scaffold never drifts from production values).
// ---------------------------------------------------------------------------

pub const RD: &str = "urn:test:rd";
pub const DV: &str = "urn:test:dv";
pub const ACS: &str = "https://dv.example.com/acs";

/// The same three as domain types, for the validator options.
pub fn rd_entity_id() -> EntityId {
    EntityId::from_static(RD)
}

pub fn dv_entity_id() -> EntityId {
    EntityId::from_static(DV)
}

pub fn acs_url() -> EndpointUrl {
    EndpointUrl::from_metadata(ACS, "ACS").expect("test ACS URL")
}
pub const SUCCESS: &str = STATUS_SUCCESS;
pub const BEARER: &str = SUBJECT_CONFIRMATION_BEARER;
pub const SAML: &str = NS_SAML;
pub const SAMLP: &str = NS_SAMLP;

// ---------------------------------------------------------------------------
// Generic helpers
// ---------------------------------------------------------------------------

/// Load a keypair (by fixture base name) from the committed TVS fixtures.
///
/// Reads the files synchronously (the library's loaders are async) so the many
/// plain `#[test]` functions here need no runtime.
pub fn load_key(name: &str) -> KeyPair {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let paths = key_pair_paths(&dir, name);
    let read = |path: &std::path::Path| {
        std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
    };
    let cert = CertificatePem::parse(read(&paths.cert)).expect("fixture cert parses");
    KeyPair::from_pem(cert, PrivateKeyPem::new(read(&paths.key)))
}

/// A SAML timestamp (`%Y-%m-%dT%H:%M:%SZ`) at `offset` from now.
pub fn ts(offset: Duration) -> String {
    (Utc::now() + offset)
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// The inline `ds:Signature` template (empty digest/sig) the signer fills,
/// matching what `xml_builder` embeds in real messages.
pub fn inline_signature(ref_id: &str, cert_b64: &CertificateBase64) -> String {
    format!(
        r##"<dsig:Signature xmlns:dsig="http://www.w3.org/2000/09/xmldsig#"><dsig:SignedInfo><dsig:CanonicalizationMethod Algorithm="http://www.w3.org/2001/10/xml-exc-c14n#"/><dsig:SignatureMethod Algorithm="http://www.w3.org/2001/04/xmldsig-more#rsa-sha256"/><dsig:Reference URI="#{ref_id}"><dsig:Transforms><dsig:Transform Algorithm="http://www.w3.org/2000/09/xmldsig#enveloped-signature"/><dsig:Transform Algorithm="http://www.w3.org/2001/10/xml-exc-c14n#"/></dsig:Transforms><dsig:DigestMethod Algorithm="http://www.w3.org/2001/04/xmlenc#sha256"/><dsig:DigestValue></dsig:DigestValue></dsig:Reference></dsig:SignedInfo><dsig:SignatureValue></dsig:SignatureValue><dsig:KeyInfo><dsig:X509Data><dsig:X509Certificate>{cert_b64}</dsig:X509Certificate></dsig:X509Data></dsig:KeyInfo></dsig:Signature>"##
    )
}

/// Wrap `body` in a `soapenv:` SOAP envelope, as real TVS does.
pub fn soap_envelope(body: &str) -> String {
    format!(
        r#"<soapenv:Envelope xmlns:soapenv="{NS_SOAP}"><soapenv:Body>{body}</soapenv:Body></soapenv:Envelope>"#
    )
}

// ---------------------------------------------------------------------------
// XSW scaffolding (shared by the two XML-Signature-Wrapping suites)
// ---------------------------------------------------------------------------

/// A full, valid `Response` + `Assertion` for `name_id` (every check passes for
/// our DV). Namespaces are inherited from the enclosing ArtifactResponse.
pub fn response(id_suffix: &str, name_id: &str) -> String {
    let issued = ts(Duration::zero());
    let nb = ts(-Duration::minutes(5));
    let noa = ts(Duration::minutes(5));
    let scd = ts(Duration::minutes(2));
    format!(
        r#"<samlp:Response ID="_resp{id_suffix}" Version="2.0" IssueInstant="{issued}" Destination="{ACS}"><saml:Issuer>{RD}</saml:Issuer><samlp:Status><samlp:StatusCode Value="{SUCCESS}"/></samlp:Status><saml:Assertion ID="_a{id_suffix}" Version="2.0" IssueInstant="{issued}"><saml:Issuer>{RD}</saml:Issuer><saml:Subject><saml:NameID Format="urn:oasis:names:tc:SAML:2.0:nameid-format:transient">{name_id}</saml:NameID><saml:SubjectConfirmation Method="{BEARER}"><saml:SubjectConfirmationData NotOnOrAfter="{scd}" Recipient="{ACS}"/></saml:SubjectConfirmation></saml:Subject><saml:Conditions NotBefore="{nb}" NotOnOrAfter="{noa}"><saml:AudienceRestriction><saml:Audience>{DV}</saml:Audience></saml:AudienceRestriction></saml:Conditions><saml:AuthnStatement AuthnInstant="{issued}"><saml:AuthnContext><saml:AuthnContextClassRef>http://eidas.europa.eu/LoA/substantial</saml:AuthnContextClassRef></saml:AuthnContext></saml:AuthnStatement></saml:Assertion></samlp:Response>"#
    )
}

/// Outcome of [`run_chain`]: the NameID the chain accepted (`None` when any
/// stage rejected) plus every recorded validation error, so security tests can
/// assert both that a forgery was rejected *and* why.
#[derive(Debug)]
pub struct ChainResult {
    pub accepted: Option<String>,
    pub errors: Vec<String>,
}

/// Drive the exact handler chain (single parse, node navigation) over a SOAP
/// envelope.
pub fn run_chain(soap: &str, rd_key: &KeyPair) -> ChainResult {
    let mut errors = Vec::new();
    let rejected = |errors: Vec<String>| ChainResult {
        accepted: None,
        errors,
    };

    let Ok(doc) = parse(soap) else {
        return rejected(vec!["XML parse error".to_string()]);
    };
    let root = doc.document_element();
    let Some(art_node) = unwrap_soap(&doc, root) else {
        return rejected(vec!["failed to unwrap SOAP envelope".to_string()]);
    };

    let response_node = validate_artifact_response_at(
        &doc,
        art_node,
        &ValidateArtifactResponseOpts {
            trusted_keys: std::slice::from_ref(rd_key),
            expected_in_response_to: None,
            expected_issuer: Some(&rd_entity_id()),
        },
        &mut errors,
    );
    if !errors.is_empty() {
        return rejected(errors);
    }
    let Some(response_node) = response_node else {
        return rejected(vec!["no Response extracted".to_string()]);
    };

    let assertion_node = validate_response_at(
        &doc,
        response_node,
        &ValidateResponseOpts {
            expected_destination: Some(&acs_url()),
            expected_issuer: Some(&rd_entity_id()),
        },
        &mut errors,
    );
    if !errors.is_empty() {
        return rejected(errors);
    }
    let Some(assertion_node) = assertion_node else {
        return rejected(vec!["no Assertion extracted".to_string()]);
    };

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
        &mut errors,
    );
    ChainResult {
        accepted: claims.map(|c| c.name_id.into_string()),
        errors,
    }
}

// ---------------------------------------------------------------------------
// String-input validation wrappers (test-only).
//
// Production navigates the single parsed tree via the `_at` entry points; these
// parse-then-delegate wrappers live here (rather than in the library) so the
// crate's public API carries only the node-based validators it actually uses.
// ---------------------------------------------------------------------------

pub struct ResponseResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub assertion_xml: Option<String>,
}

/// Parse a Response and validate it (no recipient/issuer binding). The inner
/// Assertion's source is returned whenever one was extracted, valid or not
/// (some tests assert on it for rejected documents).
pub fn validate_response(response_xml: &str) -> ResponseResult {
    let doc = match parse(response_xml) {
        Ok(d) => d,
        Err(e) => {
            return ResponseResult {
                valid: false,
                errors: vec![format!("XML parse error: {e}")],
                assertion_xml: None,
            };
        }
    };
    let root = doc.document_element();
    let mut errors = Vec::new();
    let assertion = validate_response_at(
        &doc,
        root,
        &ValidateResponseOpts {
            expected_destination: None,
            expected_issuer: None,
        },
        &mut errors,
    );
    ResponseResult {
        valid: errors.is_empty(),
        assertion_xml: assertion.and_then(|n| doc.node_source(n).map(str::to_string)),
        errors,
    }
}

pub struct ArtifactResponseResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub response_xml: Option<String>,
}

/// Parse a SOAP-wrapped ArtifactResponse and validate it. The inner Response's
/// source is returned whenever one was extracted, valid or not (some tests
/// assert on it for rejected documents).
pub fn validate_artifact_response(
    soap_xml: &str,
    trusted_keys: &[KeyPair],
    expected_in_response_to: Option<&MessageId>,
) -> ArtifactResponseResult {
    let doc = match parse(soap_xml) {
        Ok(d) => d,
        Err(e) => {
            return ArtifactResponseResult {
                valid: false,
                errors: vec![format!("XML parse error: {e}")],
                response_xml: None,
            };
        }
    };
    let root = doc.document_element();
    let Some(art_node) = unwrap_soap(&doc, root) else {
        return ArtifactResponseResult {
            valid: false,
            errors: vec!["Failed to unwrap SOAP envelope".to_string()],
            response_xml: None,
        };
    };
    let mut errors = Vec::new();
    let response = validate_artifact_response_at(
        &doc,
        art_node,
        &ValidateArtifactResponseOpts {
            trusted_keys,
            expected_in_response_to,
            // The string wrapper focuses on the other checks; Issuer binding has
            // its own coverage (run_chain and the dedicated integration tests).
            expected_issuer: None,
        },
        &mut errors,
    );
    ArtifactResponseResult {
        valid: errors.is_empty(),
        response_xml: response.and_then(|n| doc.node_source(n).map(str::to_string)),
        errors,
    }
}

/// Test-side bundle of [`validate_assertion_at`]'s two outputs.
pub struct AssertionResult {
    pub errors: Vec<String>,
    pub claims: Option<Claims>,
}

/// Parse an Assertion and validate it against `opts`.
pub fn validate_assertion(assertion_xml: &str, opts: ValidateAssertionOpts<'_>) -> AssertionResult {
    let doc = match parse(assertion_xml) {
        Ok(d) => d,
        Err(e) => {
            return AssertionResult {
                errors: vec![format!("XML parse error: {e}")],
                claims: None,
            };
        }
    };
    let root = doc.document_element();
    let mut errors = Vec::new();
    let claims = validate_assertion_at(&doc, root, &opts, &mut errors);
    AssertionResult { errors, claims }
}
