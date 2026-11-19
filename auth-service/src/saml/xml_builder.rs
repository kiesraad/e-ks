//! XML document builders using `askama` templates (under `templates/saml/`).
//!
//! Signed messages embed an inline `dsig:Signature` (empty DigestValue/
//! SignatureValue plus the signer's cert) that [`crate::saml::crypto::sign`]
//! fills in place. Templated values are XML-escaped by askama; raw embedded XML
//! (the SOAP body) is emitted with `|safe`.

use askama::Template;

use crate::{
    error::Result,
    keys::{CertificateBase64, KeyName},
    types::{Artifact, EndpointUrl, EntityId, MessageId, NameId, ServiceUuid},
};

// ---------------------------------------------------------------------------
// SAML message builders
// ---------------------------------------------------------------------------

/// Inputs to [`build_authn_request`].
#[derive(Template)]
#[template(path = "saml/authn_request.xml")]
pub struct AuthnRequestArgs<'a> {
    pub id: &'a MessageId,
    pub issue_instant: &'a str,
    pub destination: &'a EndpointUrl,
    pub issuer: &'a EntityId,
    /// Optional explicit ACS URL. When set, emitted as
    /// `@AssertionConsumerServiceURL` + `@ProtocolBinding` (HTTP-Artifact)
    /// *instead of* the index. eID §7.3 forbids this against the real TVS, so it
    /// is only used in the `Test` environment against the standalone TVS mock,
    /// which resolves the SP callback from the request.
    pub acs_url: Option<&'a EndpointUrl>,
    pub intended_audience: &'a EntityId,
    pub service_uuid: &'a ServiceUuid,
    /// Optional minimum `AuthnContextClassRef` URI. When set, emitted as a
    /// `RequestedAuthnContext` with `Comparison="minimum"`, asking the RD to
    /// authenticate at this level or higher (eID §7.6.3.2 / TVS T6). See
    /// [`LevelOfAssurance::as_request_uri`](crate::saml::loa::LevelOfAssurance::as_request_uri).
    pub requested_loa_uri: Option<&'a str>,
    /// Optional pre-selected AD/BVD EntityID. When set, emitted as
    /// `Scoping/IDPList/IDPEntry@ProviderID` per eID §7.3.
    pub preselected_ad_entity_id: Option<&'a EntityId>,
    /// The signing certificate, embedded in the inline KeyInfo.
    pub signing_cert_base64: &'a CertificateBase64,
}

/// Build a signed-shape AuthnRequest per eID §7.3 (signature filled by
/// [`crypto::sign`](crate::saml::crypto::sign)).
///
/// The template emits `@AssertionConsumerServiceIndex="0"`, the index of the
/// single `AssertionConsumerService` in our DV metadata
/// (`templates/saml/dv_metadata.xml`); §7.3 forbids
/// `@AssertionConsumerServiceURL` against the real TVS, see
/// [`AuthnRequestArgs::acs_url`] for the Test-only exception. Extensions carry
/// IntendedAudience + ServiceUUID (§7.3.1.1); ForceAuthn=true (§7.7); optional
/// Scoping/IDPList pre-selects an AD.
pub fn build_authn_request(a: AuthnRequestArgs<'_>) -> Result<String> {
    Ok(a.render()?)
}

/// Inputs to [`build_artifact_resolve`].
#[derive(Template)]
#[template(path = "saml/artifact_resolve.xml")]
pub struct ArtifactResolveArgs<'a> {
    pub id: &'a MessageId,
    pub issue_instant: &'a str,
    pub destination: &'a EndpointUrl,
    pub issuer: &'a EntityId,
    pub artifact: &'a Artifact,
    pub signing_cert_base64: &'a CertificateBase64,
}

/// Build a signed-shape ArtifactResolve per eID §7.5 (sent over the mTLS SOAP
/// back-channel, §9.4).
pub fn build_artifact_resolve(a: ArtifactResolveArgs<'_>) -> Result<String> {
    Ok(a.render()?)
}

/// Inputs to [`build_logout_request`].
#[derive(Template)]
#[template(path = "saml/logout_request.xml")]
pub struct LogoutRequestArgs<'a> {
    pub id: &'a MessageId,
    pub issue_instant: &'a str,
    pub destination: &'a EndpointUrl,
    pub issuer: &'a EntityId,
    pub name_id: &'a NameId,
    pub signing_cert_base64: &'a CertificateBase64,
}

