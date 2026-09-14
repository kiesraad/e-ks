//! Parse and fetch IdP (RD) metadata.
//!
//! eID §9.2: signature verification keys MUST come from verified metadata.
//! The `entityID`, the `SingleSignOnService` / `ArtifactResolutionService` /
//! `SingleLogoutService` endpoints, and the signing/encryption certs all live
//! inside the metadata document; config only carries the bootstrap URL.
use crate::{
    error::{AuthError, Result},
    keys::{CertificateBase64, CertificatePem, KeyPair, PrivateKeyPem},
    saml::{
        constants::{BINDING_HTTP_POST, BINDING_SOAP, CLOCK_SKEW_SECONDS, NS_DSIG, NS_MD},
        verification::{ExpectedRoot, verify_xml_signature},
        xml_parser::{
            Document, NodeId, descendants_by_tag, direct_text, find_child, find_descendant,
        },
    },
    types::{EndpointUrl, EntityId},
};
use rustls_pki_types::{CertificateDer, UnixTime};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tracing::{debug, info, warn};

/// All IdP data extracted from a verified metadata document.
#[derive(Debug, Clone)]
pub struct IdpMetadata {
    pub entity_id: EntityId,
    /// SSO endpoint with HTTP-POST binding (eID §3.1.1).
    pub sso_url: EndpointUrl,
    /// Artifact Resolution endpoint with SOAP binding (eID §7.5).
    pub ars_url: EndpointUrl,
    /// Single Logout endpoint with HTTP-POST binding (eID §7.7.1).
    pub slo_url: EndpointUrl,
    pub signing_keys: Vec<KeyPair>,
    /// Parsed `cacheDuration` (eID §8.5), the RD's hint for how long the
    /// descriptor may be cached. The background refresh task uses it, capped at
    /// 24h, to schedule the next fetch. `None` when absent or unparseable.
    pub cache_duration: Option<Duration>,
}

#[cfg(test)]
impl IdpMetadata {
    /// A descriptor for tests: the RD identity and its three endpoints, no keys.
    /// Callers override individual fields with `..IdpMetadata::for_tests()`.
    pub(crate) fn for_tests() -> Self {
        let endpoint = |path: &str| {
            EndpointUrl::from_metadata(&format!("https://rd.example.com/{path}"), path)
                .expect("test RD endpoint")
        };
        Self {
            entity_id: EntityId::from_static("urn:test:rd"),
            sso_url: endpoint("sso"),
            ars_url: endpoint("ars"),
            slo_url: endpoint("slo"),
            signing_keys: Vec::new(),
            cache_duration: None,
        }
    }
}

/// Keys extracted from metadata `KeyDescriptor` elements, separated by intended use.
pub struct IdpKeys {
    /// `use="signing"` (plus any use-less) certs: the only keys the DV verifies
    /// incoming signatures against (eID §9.2).
    pub signing: Vec<KeyPair>,
    /// `use="encryption"` (plus any use-less) certs. The DV never encrypts *to*
    /// the RD, so these are unused in the flow; they are extracted so the
    /// metadata tests can assert an encryption-only key never verifies a
    /// signature.
    pub encryption: Vec<KeyPair>,
}

/// Extract keys from metadata `KeyDescriptor` elements.
///
/// Each `KeyDescriptor` has a `use` attribute (`"signing"` or `"encryption"`).
/// Per the SAML metadata spec, omitting `use` means the key may be used for
/// both purposes, so it is added to both lists.
///
/// Only keys with an `X509Certificate` are extracted; KeyName-only descriptors
/// are skipped (they require an out-of-band certificate lookup).
pub fn extract_idp_keys(doc: &Document, root: NodeId) -> IdpKeys {
    let mut signing = Vec::new();
    let mut encryption = Vec::new();

    for kd in descendants_by_tag(doc, root, NS_MD, "KeyDescriptor") {
        let Some(kp) = descriptor_key_pair(doc, kd) else {
            continue;
        };
        match doc.get_attribute(kd, "use") {
            Some("signing") => signing.push(kp),
            Some("encryption") => encryption.push(kp),
            _ => {
                // No use attribute: usable for both purposes
                signing.push(kp.clone());
                encryption.push(kp);
            }
        }
    }

    IdpKeys {
        signing,
        encryption,
    }
}

/// The public-only [`KeyPair`] of a `KeyDescriptor`'s `X509Certificate`, or
/// `None` for a KeyName-only descriptor.
fn descriptor_key_pair(doc: &Document, kd: NodeId) -> Option<KeyPair> {
    let cert_node = find_descendant(doc, kd, NS_DSIG, "X509Certificate")?;
    // `direct_text`: a trusted signing cert is the element's own text.
    let cert_base64 = match CertificateBase64::parse(&direct_text(doc, cert_node)?) {
        Ok(cert) => cert,
        Err(e) => {
            warn!("[metadata] Skipping malformed KeyDescriptor certificate: {e}");
            return None;
        }
    };
    Some(KeyPair::from_pem(
        cert_base64.to_pem(),
        PrivateKeyPem::absent(),
    ))
}

/// The RD's `<md:IDPSSODescriptor>`: the one role descriptor every endpoint and
/// `KeyDescriptor` below is read from.
fn idp_sso_descriptor(doc: &Document, root: NodeId) -> Result<NodeId> {
    find_child(doc, root, NS_MD, "IDPSSODescriptor").ok_or_else(|| {
        AuthError::Config("metadata: EntityDescriptor has no IDPSSODescriptor".into())
    })
}

fn endpoint_location(doc: &Document, root: NodeId, tag: &str, binding: &str) -> Option<String> {
    descendants_by_tag(doc, root, NS_MD, tag)
        .into_iter()
        .find_map(|n| {
            (doc.get_attribute(n, "Binding") == Some(binding))
                .then(|| doc.get_attribute(n, "Location").map(str::to_owned))
                .flatten()
        })
}

