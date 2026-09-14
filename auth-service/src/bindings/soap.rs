//! SAML SOAP binding: the mTLS back-channel used for artifact resolution
//! (eID §7.5, §9.4).

use crate::{
    config::TlsConfig,
    error::{AuthError, Result},
    saml::{
        constants::NS_SOAP,
        xml_parser::{Document, NodeId, find_descendant},
    },
    types::EndpointUrl,
};
use std::{
    path::PathBuf,
    sync::{Mutex, PoisonError},
    time::Duration,
};
use tokio::fs;
use tracing::debug;

/// Connection-establishment ceiling for the mTLS back-channel.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Overall request ceiling (connect + send + receive) for one ArtifactResolve
/// round-trip, so a slow or hung RD cannot tie up the handler indefinitely
/// (slow-loris / stuck socket). eID §9.5 reasons in ~30s clock-skew terms, so a
/// 30s ceiling is ample for a synchronous SOAP exchange.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Max response body we buffer from the back-channel / metadata fetch, bounding
/// memory from an oversized or hostile response. SAML responses are a few KB.
pub(crate) const MAX_HTTP_BODY_BYTES: usize = 5 * 1024 * 1024;

/// Process-wide cache of the built mTLS client.
///
/// `reqwest::Client` is internally reference-counted and designed to be built
/// once and reused; rebuilding it would re-read the cert/key from disk and redo
/// the TLS setup on every artifact resolution. Keyed on the
/// (cert, key) paths so a configuration change still forces a rebuild, and a
/// build failure is never cached (a transient read error is retried next call).
/// A cert rotated in place under the same path is picked up on the next process
/// start, matching the deployment model (the DV mTLS identity rotates via
/// redeploy).
///
/// The lock is never held across an await: a miss releases it, builds the
/// client, then stores it. Two concurrent first callers may both build one;
/// the later store wins and the other client is simply dropped.
static MTLS_CLIENT: Mutex<Option<(PathBuf, PathBuf, reqwest::Client)>> = Mutex::new(None);

/// The mTLS client for `tls`, built once and then reused across requests.
async fn mtls_client(tls: &TlsConfig) -> Result<reqwest::Client> {
    if let Some(client) = cached_mtls_client(tls) {
        return Ok(client);
    }
    let client = build_mtls_client(tls).await?;
    let mut cache = MTLS_CLIENT.lock().unwrap_or_else(PoisonError::into_inner);
    *cache = Some((
        tls.client_cert.clone(),
        tls.client_key.clone(),
        client.clone(),
    ));
    Ok(client)
}

/// The cached client, if one was built for exactly these cert/key paths.
fn cached_mtls_client(tls: &TlsConfig) -> Option<reqwest::Client> {
    let cache = MTLS_CLIENT.lock().unwrap_or_else(PoisonError::into_inner);
    let (cert, key, client) = cache.as_ref()?;
    (*cert == tls.client_cert && *key == tls.client_key).then(|| client.clone())
}

/// Build a reqwest async client configured for mTLS per eID §9.4.
///
/// eID §9.4: Back-channel requires mutual TLS with PKIoverheid certificates
/// (key length >= 2048 bits). TLS v1.2 or higher per NCSC directive.
async fn build_mtls_client(tls: &TlsConfig) -> Result<reqwest::Client> {
    debug!(
        "[soap] Building mTLS client: client_cert={}, client_key=<redacted>",
        tls.client_cert.display(),
    );
    let cert_pem = fs::read(&tls.client_cert)
        .await
        .map_err(|e| AuthError::Http(format!("Failed to read TLS client cert: {e}")))?;
    // SECURITY: never log key_pem bytes; it is the private key.
    let key_pem = fs::read(&tls.client_key)
        .await
        .map_err(|e| AuthError::Http(format!("Failed to read TLS client key: {e}")))?;

    let mut identity_pem = cert_pem;
    identity_pem.push(b'\n');
    identity_pem.extend_from_slice(&key_pem);
    let identity = reqwest::Identity::from_pem(&identity_pem)
        .map_err(|e| AuthError::Http(format!("Failed to build TLS identity: {e}")))?;

    let ca = reqwest::Certificate::from_pem(crate::saml::pki::BACKCHANNEL_ROOT_CA_PEM)
        .map_err(|e| AuthError::Http(format!("Failed to parse back-channel root CA: {e}")))?;

    reqwest::Client::builder()
        .identity(identity)
        .tls_certs_only([ca])
        // eID §9.4 / NCSC: TLS 1.2 or higher. rustls already refuses older
        // versions; pin it explicitly so the floor survives a backend change.
        .min_tls_version(reqwest::tls::Version::TLS_1_2)
        // Bound a slow/unresponsive RD so a back-channel call cannot hang a
        // request (or a tokio worker) indefinitely.
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| AuthError::Http(format!("Failed to build mTLS client: {e}")))
}

