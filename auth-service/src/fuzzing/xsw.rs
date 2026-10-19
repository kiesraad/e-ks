//! XML Signature Wrapping fuzzer for the ACS validation chain.
//!
//! One genuine, RD-signed SOAP ArtifactResponse is built and signed ONCE (an
//! RSA signature, plus the attacker's own forged fragments, at init). The fuzz
//! input is decoded into structural mutations that wrap, move, duplicate or
//! shadow parts of the message, and the real production validation
//! ([`acs::claims_from_soap`]) is run on the result.
//!
//! Oracle: if validation succeeds, the identity it yields MUST be the genuine
//! one. Any other accepted identity is an XSW bypass and panics.

use crate::{
    config::AuthConfig,
    handlers::acs::claims_from_soap,
    keys::{CertificatePem, KeyPair, KeySet, PrivateKeyPem, key_pair_paths},
    saml::{
        constants::{
            EID_ACTING_SUBJECT_ID, EID_SERVICE_UUID, NAMEID_PERSISTENT, NAMEID_TRANSIENT, NS_SAML,
            NS_SAMLP, STATUS_SUCCESS, SUBJECT_CONFIRMATION_BEARER,
        },
        crypto::sign,
        idp_metadata::IdpMetadata,
        validation::Claims,
    },
    state::AuthServiceState,
    types::{EndpointUrl, EntityId, MessageId, ServiceUuid},
};
use arbitrary::{Arbitrary, Unstructured};
use chrono::{Duration, Utc};
use secrecy::ExposeSecret;
use std::{path::PathBuf, sync::LazyLock};

const DV: &str = "urn:test:dv";
const RD: &str = "urn:test:rd";
const ACS: &str = "https://dv.example.com/saml/sp/acs";
const SERVICE_UUID: &str = "f847dc11-ac24-47b2-84a8-a057440ce56d";
const LOA: &str = "http://eidas.europa.eu/LoA/substantial";
const RESOLVE_ID: &str = "_resolve1";
const BOUND_ID: &str = "_bound1";

/// The genuine identity. Acceptance of anything else is the finding.
const GENUINE_NAME_ID: &str = "transient-subject";
const GENUINE_BSN: &str = "900070341";
/// The identity the attacker tries to smuggle into the session.
const ATTACKER_NAME_ID: &str = "ATTACKER-CHOSEN-VICTIM";
const ATTACKER_BSN: &str = "000000000";

/// Cap on mutations per input: enough to stack an XSW, bounded for speed.
const MAX_MUTATIONS: usize = 24;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn load_key(name: &str) -> KeyPair {
    let paths = key_pair_paths(&fixtures_dir(), name);
    let read = |p: &std::path::Path| std::fs::read_to_string(p).expect("read fixture");
    let cert = CertificatePem::parse(read(&paths.cert)).expect("fixture cert parses");
    KeyPair::from_pem(cert, PrivateKeyPem::new(read(&paths.key)))
}

