//! The validators, run against real TVS `ArtifactResponse` messages (see
//! `fixtures/tvs/README.md`) rather than XML this repository composed, so the
//! RD's actual namespace prefixes, attribute order and element whitespace are
//! covered.
//!
//! Not exercised: signatures (the samples name their key by fingerprint and eID
//! §9.2 allows only keys from verified metadata, of which we hold none for their
//! environment) and the decrypted identity (`EncryptedID` is wrapped to a DV key
//! we do not hold, so `acting_subject_id` is always `None` here).

use auth_service::{
    bindings::soap::unwrap_soap,
    saml::{
        constants::{NS_SAMLP, STATUS_SUCCESS},
        loa::MINIMUM_LOA,
        validation::{
            Claims, ValidateArtifactResponseOpts, ValidateAssertionOpts, ValidateResponseOpts,
            validate_artifact_response_at, validate_assertion_at, validate_response_at,
        },
        xml_parser::{Document, NodeId, find_child, parse},
    },
    types::{EndpointUrl, EntityId, MessageId, ServiceUuid},
};
use chrono::{DateTime, Utc};

// Literal values from the captured messages, so each test configures the DV the
// RD actually addressed.

const RD: &str = "urn:nl-eid-gdi:1.0:RD:00000004000000149000:entities:9002";

mod success {
    pub const FILE: &str = "artifact_response_success.xml";
    pub const ACS: &str = "https://poc-1.uzi.bavod.nl/acs";
    pub const DV: &str = "urn:nl-eid-gdi:1.0:DV:00000002006756402002:entities:9999";
    pub const SERVICE_UUID: &str = "464f504d-5857-5946-304f-494449414a4d";
    pub const LOA: &str = "http://eidas.europa.eu/LoA/substantial";
    pub const AD: &str = "urn:nl-eid-gdi:1.0:AD:00000004166909913000:entities:9002";
    /// Outer Subject NameID: a bare hex string with no `@Format`.
    pub const NAME_ID: &str = "64b0d194095940008ffa142b12444c01";
    pub const IN_RESPONSE_TO: &str =
        "_837f3790c95cd6ca4cb815edd30f583d0fe6313a2ab0b53ccc65f972b571fabcf70924f49e4d8deb0e";

    /// Values only inside the `<saml:Advice>` AD assertion; claims must never
    /// surface one.
    pub mod advice {
        pub const NAME_ID: &str = "2cbd6231-4257-44ae-aa87-b4bfc25e232f";
        pub const IN_RESPONSE_TO: &str = "_a7efd80d17a4f064dde50b9cd78aca7b";
        pub const RECIPIENT: &str = "https://pp2.toegang.overheid.nl/foam/saml/acs";
    }
}

mod cluster {
    pub const FILE: &str = "artifact_response_cluster.xml";
    pub const ACS: &str = "https://endpoint.example/acs";
    pub const DV: &str = "urn:nl-eid-gdi:1.0:DV:00000002003182447001:entities:9888";
    pub const SERVICE_UUID: &str = "c57ec6e6-baba-472d-9db4-5ef8cf5e29c8";
    pub const LOA: &str = "http://eidas.europa.eu/LoA/high";
    pub const NAME_ID: &str = "3efa072a56034ebe939516e30a979a00";
}