/// Read a response body into a `String`, rejecting anything larger than `max`
/// (checked by `Content-Length` up front, then while streaming so a chunked or
/// mislabeled body cannot exceed it either).
pub(crate) async fn read_body_capped(
    mut response: reqwest::Response,
    max: usize,
) -> Result<String> {
    if let Some(len) = response.content_length()
        && len > max as u64
    {
        return Err(AuthError::Http(format!(
            "response body too large: {len} bytes > {max} cap"
        )));
    }
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| AuthError::Http(format!("Failed to read response body: {e}")))?
    {
        if buf.len() + chunk.len() > max {
            return Err(AuthError::Http(format!(
                "response body exceeds {max} byte cap"
            )));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Send a SOAP request with mTLS and return the response body.
pub async fn send_soap_request(
    url: &EndpointUrl,
    soap_xml: &str,
    tls: &TlsConfig,
) -> Result<String> {
    debug!("[soap] POST {url} (request_body_len={})", soap_xml.len());
    let client = mtls_client(tls).await?;
    let response = client
        .post(url.as_str())
        .header("Content-Type", "text/xml; charset=utf-8")
        .header("SOAPAction", "\"\"")
        .body(soap_xml.to_string())
        .send()
        .await
        .map_err(|e| AuthError::Http(format!("SOAP request failed: {e}")))?;

    let status = response.status();
    debug!("[soap] {url} status={status}");
    // SECURITY: do not log the response body; it contains the (encrypted)
    // SAML Assertion. Only length information is emitted at debug level.
    let body = read_body_capped(response, MAX_HTTP_BODY_BYTES).await?;
    debug!("[soap] {url} response body received (len={})", body.len());

    if !status.is_success() {
        // The body is not included: like the success path, it may carry the
        // (encrypted) SAML Assertion, and this error string ends up in logs.
        return Err(AuthError::Http(format!(
            "SOAP request returned HTTP {status} (body_len={})",
            body.len()
        )));
    }

    Ok(body)
}

/// Locate the first child element of the SOAP `<Body>` (the ArtifactResponse)
/// as a node in the already-parsed document.
///
/// The Body is matched by `(SOAP-envelope namespace, "Body")`, so any prefix
/// bound to the SOAP 1.1 envelope namespace works, not just `soapenv:`. Returns
/// the node so the caller navigates the single parsed tree (no re-parse).
pub fn unwrap_soap(doc: &Document, root: NodeId) -> Option<NodeId> {
    let body = find_descendant(doc, root, NS_SOAP, "Body")?;
    doc.first_element_child(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::saml::{constants::NS_SAMLP, xml_parser::parse};

    fn root_of(doc: &Document) -> NodeId {
        doc.document_element()
    }

    #[test]
    fn unwrap_soap_extracts_body() {
        let xml = format!(
            r#"<soapenv:Envelope xmlns:soapenv="{NS_SOAP}"><soapenv:Body><samlp:ArtifactResponse xmlns:samlp="{NS_SAMLP}" ID="_1">content</samlp:ArtifactResponse></soapenv:Body></soapenv:Envelope>"#
        );
        let doc = parse(&xml).unwrap();
        let body_child = unwrap_soap(&doc, root_of(&doc)).unwrap();
        assert_eq!(doc.local_name(body_child), Some("ArtifactResponse"));
    }

    #[test]
    fn unwrap_soap_works_for_any_soap_prefix() {
        // Namespace-aware: any prefix bound to the SOAP envelope namespace works,
        // not just the literal `soapenv:` prefix.
        let xml = format!(
            r#"<SOAP-ENV:Envelope xmlns:SOAP-ENV="{NS_SOAP}"><SOAP-ENV:Body><samlp:ArtifactResponse xmlns:samlp="{NS_SAMLP}">x</samlp:ArtifactResponse></SOAP-ENV:Body></SOAP-ENV:Envelope>"#
        );
        let doc = parse(&xml).unwrap();
        let body_child = unwrap_soap(&doc, root_of(&doc)).unwrap();
        assert_eq!(doc.local_name(body_child), Some("ArtifactResponse"));
    }

    #[test]
    fn unwrap_soap_returns_none_when_not_soap() {
        let doc = parse(r#"<not-soap xmlns="urn:x">bad</not-soap>"#).unwrap();
        assert!(unwrap_soap(&doc, root_of(&doc)).is_none());
    }

    #[test]
    fn unwrap_soap_returns_none_for_empty_body() {
        let xml = format!(
            r#"<soapenv:Envelope xmlns:soapenv="{NS_SOAP}"><soapenv:Body>   </soapenv:Body></soapenv:Envelope>"#
        );
        let doc = parse(&xml).unwrap();
        assert!(unwrap_soap(&doc, root_of(&doc)).is_none());
    }

    fn fixture_tls() -> TlsConfig {
        let dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures"));
        TlsConfig {
            client_cert: dir.join("dv-tls.pem"),
            client_key: dir.join("dv-tls-key.pem"),
        }
    }

    #[tokio::test]
    async fn mtls_client_builds_and_is_reused() {
        let tls = fixture_tls();
        // First call builds the client; the second must hit the cache (same
        // cert/key paths) and also succeed. Both build a real rustls mTLS
        // client from the fixture identity + pinned back-channel root, but do
        // no network I/O.
        assert!(mtls_client(&tls).await.is_ok(), "first build");
        assert!(cached_mtls_client(&tls).is_some(), "client is cached");
        assert!(mtls_client(&tls).await.is_ok(), "cached reuse");
    }
}