/// A SAML timestamp `offset` from the fixture's build time. The validity
/// windows are wide (minutes), so one build serves the whole run without the
/// freshness checks ageing out in practice.
fn ts(offset: Duration) -> String {
    (Utc::now() + offset)
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// A DV state that trusts `rd-signing-1` as the RD signing key and holds the DV
/// decryption keys, built without I/O.
fn build_state() -> AuthServiceState {
    let mut cfg = AuthConfig::default().with_certs_dir(fixtures_dir());
    cfg.dv.entity_id = EntityId::from_static(DV);
    cfg.dv.service_uuid = ServiceUuid::from_static(SERVICE_UUID);
    cfg.dv.acs_url = EndpointUrl::from_base_url(ACS, "ACS").expect("ACS URL");
    let keys = KeySet {
        signing: vec![load_key("dv-signing-1")],
        encryption: vec![load_key("dv-encryption-1"), load_key("dv-encryption-2")],
    };
    AuthServiceState::new(cfg, keys, None, None)
}

fn rd_metadata() -> IdpMetadata {
    let endpoint = |path: &str| {
        EndpointUrl::from_metadata(&format!("https://rd.example.com/{path}"), path)
            .expect("RD endpoint")
    };
    IdpMetadata {
        entity_id: EntityId::from_static(RD),
        sso_url: endpoint("sso"),
        ars_url: endpoint("ars"),
        slo_url: endpoint("slo"),
        signing_keys: vec![load_key("rd-signing-1")],
        cache_duration: None,
        valid_until: None,
    }
}

/// An `<saml:EncryptedID>` carrying `bsn`, wrapped to the DV's own (public)
/// encryption cert exactly as the RD does it. The attacker can build this too:
/// the DV encryption cert is public, so secrecy is not the defence; the RD
/// signature over the whole message is.
fn encrypted_acting_subject(dv_keys: &KeySet, bsn: &str) -> String {
    use bergshamra_enc::{EncContext, encrypt::encrypt};
    use bergshamra_keys::{KeysManager, loader};

    let recipient = dv_keys.encryption.first().expect("a DV encryption key");
    let key_name = recipient.key_name.as_str();
    let name_id = format!(
        r#"<saml:NameID xmlns:saml="{NS_SAML}" Format="{NAMEID_PERSISTENT}" NameQualifier="urn:nl-eid-gdi:1.0:id:legacy-BSN">{bsn}</saml:NameID>"#
    );
    let template = format!(
        r#"<saml:EncryptedID xmlns:saml="{NS_SAML}" xmlns:xenc="http://www.w3.org/2001/04/xmlenc#" xmlns:ds="http://www.w3.org/2000/09/xmldsig#"><xenc:EncryptedData Type="http://www.w3.org/2001/04/xmlenc#Element"><xenc:EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes256-cbc"/><ds:KeyInfo><xenc:EncryptedKey Recipient="{DV}"><xenc:EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#rsa-oaep-mgf1p"/><ds:KeyInfo><ds:KeyName>{key_name}</ds:KeyName></ds:KeyInfo><xenc:CipherData><xenc:CipherValue></xenc:CipherValue></xenc:CipherData></xenc:EncryptedKey></ds:KeyInfo><xenc:CipherData><xenc:CipherValue></xenc:CipherValue></xenc:CipherData></xenc:EncryptedData></saml:EncryptedID>"#
    );
    let cert = loader::load_x509_cert_pem(recipient.cert_pem.as_str().as_bytes())
        .expect("load DV encryption cert")
        .with_name(key_name);
    let mut mgr = KeysManager::new();
    mgr.add_key(cert);
    encrypt(&EncContext::new(mgr), &template, name_id.as_bytes()).expect("encrypt NameID")
}

/// An Assertion with the given ID/identity, valid for our DV.
fn assertion(id: &str, name_id: &str, enc_id: &str) -> String {
    let issued = ts(Duration::zero());
    let scd = ts(Duration::minutes(2));
    let nb = ts(-Duration::minutes(5));
    let noa = ts(Duration::minutes(5));
    format!(
        r#"<saml:Assertion ID="{id}" Version="2.0" IssueInstant="{issued}"><saml:Issuer>{RD}</saml:Issuer><saml:Subject><saml:NameID Format="{NAMEID_TRANSIENT}">{name_id}</saml:NameID><saml:SubjectConfirmation Method="{SUBJECT_CONFIRMATION_BEARER}"><saml:SubjectConfirmationData NotOnOrAfter="{scd}" Recipient="{ACS}" InResponseTo="{BOUND_ID}"/></saml:SubjectConfirmation></saml:Subject><saml:Conditions NotBefore="{nb}" NotOnOrAfter="{noa}"><saml:AudienceRestriction><saml:Audience>{DV}</saml:Audience></saml:AudienceRestriction></saml:Conditions><saml:AuthnStatement AuthnInstant="{issued}"><saml:AuthnContext><saml:AuthnContextClassRef>{LOA}</saml:AuthnContextClassRef></saml:AuthnContext></saml:AuthnStatement><saml:AttributeStatement><saml:Attribute Name="{EID_SERVICE_UUID}"><saml:AttributeValue>{SERVICE_UUID}</saml:AttributeValue></saml:Attribute><saml:Attribute Name="{EID_ACTING_SUBJECT_ID}"><saml:AttributeValue>{enc_id}</saml:AttributeValue></saml:Attribute></saml:AttributeStatement></saml:Assertion>"#
    )
}

/// A Response wrapping `assertion`.
fn response(id: &str, assertion: &str) -> String {
    let issued = ts(Duration::zero());
    format!(
        r#"<samlp:Response ID="{id}" Version="2.0" IssueInstant="{issued}" Destination="{ACS}" InResponseTo="{BOUND_ID}"><saml:Issuer>{RD}</saml:Issuer><samlp:Status><samlp:StatusCode Value="{STATUS_SUCCESS}"/></samlp:Status>{assertion}</samlp:Response>"#
    )
}

/// The RD enveloped-signature template (empty digest/sig), over `#_art1`.
fn rd_signature(cert: &crate::keys::CertificateBase64) -> String {
    format!(
        r##"<dsig:Signature xmlns:dsig="http://www.w3.org/2000/09/xmldsig#"><dsig:SignedInfo><dsig:CanonicalizationMethod Algorithm="http://www.w3.org/2001/10/xml-exc-c14n#"/><dsig:SignatureMethod Algorithm="http://www.w3.org/2001/04/xmldsig-more#rsa-sha256"/><dsig:Reference URI="#_art1"><dsig:Transforms><dsig:Transform Algorithm="http://www.w3.org/2000/09/xmldsig#enveloped-signature"/><dsig:Transform Algorithm="http://www.w3.org/2001/10/xml-exc-c14n#"/></dsig:Transforms><dsig:DigestMethod Algorithm="http://www.w3.org/2001/04/xmlenc#sha256"/><dsig:DigestValue></dsig:DigestValue></dsig:Reference></dsig:SignedInfo><dsig:SignatureValue></dsig:SignatureValue><dsig:KeyInfo><dsig:X509Data><dsig:X509Certificate>{cert}</dsig:X509Certificate></dsig:X509Data></dsig:KeyInfo></dsig:Signature>"##
    )
}

/// Everything built once: the signed SOAP, the forged fragments the mutations
/// splice in, and the state to validate against.
struct Fixture {
    state: AuthServiceState,
    rd: IdpMetadata,
    /// The genuine, RD-signed SOAP envelope (the baseline input).
    signed_soap: String,
    /// A forged Assertion with the attacker identity (ID `_assertion_evil`).
    attacker_assertion: String,
    /// The same, reusing the genuine Assertion ID `_a1` (for ID collisions).
    attacker_assertion_same_id: String,
    /// A forged Response (ID `_resp_evil`) wrapping [`Self::attacker_assertion`].
    attacker_response: String,
}

static FIXTURE: LazyLock<Fixture> = LazyLock::new(|| {
    let state = build_state();
    let rd = rd_metadata();
    let rd_key = load_key("rd-signing-1");

    let genuine_enc = encrypted_acting_subject(state.dv_keys(), GENUINE_BSN);
    let attacker_enc = encrypted_acting_subject(state.dv_keys(), ATTACKER_BSN);

    let genuine_assertion = assertion("_a1", GENUINE_NAME_ID, &genuine_enc);
    let genuine_response = response("_resp1", &genuine_assertion);
    let artifact_response = format!(
        r#"<samlp:ArtifactResponse xmlns:samlp="{NS_SAMLP}" xmlns:saml="{NS_SAML}" ID="_art1" Version="2.0" IssueInstant="{issued}" InResponseTo="{RESOLVE_ID}"><saml:Issuer>{RD}</saml:Issuer>{sig}<samlp:Status><samlp:StatusCode Value="{STATUS_SUCCESS}"/></samlp:Status>{response}</samlp:ArtifactResponse>"#,
        issued = ts(Duration::zero()),
        sig = rd_signature(&rd_key.cert_base64),
        response = genuine_response,
    );
    let signed = sign(&artifact_response, &rd_key.key_pem).expect("RD signing");
    let signed_soap = format!(
        r#"<soapenv:Envelope xmlns:soapenv="http://schemas.xmlsoap.org/soap/envelope/"><soapenv:Body>{signed}</soapenv:Body></soapenv:Envelope>"#
    );

    let attacker_assertion = assertion("_assertion_evil", ATTACKER_NAME_ID, &attacker_enc);
    let attacker_assertion_same_id = assertion("_a1", ATTACKER_NAME_ID, &attacker_enc);
    let attacker_response = response("_resp_evil", &attacker_assertion);

    let fixture = Fixture {
        state,
        rd,
        signed_soap,
        attacker_assertion,
        attacker_assertion_same_id,
        attacker_response,
    };
    // The baseline MUST validate to the genuine identity, or every rejection
    // below is vacuous.
    match fixture.validate(&fixture.signed_soap) {
        Ok(claims) => assert!(
            is_genuine(&claims),
            "baseline signed message did not yield the genuine identity"
        ),
        Err(e) => panic!("baseline signed message was rejected: {e:?}"),
    }
    fixture
});

impl Fixture {
    fn validate(&self, soap: &str) -> Result<Claims, crate::handlers::acs::Rejection> {
        claims_from_soap(
            &self.state,
            &self.rd,
            soap,
            &MessageId::parse(RESOLVE_ID).expect("resolve id"),
            &MessageId::parse(BOUND_ID).expect("bound id"),
        )
    }
}

/// Whether `claims` is exactly the genuine identity (and nothing attacker-chosen
/// slipped into either the Subject NameID or the decrypted ActingSubjectID).
fn is_genuine(claims: &Claims) -> bool {
    let name_ok = claims.name_id.as_str() == GENUINE_NAME_ID;
    let bsn_ok = match &claims.acting_subject_id {
        Some(s) => s.value.expose_secret() == GENUINE_BSN,
        None => true,
    };
    name_ok && bsn_ok
}

// ---------------------------------------------------------------------------
// Mutations
// ---------------------------------------------------------------------------

/// Where a cloned/forged element is spliced.
#[derive(Arbitrary, Debug, Clone, Copy)]
enum Slot {
    /// Just before the genuine Response (a new first child of ArtifactResponse).
    BeforeResponse,
    /// Just after the Status (before the genuine Response).
    AfterStatus,
    /// Inside a new `<samlp:Extensions>` on the ArtifactResponse.
    Extensions,
    /// Inside a new `<saml:Advice>` on the genuine Assertion.
    Advice,
    /// Inside a new `<ds:Object>` on the signature.
    DsObject,
    /// As an extra child of the SOAP Body, before the ArtifactResponse.
    SoapBody,
    /// Inside a new `<soapenv:Header>` on the envelope.
    SoapHeader,
    /// Just before the genuine Assertion, inside the genuine Response.
    BeforeAssertion,
}

/// The attacker fragment a mutation inserts.
#[derive(Arbitrary, Debug, Clone, Copy)]
enum Forged {
    Assertion,
    AssertionSameId,
    Response,
}

#[derive(Arbitrary, Debug, Clone, Copy)]
enum InjectStyle {
    Comment,
    Cdata,
    CharRef,
}

/// One structural edit to the signed SOAP string.
#[derive(Arbitrary, Debug, Clone)]
enum Mutation {
    /// Splice a forged fragment into a wrapping/sibling position.
    Insert { forged: Forged, slot: Slot },
    /// Give a genuine element a duplicate ID (`_art1`/`_resp1`/`_a1`) under some
    /// ID-ish attribute name.
    DuplicateId { which: Which, attr: IdAttr },
    /// Add an `xml:id` alias to a genuine element.
    AddXmlId { which: Which },
    /// Wrap the whole signed ArtifactResponse in a forged element.
    WrapRoot,
    /// Rename the ArtifactResponse root element (local name / namespace).
    ChangeRoot(RootChange),
    /// Inject a comment/CDATA/charref splitting the genuine NameID text, trying
    /// to append the attacker's.
    SplitNameId(InjectStyle),
    /// Inject a comment/CDATA into a security attribute value (Recipient/ID).
    PoisonAttribute(InjectStyle),
    /// Append a second `<ds:Signature>` over a forged `#_resp_evil`.
    AddSignature,
    /// Add a second `<dsig:Reference>` to the genuine signature.
    AddReference,
    /// Shadow the `saml`/`samlp` prefix to a bogus URI on a wrapper.
    ShadowNamespace,
    /// Prepend a DOCTYPE, optionally with an internal entity.
    AddDoctype { entity: bool },
    /// Collapse insignificant whitespace / newlines in the envelope.
    Reformat(Reformat),
    /// Reorder the attributes on the ArtifactResponse start tag.
    ReorderRootAttrs,
    /// Splice a forged element that uses a leading-colon name or a `:xmlns`
    /// default-namespace declaration (the invalid-QName class the parser-diff
    /// harness flags), to confirm neither parser read of it yields an identity.
    LeadingColonForgery,
}

#[derive(Arbitrary, Debug, Clone, Copy)]
enum Which {
    ArtifactResponse,
    Response,
    Assertion,
}

#[derive(Arbitrary, Debug, Clone, Copy)]
enum IdAttr {
    Id,
    IdLower,
    AssertionId,
    XmlId,
}

#[derive(Arbitrary, Debug, Clone, Copy)]
enum RootChange {
    LocalName,
    Namespace,
    LeadingColon,
}

#[derive(Arbitrary, Debug, Clone, Copy)]
enum Reformat {
    StripInterTag,
    DoubleNewlines,
    TabsBetweenTags,
}

impl Which {
    fn id(self) -> &'static str {
        match self {
            Which::ArtifactResponse => "_art1",
            Which::Response => "_resp1",
            Which::Assertion => "_a1",
        }
    }
}