/// Build a signed-shape LogoutRequest per eID §7.7.1 (SP-initiated only).
pub fn build_logout_request(a: LogoutRequestArgs<'_>) -> Result<String> {
    Ok(a.render()?)
}

// ---------------------------------------------------------------------------
// DV (SP) metadata
// ---------------------------------------------------------------------------

/// One certificate published as a `<md:KeyDescriptor>` in the DV metadata.
///
/// A named struct rather than a `(&str, &str)` pair: the thumbprint and the
/// certificate body are both base64-ish text, so a swapped pair would produce
/// metadata that is well-formed and wrong.
#[derive(Clone, Copy)]
pub struct PublishedKey<'a> {
    pub key_name: &'a KeyName,
    pub cert_base64: &'a CertificateBase64,
}

/// Inputs to [`build_dv_metadata`].
pub struct DvMetadataArgs<'a> {
    pub entity_id: &'a EntityId,
    pub acs_url: &'a EndpointUrl,
    pub slo_url: &'a EndpointUrl,
    pub service_name: &'a str,
    pub service_uuid: &'a ServiceUuid,
    pub metadata_id: &'a MessageId,
    /// The metadata signing certificate (inline signature).
    pub signing_cert_base64: &'a CertificateBase64,
    pub signing_keys: &'a [PublishedKey<'a>],
    pub encryption_keys: &'a [PublishedKey<'a>],
}

struct KeyDescriptorView<'a> {
    use_: &'a str,
    key_name: &'a KeyName,
    cert_base64: &'a CertificateBase64,
}

#[derive(Template)]
#[template(path = "saml/dv_metadata.xml")]
struct DvMetadataTemplate<'a> {
    entity_id: &'a EntityId,
    acs_url: &'a EndpointUrl,
    slo_url: &'a EndpointUrl,
    service_name: &'a str,
    service_uuid: &'a ServiceUuid,
    metadata_id: &'a MessageId,
    signing_cert_base64: &'a CertificateBase64,
    key_descriptors: Vec<KeyDescriptorView<'a>>,
}

/// Build signed-shape DV SP metadata per eID §8.3. KeyDescriptors are emitted
/// for every signing key (use="signing") then every encryption key
/// (use="encryption"); the inline signature is filled by
/// [`crypto::sign`](crate::saml::crypto::sign).
pub fn build_dv_metadata<'a>(a: DvMetadataArgs<'a>) -> Result<String> {
    let descriptors = |use_: &'a str, keys: &'a [PublishedKey<'a>]| {
        keys.iter().map(move |key| KeyDescriptorView {
            use_,
            key_name: key.key_name,
            cert_base64: key.cert_base64,
        })
    };
    let key_descriptors: Vec<KeyDescriptorView<'a>> = descriptors("signing", a.signing_keys)
        .chain(descriptors("encryption", a.encryption_keys))
        .collect();
    Ok(DvMetadataTemplate {
        entity_id: a.entity_id,
        acs_url: a.acs_url,
        slo_url: a.slo_url,
        service_name: a.service_name,
        service_uuid: a.service_uuid,
        metadata_id: a.metadata_id,
        signing_cert_base64: a.signing_cert_base64,
        key_descriptors,
    }
    .render()?)
}

// ---------------------------------------------------------------------------
// SOAP envelope
// ---------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "saml/soap_envelope.xml")]
struct SoapEnvelope<'a> {
    body: &'a str,
}

/// Wrap raw body XML in a SOAP envelope (mTLS SOAP back-channel, eID §9.4).
pub fn wrap_in_soap_envelope(body_xml: &str) -> Result<String> {
    Ok(SoapEnvelope { body: body_xml }.render()?)
}

// ---------------------------------------------------------------------------
// ID and timestamp helpers
// ---------------------------------------------------------------------------