/// The `Location` of the `tag` endpoint with `binding`, as a validated
/// [`EndpointUrl`] (eID §9.4 requires https; see
/// [`EndpointUrl::from_metadata`]).
fn required_endpoint(
    doc: &Document,
    root: NodeId,
    tag: &str,
    binding: &str,
) -> Result<EndpointUrl> {
    let location = endpoint_location(doc, root, tag, binding)
        .ok_or_else(|| AuthError::Config(format!("metadata: no {binding} {tag}")))?;
    EndpointUrl::from_metadata(&location, tag)
}

/// Parse an XML Schema duration (e.g. `PT24H`, `P1D`, `PT1H30M`) into a
/// [`Duration`]. Supports days/weeks (date part) and hours/minutes/seconds (time
/// part), which covers the SAML metadata `cacheDuration` values; returns `None`
/// for fractional, year/month, or otherwise unsupported forms (the caller then
/// falls back to the default refresh cap).
fn parse_xs_duration(s: &str) -> Option<Duration> {
    fn accumulate(part: &str, in_time: bool, secs: &mut u64) -> Option<()> {
        let mut num = String::new();
        for c in part.chars() {
            if c.is_ascii_digit() {
                num.push(c);
                continue;
            }
            let n: u64 = num.parse().ok()?;
            num.clear();
            let unit: u64 = match (in_time, c) {
                (false, 'D') => 86_400,
                (false, 'W') => 604_800,
                (true, 'H') => 3_600,
                (true, 'M') => 60,
                (true, 'S') => 1,
                // Years/months (calendar-ambiguous), fractions, or junk: bail.
                _ => return None,
            };
            *secs = secs.checked_add(n.checked_mul(unit)?)?;
        }
        // Trailing digits with no unit are malformed.
        num.is_empty().then_some(())
    }

    let body = s.strip_prefix('P')?;
    let (date_part, time_part) = body.split_once('T').unwrap_or((body, ""));
    let mut secs = 0u64;
    accumulate(date_part, false, &mut secs)?;
    accumulate(time_part, true, &mut secs)?;
    Some(Duration::from_secs(secs))
}

/// Pinned trust material for validating the RD metadata signing certificate
/// (eID §9.1/§9.2): the expected RD identity plus the embedded root/intermediate
/// CAs the signing cert must chain to. Every field is a compile-time constant, so
/// this is cheap to build and copy.
#[derive(Clone)]
pub struct RdTrust {
    /// Expected RD `entityID`; loaded metadata must match it exactly.
    pub expected_entity_id: EntityId,
    /// Expected RD OIN, required in the signing cert's `Subject.serialNumber`.
    pub expected_oin: &'static str,
    /// Trust-anchor root CA(s), PEM-encoded
    /// ([`crate::saml::pki::RD_METADATA_TRUST_ROOTS`]).
    pub roots: &'static [&'static [u8]],
    /// Path-building intermediate CA(s), PEM-encoded
    /// ([`crate::saml::pki::RD_METADATA_INTERMEDIATES`]).
    pub intermediates: &'static [&'static [u8]],
}

impl RdTrust {
    /// The pinned RD trust for `environment`: the RD EntityID and OIN from
    /// [`crate::config`] and the root/intermediate CAs from [`crate::saml::pki`].
    pub fn for_environment(environment: crate::config::Environment) -> Self {
        Self {
            expected_entity_id: environment.rd_entity_id(),
            expected_oin: crate::config::RD_OIN,
            roots: crate::saml::pki::RD_METADATA_TRUST_ROOTS,
            intermediates: crate::saml::pki::RD_METADATA_INTERMEDIATES,
        }
    }
}

fn pem_bytes_to_der(pem: &[u8]) -> Result<Vec<u8>> {
    let pem = std::str::from_utf8(pem)
        .map_err(|e| AuthError::Crypto(format!("non-UTF-8 certificate PEM: {e}")))?;
    Ok(CertificatePem::parse(pem)?.to_der())
}

/// The `Subject.serialNumber` (OID 2.5.4.5) of a DER certificate, if present.
/// PKIoverheid encodes the participant OIN there (eID §9.1).
fn subject_oin(leaf_der: &[u8]) -> Option<String> {
    use x509_cert::der::Decode;
    let cert = x509_cert::Certificate::from_der(leaf_der).ok()?;
    // Build the OID from the same `const_oid` version that `x509_cert` exposes on
    // `atv.oid`; a direct `const_oid` dep can resolve to a different major version.
    let serial_number_oid = x509_cert::der::asn1::ObjectIdentifier::new_unwrap("2.5.4.5");
    cert.tbs_certificate()
        .subject()
        .iter()
        .find(|atv| atv.oid == serial_number_oid)
        // The serialNumber value is a DER string (Printable/UTF8/IA5); its content
        // bytes are the ASCII OIN regardless of the exact string type.
        .and_then(|atv| std::str::from_utf8(atv.value.value()).ok())
        .map(|s| s.trim().to_string())
}

/// Whether `cert_pem` is a trusted RD signing certificate (eID §9.1/§9.2): it
/// MUST carry the expected RD OIN in its subject AND chain to one of the pinned
/// roots via the supplied intermediates. The signing keys themselves are
/// intentionally NOT pinned (they rotate); trust derives from the chain + OIN.
fn cert_is_trusted(cert_pem: &CertificatePem, trust: &RdTrust) -> Result<()> {
    let leaf_der = cert_pem.to_der();
    check_rd_oin(&leaf_der, trust)?;
    check_chains_to_pinned_root(&leaf_der, trust)
}

