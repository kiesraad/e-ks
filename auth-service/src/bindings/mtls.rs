//! rustls configuration of the mTLS SOAP back-channel (eID §9.4).
//!
//! reqwest's own builder can pin the root CA and present the client identity,
//! but it cannot look inside the server certificate. eID §9.4 requires the RD to
//! present a PKIoverheid certificate, and PKIoverheid certificates carry the
//! participant OIN in `Subject.serialNumber`, so on top of the webpki chain and
//! hostname checks the ARS server certificate must name the RD OIN, mirroring the
//! check on the RD metadata signing certificate. The check runs inside the TLS
//! handshake, so a server with the wrong OIN never receives the ArtifactResolve.

use crate::{
    error::{AuthError, Result},
    saml::pki::subject_oin,
};
use rustls::{
    ClientConfig, DigitallySignedStruct, DistinguishedName, RootCertStore, SignatureScheme,
    client::{
        WebPkiServerVerifier,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    crypto::CryptoProvider,
    pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime, pem::PemObject},
};
use std::sync::Arc;
use tracing::{debug, warn};

/// Build the back-channel TLS configuration: the pinned root CA, the RD-OIN
/// checking server verifier, TLS 1.2+ and the DV client identity.
///
/// All of this used to be reqwest builder calls; with a preconfigured backend
/// reqwest ignores its own `identity`/`tls_certs_only`/`min_tls_version`, so
/// everything the back-channel relies on is set here.
pub(crate) fn client_config(
    root_ca_pem: &[u8],
    expected_oin: &'static str,
    client_cert_pem: &[u8],
    client_key_pem: &[u8],
) -> Result<ClientConfig> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let verifier = server_cert_verifier(root_ca_pem, expected_oin, provider.clone())?;

    let client_chain: Vec<CertificateDer<'static>> =
        CertificateDer::pem_slice_iter(client_cert_pem)
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| AuthError::Http(format!("Failed to parse TLS client cert: {e}")))?;
    if client_chain.is_empty() {
        return Err(AuthError::Http(
            "TLS client cert file holds no certificate".to_string(),
        ));
    }
    // SECURITY: never log the key; the error carries only the parser's reason.
    let client_key = PrivateKeyDer::from_pem_slice(client_key_pem)
        .map_err(|e| AuthError::Http(format!("Failed to parse TLS client key: {e}")))?;

    let mut config = ClientConfig::builder_with_provider(provider)
        // eID §9.4 / NCSC: TLS 1.2 or higher; rustls' defaults are exactly 1.2 and 1.3.
        .with_safe_default_protocol_versions()
        .map_err(|e| AuthError::Http(format!("Failed to select TLS versions: {e}")))?
        // `dangerous` is rustls' name for any custom verifier. This one performs the
        // full webpki verification and only adds a check on top (see
        // `OinServerCertVerifier`).
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_client_auth_cert(client_chain, client_key)
        .map_err(|e| AuthError::Http(format!("Failed to build TLS identity: {e}")))?;
    // reqwest sets ALPN only on configs it builds itself; the SOAP binding is HTTP/1.1.
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}

/// The webpki verifier over the pinned root(s), wrapped in the RD OIN check.
fn server_cert_verifier(
    root_ca_pem: &[u8],
    expected_oin: &'static str,
    provider: Arc<CryptoProvider>,
) -> Result<Arc<dyn ServerCertVerifier>> {
    let mut roots = RootCertStore::empty();
    for cert in CertificateDer::pem_slice_iter(root_ca_pem) {
        let cert = cert
            .map_err(|e| AuthError::Http(format!("Failed to parse back-channel root CA: {e}")))?;
        roots
            .add(cert)
            .map_err(|e| AuthError::Http(format!("Invalid back-channel root CA: {e}")))?;
    }
    if roots.is_empty() {
        return Err(AuthError::Http(
            "back-channel root CA PEM holds no certificate".to_string(),
        ));
    }
    let inner = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider)
        .build()
        .map_err(|e| AuthError::Http(format!("Failed to build server cert verifier: {e}")))?;
    Ok(Arc::new(OinServerCertVerifier {
        inner,
        expected_oin,
    }))
}

/// Server certificate verifier for the ARS: webpki's chain, validity and
/// hostname verification, plus the requirement that the leaf certificate's
/// `Subject.serialNumber` is the RD OIN (eID §9.4, PKIoverheid).
///
/// Everything except [`ServerCertVerifier::verify_server_cert`] is delegated
/// unchanged, and that method only adds a check after the delegate succeeded, so
/// this can never accept a certificate webpki would reject.
#[derive(Debug)]
struct OinServerCertVerifier {
    inner: Arc<WebPkiServerVerifier>,
    expected_oin: &'static str,
}