/// The current UTC time formatted as a SAML `IssueInstant` (`YYYY-MM-DDThh:mm:ssZ`).
pub fn now_utc() -> String {
    jiff::Timestamp::now()
        .strftime("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cert(b64: &str) -> CertificateBase64 {
        CertificateBase64::parse(b64).expect("test certificate body")
    }

    fn test_cert() -> CertificateBase64 {
        cert("Y2VydA==")
    }

    fn id(value: &str) -> MessageId {
        MessageId::parse(value).expect("test message id")
    }

    fn dv() -> EntityId {
        EntityId::from_static("urn:test:dv")
    }

    fn url(value: &str) -> EndpointUrl {
        EndpointUrl::from_base_url(value, "endpoint").expect("test endpoint")
    }

    fn service_uuid() -> ServiceUuid {
        ServiceUuid::from_static("f847dc11-ac24-47b2-84a8-a057440ce56d")
    }

    #[test]
    fn build_authn_request_produces_valid_xml() {
        let xml = build_authn_request(AuthnRequestArgs {
            id: &id("_test123"),
            issue_instant: "2025-01-01T00:00:00Z",
            destination: &url("https://rd.example.com/sso"),
            issuer: &dv(),
            acs_url: None,
            intended_audience: &dv(),
            service_uuid: &service_uuid(),
            requested_loa_uri: Some(
                "urn:oasis:names:tc:SAML:2.0:ac:classes:MobileTwoFactorContract",
            ),
            preselected_ad_entity_id: None,
            signing_cert_base64: &test_cert(),
        })
        .unwrap();

        assert!(xml.contains("AuthnRequest"));
        // RequestedAuthnContext asks for the minimum LoA or higher (eID §7.6.3.2 / TVS T6).
        assert!(xml.contains("<samlp:RequestedAuthnContext Comparison=\"minimum\">"));
        assert!(xml.contains(
            "<saml:AuthnContextClassRef>urn:oasis:names:tc:SAML:2.0:ac:classes:MobileTwoFactorContract</saml:AuthnContextClassRef>"
        ));
        assert!(xml.contains("ID=\"_test123\""));
        assert!(xml.contains("Version=\"2.0\""));
        assert!(xml.contains("ForceAuthn=\"true\""));
        assert!(xml.contains("<saml:Issuer>urn:test:dv</saml:Issuer>"));
        assert!(xml.contains("f847dc11-ac24-47b2-84a8-a057440ce56d"));
        // Default to the metadata index; no explicit ACS URL.
        assert!(xml.contains("AssertionConsumerServiceIndex=\"0\""));
        assert!(!xml.contains("AssertionConsumerServiceURL"));
        // Inline signature template present, referencing the document ID.
        assert!(xml.contains("<dsig:Signature"));
        assert!(xml.contains("URI=\"#_test123\""));
        assert!(xml.contains("<dsig:X509Certificate>Y2VydA==</dsig:X509Certificate>"));
        assert!(!xml.contains("Scoping"));
    }

    #[test]
    fn build_authn_request_emits_acs_url_when_set() {
        let xml = build_authn_request(AuthnRequestArgs {
            id: &id("_test123"),
            issue_instant: "2025-01-01T00:00:00Z",
            destination: &url("https://rd.example.com/sso"),
            issuer: &dv(),
            acs_url: Some(&url("https://pr-7.preview.example.test/saml/sp/acs")),
            intended_audience: &dv(),
            service_uuid: &service_uuid(),
            requested_loa_uri: None,
            preselected_ad_entity_id: None,
            signing_cert_base64: &test_cert(),
        })
        .unwrap();

        // The explicit ACS URL replaces the index (mutually exclusive per SAML
        // core §3.4.1) and pins the HTTP-Artifact binding.
        assert!(xml.contains(
            "AssertionConsumerServiceURL=\"https://pr-7.preview.example.test/saml/sp/acs\""
        ));
        assert!(
            xml.contains("ProtocolBinding=\"urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Artifact\"")
        );
        assert!(!xml.contains("AssertionConsumerServiceIndex"));
    }

    #[test]
    fn build_authn_request_emits_scoping_when_preselected() {
        let ad_entity_id = crate::config::PreselectedAd::DigiD
            .entity_id(false)
            .expect("DigiD resolves to an EntityID");
        let xml = build_authn_request(AuthnRequestArgs {
            id: &id("_test123"),
            issue_instant: "2025-01-01T00:00:00Z",
            destination: &url("https://rd.example.com/sso"),
            issuer: &dv(),
            acs_url: None,
            intended_audience: &dv(),
            service_uuid: &service_uuid(),
            requested_loa_uri: None,
            preselected_ad_entity_id: Some(&ad_entity_id),
            signing_cert_base64: &test_cert(),
        })
        .unwrap();

        assert!(xml.contains("<samlp:Scoping>"));
        assert!(xml.contains("<samlp:IDPList>"));
        assert!(xml.contains(&format!("<samlp:IDPEntry ProviderID=\"{ad_entity_id}\"")));
        // Scoping must appear after Extensions per SAML schema.
        let ext_end = xml.find("</samlp:Extensions>").expect("Extensions present");
        let scoping_start = xml.find("<samlp:Scoping>").expect("Scoping present");
        assert!(ext_end < scoping_start);
    }

    #[test]
    fn build_artifact_resolve_contains_artifact() {
        let xml = build_artifact_resolve(ArtifactResolveArgs {
            id: &id("_res1"),
            issue_instant: "2025-01-01T00:00:00Z",
            destination: &url("https://rd.example.com/ars"),
            issuer: &dv(),
            artifact: &Artifact::parse("AAQAAMh48/1o==").expect("test artifact"),
            signing_cert_base64: &test_cert(),
        })
        .unwrap();

        assert!(xml.contains("ArtifactResolve"));
        assert!(xml.contains("<samlp:Artifact>AAQAAMh48/1o==</samlp:Artifact>"));
        assert!(xml.contains("<saml:Issuer>urn:test:dv</saml:Issuer>"));
        assert!(xml.contains("URI=\"#_res1\""));
    }

    #[test]
    fn build_logout_request_contains_name_id() {
        let xml = build_logout_request(LogoutRequestArgs {
            id: &id("_lr1"),
            issue_instant: "2025-01-01T00:00:00Z",
            destination: &url("https://rd.example.com/slo"),
            issuer: &dv(),
            name_id: &NameId::parse("transient-id-abc").expect("test NameID"),
            signing_cert_base64: &test_cert(),
        })
        .unwrap();

        assert!(xml.contains("LogoutRequest"));
        assert!(xml.contains("<saml:NameID>transient-id-abc</saml:NameID>"));
        assert!(xml.contains("Destination=\"https://rd.example.com/slo\""));
    }

    #[test]
    fn build_dv_metadata_contains_required_elements() {
        let xml = build_dv_metadata(DvMetadataArgs {
            entity_id: &dv(),
            acs_url: &url("https://dv.example.com/acs"),
            slo_url: &url("https://dv.example.com/slo"),
            service_name: "Test Service",
            service_uuid: &service_uuid(),
            metadata_id: &id("_md1"),
            signing_cert_base64: &test_cert(),
            signing_keys: &[PublishedKey {
                key_name: &KeyName::parse("a1b2").unwrap(),
                cert_base64: &cert("Y2VydDE="),
            }],
            encryption_keys: &[PublishedKey {
                key_name: &KeyName::parse("c3d4").unwrap(),
                cert_base64: &cert("Y2VydDI="),
            }],
        })
        .unwrap();

        assert!(xml.contains("EntityDescriptor"));
        assert!(xml.contains("entityID=\"urn:test:dv\""));
        assert!(xml.contains("SPSSODescriptor"));
        assert!(xml.contains("AuthnRequestsSigned=\"true\""));
        assert!(xml.contains("WantAssertionsSigned=\"true\""));
        assert!(xml.contains("SingleLogoutService"));
        assert!(xml.contains("AssertionConsumerService"));
        assert!(xml.contains("ServiceName"));
        assert!(xml.contains("Test Service"));
        // Both KeyDescriptors rendered.
        assert!(xml.contains("use=\"signing\""));
        assert!(xml.contains("use=\"encryption\""));
        assert!(xml.contains("<dsig:KeyName>a1b2</dsig:KeyName>"));
        assert!(xml.contains("<dsig:KeyName>c3d4</dsig:KeyName>"));
        // Inline signature references the metadata ID.
        assert!(xml.contains("URI=\"#_md1\""));
    }

    #[test]
    fn wrap_in_soap_envelope_wraps_body() {
        let soap = wrap_in_soap_envelope("<msg>hi</msg>").unwrap();
        assert!(soap.starts_with("<?xml version=\"1.0\""));
        assert!(soap.contains("soap:Envelope"));
        assert!(soap.contains("soap:Body"));
        assert!(soap.contains("<msg>hi</msg>"));
    }

    #[test]
    fn now_utc_matches_iso_format() {
        let ts = now_utc();
        assert!(ts.ends_with('Z'));
        assert!(ts.contains('T'));
        assert!(ts.parse::<jiff::Timestamp>().is_ok());
    }
}