/// eID §9.1: the certificate MUST contain the participant (RD) OIN, so a
/// different PKIoverheid participant's cert cannot impersonate the RD.
fn check_rd_oin(leaf_der: &[u8], trust: &RdTrust) -> Result<()> {
    match subject_oin(leaf_der) {
        Some(oin) if oin == trust.expected_oin => Ok(()),
        Some(oin) => Err(AuthError::Crypto(format!(
            "RD signing cert OIN {oin} does not match expected {}",
            trust.expected_oin
        ))),
        None => Err(AuthError::Crypto(
            "RD signing cert has no subject serialNumber (OIN)".into(),
        )),
    }
}

/// eID §9.2: the certificate MUST chain to a pinned PKIoverheid root. The RD
/// metadata ships only the leaf, so the intermediates are supplied from pki.
fn check_chains_to_pinned_root(leaf_der: &[u8], trust: &RdTrust) -> Result<()> {
    let root_ders = pem_list_to_der(trust.roots)?;
    let intermediate_ders = pem_list_to_der(trust.intermediates)?;

    let root_certs = certificates(&root_ders);
    let anchors: Vec<rustls_pki_types::TrustAnchor> = root_certs
        .iter()
        .map(webpki::anchor_from_trusted_cert)
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| AuthError::Crypto(format!("invalid pinned PKIoverheid root: {e}")))?;
    let intermediates = certificates(&intermediate_ders);

    verify_cert_chain(leaf_der, &anchors, &intermediates)
}

/// Run the webpki path validation of `leaf_der` against the pinned anchors.
fn verify_cert_chain(
    leaf_der: &[u8],
    anchors: &[rustls_pki_types::TrustAnchor],
    intermediates: &[CertificateDer],
) -> Result<()> {
    let leaf = CertificateDer::from(leaf_der);
    let end_entity = webpki::EndEntityCert::try_from(&leaf)
        .map_err(|e| AuthError::Crypto(format!("invalid RD signing cert: {e}")))?;
    end_entity
        .verify_for_usage(
            webpki::ALL_VERIFICATION_ALGS,
            anchors,
            intermediates,
            UnixTime::now(),
            // `server_auth()` is `required_if_present(serverAuth)`: it accepts the
            // production leaf (which carries serverAuth) and the mock leaf (no EKU).
            webpki::KeyUsage::server_auth(),
            None,
            None,
        )
        .map_err(|e| {
            AuthError::Crypto(format!(
                "RD signing cert does not chain to the pinned PKIoverheid root: {e}"
            ))
        })?;
    Ok(())
}

fn pem_list_to_der(pems: &[&[u8]]) -> Result<Vec<Vec<u8>>> {
    pems.iter().map(|pem| pem_bytes_to_der(pem)).collect()
}

fn certificates(ders: &[Vec<u8>]) -> Vec<CertificateDer<'_>> {
    ders.iter()
        .map(|d| CertificateDer::from(d.as_slice()))
        .collect()
}

/// Parse metadata XML and extract entity ID, endpoints, and keys, after pinning
/// the RD identity (eID §9.1/§9.2).
///
/// Trust is anchored, not self-referential:
///  1. the metadata `entityID` MUST equal the configured `trust.expected_entity_id`;
///  2. only signing certs that carry the RD OIN AND chain to the embedded
///     PKIoverheid root (`trust.roots`/`trust.intermediates`) are kept;
///  3. the enveloping XML signature is then verified against those pinned certs.
///
/// So a spoofed metadata document re-signed with an attacker's own key is
/// rejected even if the HTTPS metadata fetch is subverted. The RD signing keys
/// are not pinned individually (they rotate); trust comes from the chain + OIN.
pub fn parse_idp_metadata(xml: &str, trust: &RdTrust) -> Result<IdpMetadata> {
    debug!("[metadata] Parsing IdP metadata (xml_len={})", xml.len());
    let doc = crate::saml::xml_parser::parse(xml)?;
    let root = doc.document_element();

    let entity_id = pinned_entity_id(&doc, root, trust)?;
    check_metadata_expiry(&doc, root)?;
    // Endpoints and keys are read only from the IdP role descriptor, never
    // document-wide (see `idp_sso_descriptor`).
    let idp = idp_sso_descriptor(&doc, root)?;
    let signing_keys = verified_signing_keys(xml, &doc, root, idp, trust)?;

    let (sso_url, ars_url, slo_url) = resolve_endpoints(&doc, idp)?;
    debug!("[metadata] Endpoints resolved: sso={sso_url}, ars={ars_url}, slo={slo_url}");

    let cache_duration = doc
        .get_attribute(root, "cacheDuration")
        .and_then(parse_xs_duration);
    debug!("[metadata] cacheDuration parsed as {cache_duration:?}");

    Ok(IdpMetadata {
        entity_id,
        sso_url,
        ars_url,
        slo_url,
        signing_keys,
        cache_duration,
    })
}

/// eID §9.2 / §10.2: pin the RD identity. The expected EntityID is a configured
/// constant, not a value taken from this (only self-signature-checked) document.
fn pinned_entity_id(doc: &Document, root: NodeId, trust: &RdTrust) -> Result<EntityId> {
    let entity_id = doc
        .get_attribute(root, "entityID")
        .ok_or_else(|| AuthError::Xml("metadata: missing entityID".into()))?;
    debug!("[metadata] entityID={entity_id}");
    let entity_id = EntityId::parse(entity_id)?;
    if entity_id != trust.expected_entity_id {
        return Err(AuthError::Crypto(format!(
            "metadata entityID {entity_id} does not match the pinned RD EntityID {}",
            trust.expected_entity_id
        )));
    }
    Ok(entity_id)
}