impl IdAttr {
    fn name(self) -> &'static str {
        match self {
            IdAttr::Id => "Id",
            IdAttr::IdLower => "id",
            IdAttr::AssertionId => "AssertionID",
            IdAttr::XmlId => "xml:id",
        }
    }
}

impl Fixture {
    fn forged(&self, f: Forged) -> &str {
        match f {
            Forged::Assertion => &self.attacker_assertion,
            Forged::AssertionSameId => &self.attacker_assertion_same_id,
            Forged::Response => &self.attacker_response,
        }
    }
}

/// Apply one mutation to `xml`. A mutation whose anchor is absent (removed by an
/// earlier mutation) is a no-op; the point is a valid-ish document to validate.
fn apply(xml: String, m: &Mutation, fx: &Fixture) -> String {
    match m {
        Mutation::Insert { forged, slot } => apply_insert(xml, fx.forged(*forged), *slot),
        Mutation::DuplicateId { which, attr } => xml.replacen(
            r#"<samlp:Status>"#,
            &format!(
                r#"<samlp:Extensions {}="{}"/><samlp:Status>"#,
                attr.name(),
                which.id()
            ),
            1,
        ),
        Mutation::AddXmlId { which } => {
            let anchor = format!(r#"ID="{}""#, which.id());
            xml.replacen(&anchor, &format!(r#"{anchor} xml:id="_alias""#), 1)
        }
        Mutation::WrapRoot => xml
            .replacen(
                r#"<samlp:ArtifactResponse "#,
                r#"<samlp:ArtifactResponse xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol" ID="_art1"><samlp:InnerArtifactResponse "#,
                1,
            )
            .replacen(
                r#"</samlp:ArtifactResponse>"#,
                r#"</samlp:InnerArtifactResponse></samlp:ArtifactResponse>"#,
                1,
            ),
        Mutation::ChangeRoot(change) => apply_change_root(xml, *change),
        Mutation::SplitNameId(style) => apply_split_name_id(xml, *style),
        Mutation::PoisonAttribute(style) => apply_poison_attribute(xml, *style),
        Mutation::AddSignature => apply_add_signature(xml),
        Mutation::AddReference => xml.replacen(
            r#"</dsig:Reference>"#,
            r##"</dsig:Reference><dsig:Reference URI="#_resp_evil"><dsig:Transforms><dsig:Transform Algorithm="http://www.w3.org/2000/09/xmldsig#enveloped-signature"/></dsig:Transforms><dsig:DigestMethod Algorithm="http://www.w3.org/2001/04/xmlenc#sha256"/><dsig:DigestValue></dsig:DigestValue></dsig:Reference>"##,
            1,
        ),
        Mutation::ShadowNamespace => xml.replacen(
            r#"<samlp:Status>"#,
            r#"<samlp:Status xmlns:saml="urn:evil" xmlns:samlp="urn:evil">"#,
            1,
        ),
        Mutation::AddDoctype { entity } => {
            let dt = if *entity {
                "<!DOCTYPE soapenv:Envelope [<!ENTITY x \"y\">]>"
            } else {
                "<!DOCTYPE soapenv:Envelope>"
            };
            format!("{dt}{xml}")
        }
        Mutation::Reformat(kind) => apply_reformat(xml, *kind),
        Mutation::ReorderRootAttrs => xml.replacen(
            r#"ID="_art1" Version="2.0""#,
            r#"Version="2.0" ID="_art1""#,
            1,
        ),
        Mutation::LeadingColonForgery => {
            // A forged Response reached by a leading-colon element name and a
            // `:xmlns` default-ns declaration (both invalid QNames roxmltree
            // reinterprets). Spliced after the Status, outside the signature.
            let frag = format!(r#"<:wrap :xmlns="{NS_SAMLP}">{}</:wrap>"#, fx.attacker_response);
            xml.replacen("</samlp:Status>", &format!("</samlp:Status>{frag}"), 1)
        }
    }
}

fn apply_reformat(xml: String, kind: Reformat) -> String {
    match kind {
        Reformat::StripInterTag => xml.replace(">\n<", "><").replace(">  <", "><"),
        Reformat::DoubleNewlines => xml.replace("><", ">\n\n<"),
        Reformat::TabsBetweenTags => xml.replace("><", ">\t<"),
    }
}

/// Splice a forged fragment into a wrapping/sibling position.
fn apply_insert(xml: String, frag: &str, slot: Slot) -> String {
    let (anchor, replacement) = match slot {
        Slot::BeforeResponse | Slot::AfterStatus => {
            ("</samlp:Status>", format!("</samlp:Status>{frag}"))
        }
        Slot::Extensions => (
            "</samlp:Status>",
            format!("</samlp:Status><samlp:Extensions>{frag}</samlp:Extensions>"),
        ),
        Slot::Advice => (
            "<saml:Subject>",
            format!("<saml:Advice>{frag}</saml:Advice><saml:Subject>"),
        ),
        Slot::DsObject => (
            "</dsig:Signature>",
            format!("<dsig:Object>{frag}</dsig:Object></dsig:Signature>"),
        ),
        Slot::SoapBody => ("<soapenv:Body>", format!("<soapenv:Body>{frag}")),
        Slot::SoapHeader => (
            "<soapenv:Body>",
            format!("<soapenv:Header>{frag}</soapenv:Header><soapenv:Body>"),
        ),
        Slot::BeforeAssertion => ("<saml:Assertion ", format!("{frag}<saml:Assertion ")),
    };
    xml.replacen(anchor, &replacement, 1)
}

fn apply_change_root(xml: String, change: RootChange) -> String {
    match change {
        RootChange::LocalName => xml
            .replacen("<samlp:ArtifactResponse ", "<samlp:Response ", 1)
            .replacen("</samlp:ArtifactResponse>", "</samlp:Response>", 1),
        RootChange::Namespace => xml.replacen(
            r#"xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol""#,
            r#"xmlns:samlp="urn:oasis:names:tc:SAML:2.0:assertion""#,
            1,
        ),
        RootChange::LeadingColon => xml
            .replacen("<samlp:ArtifactResponse ", "<:ArtifactResponse ", 1)
            .replacen("</samlp:ArtifactResponse>", "</:ArtifactResponse>", 1),
    }
}

fn apply_split_name_id(xml: String, style: InjectStyle) -> String {
    let anchor = format!(">{GENUINE_NAME_ID}<");
    let injected = match style {
        InjectStyle::Comment => format!(">{GENUINE_NAME_ID}<!--x-->{ATTACKER_NAME_ID}<"),
        InjectStyle::Cdata => format!(">{GENUINE_NAME_ID}<![CDATA[{ATTACKER_NAME_ID}]]><"),
        InjectStyle::CharRef => format!(">{GENUINE_NAME_ID}&#32;{ATTACKER_NAME_ID}<"),
    };
    xml.replacen(&anchor, &injected, 1)
}

fn apply_poison_attribute(xml: String, style: InjectStyle) -> String {
    let injected = match style {
        InjectStyle::Comment => format!(r#"Recipient="{ACS}"><!--x-->"#),
        InjectStyle::Cdata => format!(r#"Recipient="{ACS}">"#),
        InjectStyle::CharRef => format!(r#"Recipient="{ACS}&#32;">"#),
    };
    xml.replacen(&format!(r#"Recipient="{ACS}">"#), &injected, 1)
}

fn apply_add_signature(xml: String) -> String {
    let rd_key = load_key("rd-signing-1");
    let sig = rd_signature(&rd_key.cert_base64)
        .replace("#_art1", "#_resp_evil")
        .replace("<dsig:", "<dsig2:")
        .replace("</dsig:", "</dsig2:")
        .replacen(
            "<dsig2:Signature ",
            "<dsig2:Signature xmlns:dsig2=\"http://www.w3.org/2000/09/xmldsig#\" ",
            1,
        );
    xml.replacen("</samlp:Status>", &format!("</samlp:Status>{sig}"), 1)
}

/// The harness: panic only if validation accepts a non-genuine identity.
pub(crate) fn run(data: &[u8]) {
    let fx = &*FIXTURE;
    let mut u = Unstructured::new(data);
    let Ok(mutations) = Vec::<Mutation>::arbitrary(&mut u) else {
        return;
    };
    let mut xml = fx.signed_soap.clone();
    for m in mutations.iter().take(MAX_MUTATIONS) {
        xml = apply(xml, m, fx);
    }
    if let Ok(claims) = fx.validate(&xml)
        && !is_genuine(&claims)
    {
        panic!(
            "XSW BYPASS: validation accepted a non-genuine identity.\n\
             name_id={:?}, acting_bsn={:?}\nmutations={:?}\nxml={xml}",
            claims.name_id.as_str(),
            claims
                .acting_subject_id
                .as_ref()
                .map(|s| s.value.expose_secret()),
            mutations,
        );
    }
}