impl OinServerCertVerifier {
    fn check_oin(&self, end_entity: &CertificateDer<'_>) -> std::result::Result<(), rustls::Error> {
        match subject_oin(end_entity) {
            Some(oin) if oin == self.expected_oin => {
                debug!("[mtls] ARS server cert carries the RD OIN");
                Ok(())
            }
            found => {
                warn!(
                    "[mtls] Rejecting ARS server cert: subject serialNumber (OIN) is {found:?}, \
                     expected {:?}",
                    self.expected_oin
                );
                Err(rustls::Error::InvalidCertificate(
                    rustls::CertificateError::ApplicationVerificationFailure,
                ))
            }
        }
    }
}

impl ServerCertVerifier for OinServerCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        let verified = self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        )?;
        self.check_oin(end_entity)?;
        Ok(verified)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }

    fn root_hint_subjects(&self) -> Option<&[DistinguishedName]> {
        self.inner.root_hint_subjects()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RD_OIN;
    use rustls::CertificateError;

    const CA: &[u8] = include_bytes!("../../fixtures/ca.pem");
    const RD_TLS: &[u8] = include_bytes!("../../fixtures/rd-tls.pem");
    /// Chains to the same test CA, but carries the DV's OIN, not the RD's.
    const DV_TLS: &[u8] = include_bytes!("../../fixtures/dv-tls.pem");
    const DV_TLS_KEY: &[u8] = include_bytes!("../../fixtures/dv-tls-key.pem");

    fn verifier(expected_oin: &'static str) -> Arc<dyn ServerCertVerifier> {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        server_cert_verifier(CA, expected_oin, provider).expect("verifier builds")
    }

    fn verify(
        verifier: &dyn ServerCertVerifier,
        leaf_pem: &[u8],
        host: &'static str,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        let leaf = CertificateDer::from_pem_slice(leaf_pem).expect("leaf PEM");
        verifier.verify_server_cert(
            &leaf,
            &[],
            &ServerName::try_from(host).expect("server name"),
            &[],
            UnixTime::now(),
        )
    }

    fn is_oin_failure(err: &rustls::Error) -> bool {
        matches!(
            err,
            rustls::Error::InvalidCertificate(CertificateError::ApplicationVerificationFailure)
        )
    }

    #[test]
    fn accepts_rd_server_cert_with_rd_oin() {
        assert!(verify(&*verifier(RD_OIN), RD_TLS, "localhost").is_ok());
    }

    #[test]
    fn rejects_chain_valid_cert_carrying_another_oin() {
        // dv-tls is valid for localhost and chains to the CA: only the OIN differs.
        let err = verify(&*verifier(RD_OIN), DV_TLS, "localhost").unwrap_err();
        assert!(is_oin_failure(&err), "{err:?}");
    }

    #[test]
    fn rejects_when_expected_oin_differs() {
        let err = verify(&*verifier("00000000000000000009"), RD_TLS, "localhost").unwrap_err();
        assert!(is_oin_failure(&err), "{err:?}");
    }

    #[test]
    fn webpki_checks_still_apply() {
        // A wrong hostname is refused by the delegate, before the OIN check.
        let err = verify(&*verifier(RD_OIN), RD_TLS, "not-the-ars.example").unwrap_err();
        assert!(
            matches!(
                err,
                rustls::Error::InvalidCertificate(CertificateError::NotValidForNameContext { .. })
            ),
            "{err:?}"
        );
        // Untrusted issuer: no roots match, even with the right OIN.
        let untrusted = {
            let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
            server_cert_verifier(DV_TLS, RD_OIN, provider).expect("any cert can be a root")
        };
        let err = verify(&*untrusted, RD_TLS, "localhost").unwrap_err();
        assert!(!is_oin_failure(&err), "{err:?}");
    }

    #[test]
    fn client_config_builds_from_fixtures() {
        let config = client_config(CA, RD_OIN, DV_TLS, DV_TLS_KEY).expect("config builds");
        assert_eq!(config.alpn_protocols, vec![b"http/1.1".to_vec()]);
    }

    #[test]
    fn client_config_rejects_empty_identity() {
        assert!(client_config(CA, RD_OIN, b"", DV_TLS_KEY).is_err());
        assert!(client_config(CA, RD_OIN, DV_TLS, b"").is_err());
        assert!(client_config(b"", RD_OIN, DV_TLS, DV_TLS_KEY).is_err());
    }
}