/// The signing certs that pass the pinned-trust filter (OIN + chain), after
/// verifying the metadata's enveloping signature against exactly those certs.
fn verified_signing_keys(
    xml: &str,
    doc: &Document,
    root: NodeId,
    idp: NodeId,
    trust: &RdTrust,
) -> Result<Vec<KeyPair>> {
    // Keys from the IdP role descriptor (`idp`); the signature covers, and is
    // bound to, the whole `EntityDescriptor` (`root`).
    let signing_keys = pinned_signing_keys(extract_idp_keys(doc, idp), trust)?;
    debug!("[metadata] Trusted signing keys: {}", signing_keys.len());

    // `xml` is the document the endpoints and keys came from, so the re-parse
    // inside `verify_xml_signature` must land on the same root.
    let expected_root = ExpectedRoot {
        namespace: NS_MD,
        local_name: "EntityDescriptor",
        id: doc.get_attribute(root, "ID"),
    };
    let sig_result = verify_xml_signature(xml, &signing_keys, &expected_root);
    if !sig_result.is_valid() {
        return Err(AuthError::Crypto(format!(
            "metadata signature verification failed: {}",
            sig_result.errors.join("; ")
        )));
    }
    debug!("[metadata] Metadata signature verified against a pinned RD signing cert");
    Ok(signing_keys)
}

/// eID §8.2/§8.5: do not use metadata past its hard expiry. If `validUntil` is
/// present and has passed (subject to clock skew), reject the document so an
/// expired descriptor, including a stale on-disk cache, is never trusted.
fn check_metadata_expiry(doc: &Document, root: NodeId) -> Result<()> {
    let valid_until = doc.get_attribute(root, "validUntil");
    if let Some(s) = valid_until {
        // This runs before the metadata signature is verified, so `s` is
        // attacker-influenced whenever the HTTPS fetch (or the on-disk cache) is
        // subverted. `checked_add_signed`, not `+`: chrono panics when a
        // timestamp near the edge of the representable range is shifted.
        let expires_at = s
            .parse::<chrono::DateTime<chrono::Utc>>()
            .ok()
            .and_then(|t| t.checked_add_signed(chrono::Duration::seconds(CLOCK_SKEW_SECONDS)))
            .ok_or_else(|| {
                AuthError::Config(format!(
                    "metadata has an unusable validUntil timestamp: {s}"
                ))
            })?;
        if chrono::Utc::now() > expires_at {
            return Err(AuthError::Config(format!(
                "metadata has expired: validUntil {s} is in the past"
            )));
        }
    }
    // eID §8.4 (RD IdP metadata table): "Either validUntil or cacheDuration MUST
    // be present". A descriptor with neither has no expiry and no refresh hint,
    // so it would be cached indefinitely: reject it rather than pin the RD's keys
    // forever on a document that never goes stale.
    if valid_until.is_none() && doc.get_attribute(root, "cacheDuration").is_none() {
        return Err(AuthError::Config(
            "metadata carries neither validUntil nor cacheDuration (eID §8.4 requires one)"
                .to_string(),
        ));
    }
    Ok(())
}

/// eID §9.1/§9.2: keep only signing certs that carry the RD OIN and chain to
/// the embedded PKIoverheid root. The metadata self-signature check is then
/// meaningful: it must be produced by a pinned-trust RD certificate. Errors
/// when no signing cert survives the filter.
fn pinned_signing_keys(keys: IdpKeys, trust: &RdTrust) -> Result<Vec<KeyPair>> {
    let signing_keys: Vec<KeyPair> = keys
        .signing
        .into_iter()
        .filter(|kp| match cert_is_trusted(&kp.cert_pem, trust) {
            Ok(()) => true,
            Err(e) => {
                warn!("[metadata] Rejecting RD signing certificate: {e}");
                false
            }
        })
        .collect();
    if signing_keys.is_empty() {
        return Err(AuthError::Crypto(
            "no RD metadata signing certificate chains to the pinned PKIoverheid root with the expected OIN"
                .into(),
        ));
    }
    Ok(signing_keys)
}

/// Resolve the three required endpoints (eID §3.1.1/§7.5/§7.7.1) and validate
/// each as a clean absolute https URL (eID §9.4). The validation also keeps the
/// values safe to interpolate downstream (HTML attribute, CSP, HTTP target).
fn resolve_endpoints(
    doc: &Document,
    root: NodeId,
) -> Result<(EndpointUrl, EndpointUrl, EndpointUrl)> {
    Ok((
        required_endpoint(doc, root, "SingleSignOnService", BINDING_HTTP_POST)?,
        required_endpoint(doc, root, "ArtifactResolutionService", BINDING_SOAP)?,
        required_endpoint(doc, root, "SingleLogoutService", BINDING_HTTP_POST)?,
    ))
}

/// On-disk cache file name for the RD (Routeringsdienst) metadata.
const RD_METADATA_CACHE_FILE: &str = "rd-metadata.xml";

/// Path of the on-disk RD metadata cache under `certs_dir`.
pub fn metadata_cache_path(certs_dir: &Path) -> PathBuf {
    certs_dir.join(RD_METADATA_CACHE_FILE)
}

/// Fetch metadata from `url` (HTTP GET), parse it, and on success persist the
/// raw document to the on-disk cache. Used at startup and by the background
/// refresh task that keeps the descriptor within its `cacheDuration`/`validUntil`
/// (eID §8.5).
///
/// The cached file is the signed document verbatim, so reloading it via
/// [`load_cached_idp_metadata`] re-runs the same signature verification. A
/// failure to write the cache is logged but does not fail the fetch.
pub async fn fetch_and_cache_idp_metadata(
    url: &str,
    certs_dir: &Path,
    trust: &RdTrust,
) -> Result<IdpMetadata> {
    info!("[metadata] Fetching IdP metadata from {url}");
    let response = reqwest::get(url).await?.error_for_status()?;
    // Cap the buffered body (see bindings::soap::read_body_capped).
    let xml = crate::bindings::soap::read_body_capped(
        response,
        crate::bindings::soap::MAX_HTTP_BODY_BYTES,
    )
    .await?;
    debug!("[metadata] Fetched metadata XML (len={})", xml.len());
    let metadata = parse_idp_metadata(&xml, trust)?;
    info!(
        "[metadata] IdP metadata loaded: entity_id={}, signing_keys={}",
        metadata.entity_id,
        metadata.signing_keys.len(),
    );

    let path = metadata_cache_path(certs_dir);
    match write_cache(certs_dir, &path, &xml).await {
        Ok(()) => debug!("[metadata] Wrote metadata cache to {}", path.display()),
        Err(e) => warn!(
            "[metadata] Failed to write metadata cache {}: {e}",
            path.display()
        ),
    }

    Ok(metadata)
}