fn load(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/tvs")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Move every timestamp in `xml` into the current validity window: the first
/// instant becomes "now" and the rest keep their offset from it, so the RD's
/// two-minute windows still bracket the present. Only attribute values that
/// parse as a timestamp are touched.
fn shift_timestamps(xml: &str) -> String {
    let now = Utc::now();
    let mut anchor: Option<DateTime<Utc>> = None;
    let mut out = String::with_capacity(xml.len());

    for (i, part) in xml.split('"').enumerate() {
        if i > 0 {
            out.push('"');
        }
        match part.parse::<DateTime<Utc>>() {
            Ok(instant) => {
                let base = *anchor.get_or_insert(instant);
                let shifted = now + (instant - base);
                // The outer messages are whole seconds, the AD assertion is not.
                let format = if part.contains('.') {
                    "%Y-%m-%dT%H:%M:%S%.3fZ"
                } else {
                    "%Y-%m-%dT%H:%M:%SZ"
                };
                out.push_str(&shifted.format(format).to_string());
            }
            Err(_) => out.push_str(part),
        }
    }
    out
}

fn artifact_response(doc: &Document) -> NodeId {
    unwrap_soap(doc, doc.document_element()).expect("sample has a SOAP Body with one child")
}

/// The inner Response, reached without [`validate_artifact_response_at`], which
/// would first fail on the signature we cannot verify.
fn inner_response(doc: &Document, art: NodeId) -> NodeId {
    find_child(doc, art, NS_SAMLP, "Response").expect("sample carries an inner Response")
}

/// Run the Response (§7.6.2) and Assertion (§7.6.3) validators over a sample.
fn validate_sample(
    doc: &Document,
    acs: &str,
    dv: &str,
    service_uuid: &str,
) -> (Option<Claims>, Vec<String>) {
    let mut errors = Vec::new();

    let acs = EndpointUrl::from_metadata(acs, "ACS").expect("fixture ACS URL");
    let dv = EntityId::parse(dv).expect("fixture DV EntityID");
    let rd = EntityId::parse(RD).expect("fixture RD EntityID");
    let service_uuid = ServiceUuid::parse(service_uuid).expect("fixture ServiceUUID");

    let art = artifact_response(doc);
    let response = inner_response(doc, art);

    let assertion = validate_response_at(
        doc,
        response,
        &ValidateResponseOpts {
            expected_destination: Some(&acs),
            expected_issuer: Some(&rd),
        },
        &mut errors,
    );

    let claims = assertion.and_then(|a| {
        validate_assertion_at(
            doc,
            a,
            &ValidateAssertionOpts {
                dv_entity_id: &dv,
                expected_recipient: Some(&acs),
                expected_issuer: Some(&rd),
                private_keys: &[],
                minimum_loa: Some(MINIMUM_LOA),
                expected_service_uuid: Some(&service_uuid),
            },
            &mut errors,
        )
    });

    (claims, errors)
}

/// [`validate_sample`] on a time-shifted sample, asserting nothing was recorded.
fn validate_shifted(name: &str, acs: &str, dv: &str, service_uuid: &str) -> Claims {
    let xml = shift_timestamps(&load(name));
    let doc = parse(&xml).expect("real TVS ArtifactResponse parses");
    let (claims, errors) = validate_sample(&doc, acs, dv, service_uuid);
    assert!(
        errors.is_empty(),
        "{name}: a real TVS message was rejected: {errors:#?}"
    );
    claims.expect("a run with no errors yields claims")
}

#[test]
fn success_sample_validates_and_yields_the_expected_claims() {
    let claims = validate_shifted(
        success::FILE,
        success::ACS,
        success::DV,
        success::SERVICE_UUID,
    );

    assert_eq!(claims.name_id.as_str(), success::NAME_ID);
    assert_eq!(claims.service_uuid.as_deref(), Some(success::SERVICE_UUID));
    assert_eq!(
        claims.in_response_to.as_ref().map(MessageId::as_str),
        Some(success::IN_RESPONSE_TO)
    );
    // No DV decryption key here.
    assert!(claims.acting_subject_id.is_none());
    assert!(claims.legal_subject_id.is_none());
}

#[test]
fn success_sample_claims_come_from_the_outer_assertion_not_from_advice() {
    // `<saml:Advice>` holds a separately signed AD assertion with its own
    // Subject, Recipient, InResponseTo and ActingSubjectID. eID §9.1 makes it
    // evidence only. The values differ, which is what makes the pruning
    // observable rather than merely asserted.
    let xml = load(success::FILE);
    for value in [
        success::advice::NAME_ID,
        success::advice::IN_RESPONSE_TO,
        success::advice::RECIPIENT,
    ] {
        assert!(
            xml.contains(value),
            "fixture no longer carries the Advice assertion this test relies on ({value})"
        );
    }

    let claims = validate_shifted(
        success::FILE,
        success::ACS,
        success::DV,
        success::SERVICE_UUID,
    );

    assert_ne!(claims.name_id.as_str(), success::advice::NAME_ID);
    assert_ne!(
        claims.in_response_to.as_ref().map(MessageId::as_str),
        Some(success::advice::IN_RESPONSE_TO)
    );
    // A Recipient read from the Advice would already have failed the ACS check.
}

#[test]
fn success_sample_matches_the_loa_despite_whitespace_around_the_uri() {
    // The RD pretty-prints, so the element text is the URI plus a newline and
    // the closing tag's indentation. The §10.3 lookup is an exact match.
    let raw = load(success::FILE);
    assert!(
        raw.contains(&format!("{}\n", success::LOA)),
        "fixture no longer carries the trailing whitespace this test relies on"
    );

    let claims = validate_shifted(
        success::FILE,
        success::ACS,
        success::DV,
        success::SERVICE_UUID,
    );

    assert_eq!(
        claims.authn_context_class_ref.as_deref(),
        Some(success::LOA)
    );
    assert_eq!(
        claims.authenticating_authority.as_deref(),
        Some(success::AD)
    );
}

#[test]
fn success_sample_unshifted_fails_only_the_time_checks() {
    // Guards `shift_timestamps`: on the pristine 2022 bytes only the
    // time-bounded checks may fail, so the shift cannot be hiding a structural
    // failure in the tests above.
    const TIME_ERROR_MARKERS: &[&str] = &[
        "is stale",
        "is in the future",
        "expired",
        "not yet valid",
        "outside the usable range",
    ];

    let xml = load(success::FILE);
    let doc = parse(&xml).expect("real TVS ArtifactResponse parses");
    let (_, errors) = validate_sample(&doc, success::ACS, success::DV, success::SERVICE_UUID);

    let structural: Vec<&String> = errors
        .iter()
        .filter(|e| !TIME_ERROR_MARKERS.iter().any(|m| e.contains(m)))
        .collect();
    assert!(
        structural.is_empty(),
        "expected only time-dependent errors on the unshifted sample, got: {structural:#?}"
    );
    assert!(
        !errors.is_empty(),
        "the unshifted sample is years old and must fail its time checks"
    );
}

#[test]
fn cluster_sample_validates_with_two_audiences() {
    // A cluster message names the LC and the DV in one `AudienceRestriction`;
    // §7.6.3.5 rule 5 only requires ours to be among them. The LC entry comes
    // first and carries the pretty-printing's leading whitespace.
    let claims = validate_shifted(
        cluster::FILE,
        cluster::ACS,
        cluster::DV,
        cluster::SERVICE_UUID,
    );

    assert_eq!(claims.name_id.as_str(), cluster::NAME_ID);
    assert_eq!(
        claims.authn_context_class_ref.as_deref(),
        Some(cluster::LOA)
    );
}

#[test]
fn login_cancelled_sample_is_rejected_by_the_response_status_check() {
    // The artifact layer reports Success; the failure is the inner Response's
    // nested `Responder` / `AuthnFailed`, which `handle_acs` maps to
    // `AuthFailure::Cancelled`.
    let xml = load("artifact_response_login_cancelled.xml");
    let doc = parse(&xml).expect("real TVS login-cancelled message parses");
    let art = artifact_response(&doc);
    let response = inner_response(&doc, art);

    let mut errors = Vec::new();
    let assertion = validate_response_at(
        &doc,
        response,
        &ValidateResponseOpts {
            expected_destination: None,
            expected_issuer: None,
        },
        &mut errors,
    );

    assert!(
        assertion.is_none(),
        "a cancelled login carries no Assertion"
    );
    let status_error = errors
        .iter()
        .find(|e| e.starts_with("Response status:"))
        .unwrap_or_else(|| panic!("no Response status error recorded: {errors:#?}"));
    assert!(
        status_error.contains("status:Responder")
            && status_error.contains("status:AuthnFailed")
            && status_error.contains("Authentication cancelled"),
        "the status error must carry both StatusCode levels and the StatusMessage, \
         got: {status_error}"
    );
}

#[test]
fn request_denied_sample_is_rejected_at_the_artifact_layer() {
    // §7.6.1: the status is on the ArtifactResponse and there is no inner
    // Response at all.
    let xml = load("artifact_response_request_denied.xml");
    let doc = parse(&xml).expect("real TVS request-denied message parses");
    let art = artifact_response(&doc);

    assert!(
        find_child(&doc, art, NS_SAMLP, "Response").is_none(),
        "a denied request carries no inner Response"
    );

    let mut errors = Vec::new();
    let response = validate_artifact_response_at(
        &doc,
        art,
        &ValidateArtifactResponseOpts {
            // No metadata for this environment, so the signature check fails
            // too; this test is about the status check reporting both levels.
            trusted_keys: &[],
            expected_in_response_to: None,
            expected_issuer: None,
        },
        &mut errors,
    );

    assert!(response.is_none());
    assert!(
        errors.iter().any(|e| {
            e.starts_with("ArtifactResponse status:")
                && e.contains("status:Requester")
                && e.contains("status:RequestDenied")
        }),
        "expected an ArtifactResponse status error naming both levels: {errors:#?}"
    );
}

#[test]
fn pre_44_digid_sample_is_rejected_for_carrying_no_eid_identity() {
    // The pre-4.4 koppelvlak put a sector-coded plaintext BSN in the Subject
    // NameID and carried no eID attributes. This crate only speaks 4.4, and
    // nothing about the message's status betrays it — it is a successful login.
    let xml = load("artifact_response_digid_pre44.xml");
    assert!(xml.contains(STATUS_SUCCESS));
    assert!(xml.contains("s00000000:900029365"));

    let xml = shift_timestamps(&xml);
    let doc = parse(&xml).expect("pre-4.4 DigiD message parses");
    let art = artifact_response(&doc);
    let response = inner_response(&doc, art);

    let idp = EntityId::parse("https://was-preprod1.digid.nl/saml/idp/metadata").expect("issuer");
    let dv = EntityId::parse("https://siam1.test.anoigo.nl/aselectserver/server").expect("DV");
    let acs = EndpointUrl::from_metadata(
        "https://siam1.test.anoigo.nl/aselectserver/server/saml20_assertion_digid",
        "ACS",
    )
    .expect("fixture ACS URL");
    let service_uuid = ServiceUuid::parse(success::SERVICE_UUID).expect("ServiceUUID");

    let mut errors = Vec::new();
    let assertion = validate_response_at(
        &doc,
        response,
        &ValidateResponseOpts {
            expected_destination: None,
            expected_issuer: Some(&idp),
        },
        &mut errors,
    )
    .expect("the pre-4.4 message still has an Assertion element");

    let claims = validate_assertion_at(
        &doc,
        assertion,
        &ValidateAssertionOpts {
            dv_entity_id: &dv,
            expected_recipient: Some(&acs),
            expected_issuer: Some(&idp),
            private_keys: &[],
            minimum_loa: Some(MINIMUM_LOA),
            expected_service_uuid: Some(&service_uuid),
        },
        &mut errors,
    );

    assert!(
        claims.is_none(),
        "a pre-4.4 assertion must not yield claims"
    );
    assert!(
        errors.iter().any(|e| e.contains("ServiceUUID")),
        "a pre-4.4 message carries no ServiceUUID and must be rejected for it: {errors:#?}"
    );
}
