//! DV SAML metadata builder.

use crate::{
    error::{AuthError, Result},
    keys::KeyPair,
    saml::{
        crypto::sign,
        xml_builder::{DvMetadataArgs, PublishedKey, build_dv_metadata},
    },
    types::{EndpointUrl, EntityId, MessageId, ServiceUuid},
};

/// Inputs for [`build_signed_dv_metadata`]: the SP identity and endpoints plus
/// the key material to publish. The document is signed with the first signing
/// key (whose certificate is also carried in the Signature `KeyInfo`).
pub struct SignedDvMetadataArgs<'a> {
    pub entity_id: &'a EntityId,
    pub acs_url: &'a EndpointUrl,
    pub slo_url: &'a EndpointUrl,
    pub service_name: &'a str,
    pub service_uuid: &'a ServiceUuid,
    pub signing_keys: &'a [KeyPair],
    /// The DV's mTLS client certificate, published alongside the SAML signing
    /// cert(s) as a `use="signing"` KeyDescriptor (eID §8.3); `None` to omit it.
    pub tls_signing_cert: Option<&'a KeyPair>,
    pub encryption_keys: &'a [KeyPair],
}

pub fn build_signed_dv_metadata(args: SignedDvMetadataArgs) -> Result<String> {
    let signing_key = args
        .signing_keys
        .first()
        .ok_or_else(|| AuthError::Config("no DV signing key to sign metadata".to_string()))?;

    let metadata_id = MessageId::generate();
    let sk = published_signing_keys(args.signing_keys, args.tls_signing_cert);
    let ek = key_refs(args.encryption_keys.iter());

    let xml = build_dv_metadata(DvMetadataArgs {
        entity_id: args.entity_id,
        acs_url: args.acs_url,
        slo_url: args.slo_url,
        service_name: args.service_name,
        service_uuid: args.service_uuid,
        metadata_id: &metadata_id,
        signing_cert_base64: &signing_key.cert_base64,
        signing_keys: &sk,
        encryption_keys: &ek,
    })?;

    sign(&xml, &signing_key.key_pem)
}

/// The `use="signing"` keys to publish: every SAML signing key plus the mTLS
/// client cert (eID §8.3), dropping the TLS cert when it is already one of the
/// signing certs (a combined signing+TLS certificate) so the metadata never
/// carries a duplicate KeyDescriptor.
fn published_signing_keys<'a>(
    signing_keys: &'a [KeyPair],
    tls_signing_cert: Option<&'a KeyPair>,
) -> Vec<PublishedKey<'a>> {
    let tls_extra =
        tls_signing_cert.filter(|tls| signing_keys.iter().all(|k| k.key_name != tls.key_name));
    key_refs(signing_keys.iter().chain(tls_extra))
}

/// The `<md:KeyDescriptor>` view of a list of keys.
fn key_refs<'a>(keys: impl Iterator<Item = &'a KeyPair>) -> Vec<PublishedKey<'a>> {
    keys.map(|k| PublishedKey {
        key_name: &k.key_name,
        cert_base64: &k.cert_base64,
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn dv_url(url: &str) -> EndpointUrl {
        EndpointUrl::from_base_url(url, "DV endpoint").expect("test DV endpoint")
    }

    /// Load a fixture key pair from the committed TVS test bundle.
    async fn load_pair(name: &str) -> KeyPair {
        let dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures"));
        crate::keys::load_key_pair(&crate::keys::key_pair_paths(&dir, name))
            .await
            .unwrap()
    }

    fn build(signing: &[KeyPair], tls: Option<&KeyPair>, encryption: &[KeyPair]) -> String {
        build_signed_dv_metadata(SignedDvMetadataArgs {
            entity_id: &EntityId::from_static("urn:test:dv"),
            acs_url: &dv_url("https://dv.test/saml/sp/acs"),
            slo_url: &dv_url("https://dv.test/saml/sp/logout"),
            service_name: "Test DV",
            service_uuid: &ServiceUuid::from_static("f847dc11-ac24-47b2-84a8-a057440ce56d"),
            signing_keys: signing,
            tls_signing_cert: tls,
            encryption_keys: encryption,
        })
        .expect("metadata must build and sign")
    }

    #[tokio::test]
    async fn tls_cert_published_as_extra_signing_key_descriptor() {
        let signing = load_pair("dv-signing-1").await;
        let encryption = load_pair("dv-encryption-1").await;
        let tls = load_pair("dv-tls").await;

        let xml = build(
            std::slice::from_ref(&signing),
            Some(&tls),
            std::slice::from_ref(&encryption),
        );

        assert_eq!(
            xml.matches(r#"use="signing""#).count(),
            2,
            "SAML signing cert + TLS cert each get a signing KeyDescriptor"
        );
        assert_eq!(xml.matches(r#"use="encryption""#).count(), 1);
        assert!(
            xml.contains(tls.key_name.as_str()),
            "TLS cert KeyName must appear in the metadata"
        );
        assert!(xml.contains(signing.key_name.as_str()));
    }

    #[tokio::test]
    async fn tls_cert_equal_to_signing_cert_is_not_duplicated() {
        let signing = load_pair("dv-signing-1").await;
        let encryption = load_pair("dv-encryption-1").await;

        // A combined signing+TLS certificate: the cert handed in as the TLS
        // cert is the same one already listed as the SAML signing key.
        let xml = build(
            std::slice::from_ref(&signing),
            Some(&signing),
            std::slice::from_ref(&encryption),
        );

        assert_eq!(
            xml.matches(r#"use="signing""#).count(),
            1,
            "a combined signing+TLS cert must not yield a duplicate KeyDescriptor"
        );
    }

    #[tokio::test]
    async fn signed_dv_metadata_verifies() {
        use crate::saml::{
            constants::NS_MD,
            verification::{ExpectedRoot, verify_xml_signature},
        };

        let signing = load_pair("dv-signing-1").await;
        let encryption = load_pair("dv-encryption-1").await;
        let tls = load_pair("dv-tls").await;

        let xml = build(
            std::slice::from_ref(&signing),
            Some(&tls),
            std::slice::from_ref(&encryption),
        );

        let expected = ExpectedRoot {
            namespace: NS_MD,
            local_name: "EntityDescriptor",
            id: None,
        };
        let result = verify_xml_signature(&xml, std::slice::from_ref(&signing), &expected);
        assert!(
            result.is_valid(),
            "signed DV metadata must verify with the signing key: {:?}",
            result.errors
        );
    }

    #[tokio::test]
    async fn no_tls_cert_publishes_only_saml_signing_key() {
        let signing = load_pair("dv-signing-1").await;
        let encryption = load_pair("dv-encryption-1").await;

        let xml = build(
            std::slice::from_ref(&signing),
            None,
            std::slice::from_ref(&encryption),
        );

        assert_eq!(xml.matches(r#"use="signing""#).count(), 1);
    }
}