async fn write_cache(certs_dir: &Path, path: &Path, xml: &str) -> std::io::Result<()> {
    tokio::fs::create_dir_all(certs_dir).await?;
    tokio::fs::write(path, xml).await
}

/// Load and parse IdP metadata from the on-disk cache written by an earlier
/// [`fetch_and_cache_idp_metadata`] call. Used as a startup fallback when the
/// IdP is unreachable. Returns `None` when no cache file exists or the cached
/// document fails to parse or verify; callers treat that as "no fallback".
pub async fn load_cached_idp_metadata(certs_dir: &Path, trust: &RdTrust) -> Option<IdpMetadata> {
    let path = metadata_cache_path(certs_dir);
    let xml = tokio::fs::read_to_string(&path).await.ok()?;
    let metadata = match parse_idp_metadata(&xml, trust) {
        Ok(metadata) => metadata,
        Err(e) => {
            warn!(
                "[metadata] Ignoring invalid cached metadata at {}: {e}",
                path.display()
            );
            return None;
        }
    };

    // eID §8.5: not past cacheDuration, and never past the ceiling
    let Some(age) = cache_age(&path).await else {
        warn!(
            "[metadata] Ignoring cached metadata at {}: its age cannot be determined",
            path.display()
        );
        return None;
    };
    let max_age = metadata
        .cache_duration
        .unwrap_or(DEFAULT_CACHE_AGE)
        .min(MAX_CACHE_AGE);
    if age > max_age {
        warn!(
            "[metadata] Ignoring cached metadata at {}: written {}s ago, older than its cacheDuration of {}s",
            path.display(),
            age.as_secs(),
            max_age.as_secs()
        );
        return None;
    }

    info!(
        "[metadata] Loaded IdP metadata from disk cache {} (entity_id={}, age={}s)",
        path.display(),
        metadata.entity_id,
        age.as_secs()
    );
    Some(metadata)
}

/// Age limit for a cached descriptor without a usable `cacheDuration`.
const DEFAULT_CACHE_AGE: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
/// Age limit for a cached descriptor whatever its `cacheDuration` says.
const MAX_CACHE_AGE: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 60 * 60);

/// How long ago the cache file was written, from its modification time.
async fn cache_age(path: &Path) -> Option<std::time::Duration> {
    let written = tokio::fs::metadata(path).await.ok()?.modified().ok()?;
    Some(
        std::time::SystemTime::now()
            .duration_since(written)
            .unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // The fixture CA directly issues the fixture rd-signing-1 cert, so it is the
    // trust anchor for the test chain (no intermediates), mirroring `tvs-mock`.
    const TEST_CA_PEM: &[u8] =
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/ca.pem"));
    const TEST_ROOTS: &[&[u8]] = &[TEST_CA_PEM];
    const NO_INTERMEDIATES: &[&[u8]] = &[];
    const FIXTURE_OIN: &str = "00000004000000149000";

    fn test_trust(entity_id: &'static str) -> RdTrust {
        RdTrust {
            expected_entity_id: EntityId::from_static(entity_id),
            expected_oin: FIXTURE_OIN,
            roots: TEST_ROOTS,
            intermediates: NO_INTERMEDIATES,
        }
    }

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures")
                .join(name),
        )
        .unwrap()
    }

    /// RD metadata signed by the fixture `rd-signing-1` key, with that cert in a
    /// `use="signing"` KeyDescriptor and the three required endpoints.
    fn signed_rd_metadata(entity_id: &str) -> String {
        signed_rd_metadata_attrs(entity_id, "")
    }

    /// As [`signed_rd_metadata`], with extra attributes (e.g. `validUntil` /
    /// `cacheDuration`) on the `<EntityDescriptor>` root.
    fn signed_rd_metadata_attrs(entity_id: &str, root_attrs: &str) -> String {
        let cert_pem = CertificatePem::parse(fixture("rd-signing-1.pem")).unwrap();
        let key_pem = PrivateKeyPem::new(fixture("rd-signing-1-key.pem"));
        let cert_b64 = cert_pem.to_base64();
        let id = "_rdmeta1";
        let xml = format!(
            r##"<md:EntityDescriptor xmlns:md="urn:oasis:names:tc:SAML:2.0:metadata" xmlns:dsig="http://www.w3.org/2000/09/xmldsig#" ID="{id}" entityID="{entity_id}"{root_attrs}><dsig:Signature><dsig:SignedInfo><dsig:CanonicalizationMethod Algorithm="http://www.w3.org/2001/10/xml-exc-c14n#"/><dsig:SignatureMethod Algorithm="http://www.w3.org/2001/04/xmldsig-more#rsa-sha256"/><dsig:Reference URI="#{id}"><dsig:Transforms><dsig:Transform Algorithm="http://www.w3.org/2000/09/xmldsig#enveloped-signature"/><dsig:Transform Algorithm="http://www.w3.org/2001/10/xml-exc-c14n#"/></dsig:Transforms><dsig:DigestMethod Algorithm="http://www.w3.org/2001/04/xmlenc#sha256"/><dsig:DigestValue></dsig:DigestValue></dsig:Reference></dsig:SignedInfo><dsig:SignatureValue></dsig:SignatureValue><dsig:KeyInfo><dsig:X509Data><dsig:X509Certificate>{cert_b64}</dsig:X509Certificate></dsig:X509Data></dsig:KeyInfo></dsig:Signature><md:IDPSSODescriptor protocolSupportEnumeration="urn:oasis:names:tc:SAML:2.0:protocol"><md:KeyDescriptor use="signing"><dsig:KeyInfo><dsig:X509Data><dsig:X509Certificate>{cert_b64}</dsig:X509Certificate></dsig:X509Data></dsig:KeyInfo></md:KeyDescriptor><md:ArtifactResolutionService Binding="urn:oasis:names:tc:SAML:2.0:bindings:SOAP" Location="https://rd.test/ars" index="0"/><md:SingleSignOnService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST" Location="https://rd.test/sso"/><md:SingleLogoutService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST" Location="https://rd.test/slo"/></md:IDPSSODescriptor></md:EntityDescriptor>"##
        );
        crate::saml::crypto::sign(&xml, &key_pem).expect("sign metadata")
    }

    // -- RD signing-cert trust (eID §9.1/§9.2) --

    #[test]
    fn cert_is_trusted_accepts_fixture_rd_signing() {
        // rd-signing-1 carries the RD OIN and chains to the fixture CA.
        let cert = fixture("rd-signing-1.pem");
        assert!(
            cert_is_trusted(
                &CertificatePem::parse(&cert).unwrap(),
                &test_trust("urn:any")
            )
            .is_ok()
        );
    }

    #[test]
    fn cert_is_trusted_rejects_wrong_oin() {
        let cert = fixture("rd-signing-1.pem");
        let trust = RdTrust {
            expected_oin: "99999999999999999999",
            ..test_trust("urn:any")
        };
        assert!(cert_is_trusted(&CertificatePem::parse(&cert).unwrap(), &trust).is_err());
    }

    #[test]
    fn cert_is_trusted_rejects_cert_not_chaining_to_pinned_root() {
        // Right OIN, but no trust anchor to chain to -> rejected.
        let cert = fixture("rd-signing-1.pem");
        let trust = RdTrust {
            roots: &[],
            ..test_trust("urn:any")
        };
        assert!(cert_is_trusted(&CertificatePem::parse(&cert).unwrap(), &trust).is_err());
    }

    #[test]
    fn parse_idp_metadata_accepts_pinned_fixture_signed_metadata() {
        let entity_id = "urn:nl-eid-gdi:1.0:RD:00000004000000149000:entities:9002";
        let signed = signed_rd_metadata_attrs(entity_id, r#" cacheDuration="PT24H""#);
        let md = parse_idp_metadata(&signed, &test_trust(entity_id)).expect("must parse");
        assert_eq!(md.entity_id.as_str(), entity_id);
        assert_eq!(md.sso_url.as_str(), "https://rd.test/sso");
        assert_eq!(md.ars_url.as_str(), "https://rd.test/ars");
        assert_eq!(md.slo_url.as_str(), "https://rd.test/slo");
        assert_eq!(md.signing_keys.len(), 1);
        // eID §8.5: cacheDuration is parsed for the refresh-cadence hint.
        assert_eq!(md.cache_duration, Some(Duration::from_secs(24 * 3600)));
    }

    #[test]
    fn parse_idp_metadata_rejects_expired_metadata() {
        // eID §8.2/§8.5: a past validUntil must be rejected (stale metadata).
        let entity_id = "urn:nl-eid-gdi:1.0:RD:00000004000000149000:entities:9002";
        let signed = signed_rd_metadata_attrs(entity_id, r#" validUntil="2000-01-01T00:00:00Z""#);
        let err = parse_idp_metadata(&signed, &test_trust(entity_id)).unwrap_err();
        assert!(matches!(err, AuthError::Config(_)));
    }

    #[test]
    fn parse_idp_metadata_accepts_unexpired_metadata() {
        let entity_id = "urn:nl-eid-gdi:1.0:RD:00000004000000149000:entities:9002";
        let signed = signed_rd_metadata_attrs(entity_id, r#" validUntil="2999-01-01T00:00:00Z""#);
        assert!(parse_idp_metadata(&signed, &test_trust(entity_id)).is_ok());
    }

    #[test]
    fn metadata_valid_until_at_the_edge_of_the_range_is_rejected_not_panicked_on() {
        // `check_metadata_expiry` runs before the signature is verified, so a
        // subverted HTTPS fetch (or a poisoned disk cache) controls `validUntil`.
        // chrono panics when a timestamp at the edge of its range is shifted by
        // the skew allowance, so it must be refused up front.
        let entity_id = "urn:nl-eid-gdi:1.0:RD:00000004000000149000:entities:9002";
        let signed =
            signed_rd_metadata_attrs(entity_id, r#" validUntil="+262142-12-31T23:59:59Z""#);
        let err = parse_idp_metadata(&signed, &test_trust(entity_id)).unwrap_err();
        assert!(
            matches!(&err, AuthError::Config(m) if m.contains("unusable validUntil")),
            "{err:?}"
        );
    }

    #[test]
    fn parse_idp_metadata_requires_valid_until_or_cache_duration() {
        // eID §8.4: "Either validUntil or cacheDuration MUST be present". With
        // neither, the descriptor has no expiry and no refresh hint, so it would
        // pin the RD's keys indefinitely.
        let entity_id = "urn:nl-eid-gdi:1.0:RD:00000004000000149000:entities:9002";
        let signed = signed_rd_metadata(entity_id);
        let err = parse_idp_metadata(&signed, &test_trust(entity_id)).unwrap_err();
        assert!(
            err.to_string()
                .contains("neither validUntil nor cacheDuration"),
            "{err}"
        );

        // Either one alone is enough, and so is both together (the common case
        // in real descriptors: a hard expiry plus a refresh hint).
        for attrs in [
            r#" cacheDuration="PT24H""#,
            r#" validUntil="2999-01-01T00:00:00Z""#,
            r#" validUntil="2999-01-01T00:00:00Z" cacheDuration="PT24H""#,
        ] {
            let signed = signed_rd_metadata_attrs(entity_id, attrs);
            assert!(
                parse_idp_metadata(&signed, &test_trust(entity_id)).is_ok(),
                "{attrs} must be accepted"
            );
        }

        // With both present, both are honoured: the expiry gates acceptance and
        // the cacheDuration still drives the refresh cadence.
        let signed = signed_rd_metadata_attrs(
            entity_id,
            r#" validUntil="2999-01-01T00:00:00Z" cacheDuration="PT24H""#,
        );
        let md = parse_idp_metadata(&signed, &test_trust(entity_id)).expect("must parse");
        assert_eq!(md.cache_duration, Some(Duration::from_secs(24 * 3600)));

        // An expired validUntil is still fatal even when a cacheDuration is
        // present, so a long refresh hint cannot keep a dead descriptor alive.
        let signed = signed_rd_metadata_attrs(
            entity_id,
            r#" validUntil="2000-01-01T00:00:00Z" cacheDuration="PT24H""#,
        );
        assert!(parse_idp_metadata(&signed, &test_trust(entity_id)).is_err());
    }

    #[test]
    fn endpoints_and_keys_are_read_only_from_the_idp_role_descriptor() {
        // The RD is also an SP towards the AD/BVD, so an SPSSODescriptor may sit
        // alongside. Its endpoints and keys must be invisible here: a
        // document-wide search would take the SP-side SingleLogoutService as the
        // one we send LogoutRequests to.
        let xml = r#"<md:EntityDescriptor xmlns:md="urn:oasis:names:tc:SAML:2.0:metadata" xmlns:dsig="http://www.w3.org/2000/09/xmldsig#" entityID="urn:e">
            <md:SPSSODescriptor>
                <md:KeyDescriptor use="signing"><dsig:KeyInfo><dsig:X509Data><dsig:X509Certificate>SPCERT</dsig:X509Certificate></dsig:X509Data></dsig:KeyInfo></md:KeyDescriptor>
                <md:SingleLogoutService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST" Location="https://sp-side/slo"/>
            </md:SPSSODescriptor>
            <md:IDPSSODescriptor>
                <md:SingleLogoutService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST" Location="https://idp-side/slo"/>
            </md:IDPSSODescriptor>
        </md:EntityDescriptor>"#;
        let doc = crate::saml::xml_parser::parse(xml).unwrap();
        let root = doc.document_element();
        let idp = idp_sso_descriptor(&doc, root).expect("IDPSSODescriptor present");

        assert_eq!(
            endpoint_location(&doc, idp, "SingleLogoutService", BINDING_HTTP_POST).as_deref(),
            Some("https://idp-side/slo")
        );
        // The SP role's signing key is not an IdP signing key.
        assert!(extract_idp_keys(&doc, idp).signing.is_empty());
        // For contrast: from the EntityDescriptor the SP role would win, which is
        // exactly what the scoping prevents.
        assert_eq!(
            endpoint_location(&doc, root, "SingleLogoutService", BINDING_HTTP_POST).as_deref(),
            Some("https://sp-side/slo")
        );
    }

    #[test]
    fn metadata_without_an_idp_role_descriptor_is_rejected() {
        let xml = r#"<md:EntityDescriptor xmlns:md="urn:oasis:names:tc:SAML:2.0:metadata" entityID="urn:e"><md:SPSSODescriptor/></md:EntityDescriptor>"#;
        let doc = crate::saml::xml_parser::parse(xml).unwrap();
        let err = idp_sso_descriptor(&doc, doc.document_element()).unwrap_err();
        assert!(err.to_string().contains("no IDPSSODescriptor"), "{err}");
    }

    #[test]
    fn parse_xs_duration_handles_common_forms() {
        assert_eq!(
            parse_xs_duration("PT24H"),
            Some(Duration::from_secs(86_400))
        );
        assert_eq!(parse_xs_duration("P1D"), Some(Duration::from_secs(86_400)));
        assert_eq!(
            parse_xs_duration("PT1H30M"),
            Some(Duration::from_secs(5_400))
        );
        assert_eq!(parse_xs_duration("PT30S"), Some(Duration::from_secs(30)));
        // Unsupported / malformed forms return None (caller falls back to default).
        assert_eq!(parse_xs_duration("P1Y"), None);
        assert_eq!(parse_xs_duration("24H"), None);
        assert_eq!(parse_xs_duration("PT1.5H"), None);
    }

    #[test]
    fn parse_idp_metadata_rejects_entity_id_mismatch() {
        let signed = signed_rd_metadata("urn:nl-eid-gdi:1.0:RD:00000004000000149000:entities:9002");
        let err = parse_idp_metadata(
            &signed,
            &test_trust("urn:nl-eid-gdi:1.0:RD:00000004000000149000:entities:0001"),
        )
        .unwrap_err();
        assert!(matches!(err, AuthError::Crypto(_)));
    }

    fn metadata_xml(key_descriptors: &str) -> String {
        format!(
            r#"<md:EntityDescriptor xmlns:md="urn:oasis:names:tc:SAML:2.0:metadata" entityID="urn:test:rd">
                <md:IDPSSODescriptor>{key_descriptors}</md:IDPSSODescriptor>
            </md:EntityDescriptor>"#
        )
    }

    #[test]
    fn extracts_signing_key() {
        let xml = metadata_xml(
            r#"
            <md:KeyDescriptor use="signing">
                <ds:KeyInfo xmlns:ds="http://www.w3.org/2000/09/xmldsig#"><ds:X509Data>
                    <ds:X509Certificate>Y2VydDE=</ds:X509Certificate>
                </ds:X509Data></ds:KeyInfo>
            </md:KeyDescriptor>"#,
        );

        let doc = crate::saml::xml_parser::parse(&xml).unwrap();
        let keys = extract_idp_keys(&doc, doc.document_element());
        assert_eq!(keys.signing.len(), 1);
        assert_eq!(keys.encryption.len(), 0);
        assert!(!keys.signing[0].key_name.as_str().is_empty());
        assert!(!keys.signing[0].cert_base64.as_str().is_empty());
    }

    #[test]
    fn extracts_encryption_key() {
        let xml = metadata_xml(
            r#"
            <md:KeyDescriptor use="encryption">
                <ds:KeyInfo xmlns:ds="http://www.w3.org/2000/09/xmldsig#"><ds:X509Data>
                    <ds:X509Certificate>Y2VydDI=</ds:X509Certificate>
                </ds:X509Data></ds:KeyInfo>
            </md:KeyDescriptor>"#,
        );

        let doc = crate::saml::xml_parser::parse(&xml).unwrap();
        let keys = extract_idp_keys(&doc, doc.document_element());
        assert_eq!(keys.signing.len(), 0);
        assert_eq!(keys.encryption.len(), 1);
    }

    #[test]
    fn no_use_attr_goes_to_both() {
        let xml = metadata_xml(
            r#"
            <md:KeyDescriptor>
                <ds:KeyInfo xmlns:ds="http://www.w3.org/2000/09/xmldsig#"><ds:X509Data>
                    <ds:X509Certificate>Y2VydDM=</ds:X509Certificate>
                </ds:X509Data></ds:KeyInfo>
            </md:KeyDescriptor>"#,
        );

        let doc = crate::saml::xml_parser::parse(&xml).unwrap();
        let keys = extract_idp_keys(&doc, doc.document_element());
        assert_eq!(keys.signing.len(), 1);
        assert_eq!(keys.encryption.len(), 1);
    }

    #[test]
    fn skips_key_descriptor_without_cert() {
        let xml = metadata_xml(
            r#"
            <md:KeyDescriptor use="signing">
                <ds:KeyInfo xmlns:ds="http://www.w3.org/2000/09/xmldsig#"><ds:KeyName>key-only</ds:KeyName></ds:KeyInfo>
            </md:KeyDescriptor>"#,
        );

        let doc = crate::saml::xml_parser::parse(&xml).unwrap();
        let keys = extract_idp_keys(&doc, doc.document_element());
        assert_eq!(keys.signing.len(), 0);
    }

    #[test]
    fn empty_metadata_yields_no_keys() {
        let xml = metadata_xml("");
        let doc = crate::saml::xml_parser::parse(&xml).unwrap();
        let keys = extract_idp_keys(&doc, doc.document_element());
        assert_eq!(keys.signing.len(), 0);
        assert_eq!(keys.encryption.len(), 0);
    }

    #[test]
    fn endpoint_location_picks_matching_binding() {
        let xml = r#"<md:EntityDescriptor xmlns:md="urn:oasis:names:tc:SAML:2.0:metadata" entityID="urn:e">
            <md:IDPSSODescriptor>
                <md:SingleSignOnService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Redirect" Location="https://r/redirect"/>
                <md:SingleSignOnService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST" Location="https://r/post"/>
            </md:IDPSSODescriptor>
        </md:EntityDescriptor>"#;
        let doc = crate::saml::xml_parser::parse(xml).unwrap();
        let root = doc.document_element();
        let url = endpoint_location(&doc, root, "SingleSignOnService", BINDING_HTTP_POST);
        assert_eq!(url.as_deref(), Some("https://r/post"));
    }

    #[test]
    fn parse_idp_metadata_rejects_missing_entity_id() {
        let xml = r#"<md:EntityDescriptor xmlns:md="urn:oasis:names:tc:SAML:2.0:metadata"></md:EntityDescriptor>"#;
        let err = parse_idp_metadata(xml, &test_trust("urn:test:rd")).unwrap_err();
        assert!(matches!(err, AuthError::Xml(_)));
    }

    #[test]
    fn metadata_cache_path_appends_filename() {
        let path = metadata_cache_path(Path::new("/tmp/certs"));
        assert_eq!(path, Path::new("/tmp/certs/rd-metadata.xml"));
    }

    fn unique_temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("idp-meta-test-{}", uuid::Uuid::new_v4()))
    }

    #[tokio::test]
    async fn load_cached_returns_none_when_absent() {
        // No cache file written, so no fallback available.
        assert!(
            load_cached_idp_metadata(&unique_temp_dir(), &test_trust("urn:test:rd"))
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn load_cached_returns_none_past_cache_duration() {
        let dir = unique_temp_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let path = metadata_cache_path(&dir);
        std::fs::write(
            &path,
            signed_rd_metadata_attrs("urn:test:rd", r#" cacheDuration="PT1H""#),
        )
        .unwrap();
        let trust = test_trust("urn:test:rd");

        assert!(
            load_cached_idp_metadata(&dir, &trust).await.is_some(),
            "a fresh cache is used"
        );

        let two_hours_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 3600);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(two_hours_ago)
            .unwrap();
        assert!(
            load_cached_idp_metadata(&dir, &trust).await.is_none(),
            "a cache past its cacheDuration is not trusted"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn load_cached_returns_none_for_invalid_document() {
        // A cache file that fails to parse/verify is ignored, not surfaced.
        let dir = unique_temp_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(metadata_cache_path(&dir), "<not-metadata/>").unwrap();
        assert!(
            load_cached_idp_metadata(&dir, &test_trust("urn:test:rd"))
                .await
                .is_none()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
