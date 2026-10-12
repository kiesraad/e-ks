//! ACME (Let's Encrypt) certificate renewal via http-01.
//!
//! Each instance renews its own certificate and hot-reloads the server;
//! challenge tokens live in the database so any instance behind the load
//! balancer can answer a validation request. The shared account is deployed
//! as configuration (`ACME_ACCOUNT_CREDENTIALS`), created once with the
//! `create_acme_account` tool. A first boot without cert/key files gets a
//! self-signed placeholder. Apply `deploy/schema.sql` manually before
//! enabling.

mod account;
mod acme_db;
mod acme_store;
mod bootstrap;
mod challenge;
mod renewer;

use axum_server::tls_rustls::RustlsConfig;

use crate::{AcmeConfig, AppError, TlsConfig, server};

pub use account::{create_acme_account, parse_acme_account_credentials};
pub(crate) use acme_store::AcmeStore;
pub(crate) use challenge::acme_challenge_router;

/// Bootstraps the certificate and spawns its renewer; returns the TLS config
/// to serve it with, which the renewer hot-reloads.
pub async fn start_acme_renewal(
    acme: AcmeConfig,
    tls: TlsConfig,
    store: AcmeStore,
) -> Result<RustlsConfig, AppError> {
    bootstrap::bootstrap_certificate(&acme, &tls).await?;
    let rustls_config = server::build_rustls_config(&tls).await?;
    tokio::spawn(renewer::run_acme_renewer(
        acme,
        tls,
        rustls_config.clone(),
        store,
    ));
    Ok(rustls_config)
}

#[cfg(test)]
mod tests {
    use rustls::pki_types::{CertificateDer, pem::PemObject};
    use tokio::net::TcpListener;

    use super::*;

    #[tokio::test]
    async fn start_acme_renewal_serves_the_bootstrapped_certificate() {
        let dir = std::env::temp_dir().join(format!("eks-acme-start-{}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let acme = AcmeConfig {
            directory_url: "https://acme.example/dir".to_string(),
            domain: "eks.example.nl".to_string(),
            // Invalid, so the spawned renewer fails before reaching the network.
            account_credentials: secrecy::SecretString::from("{}"),
            root_ca_path: None,
        };
        let tls = TlsConfig {
            cert_path: dir.join("cert.pem"),
            key_path: dir.join("key.pem"),
        };

        let rustls_config = start_acme_renewal(acme, tls.clone(), AcmeStore::default())
            .await
            .unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(server::serve_tls(
            axum::Router::new(),
            listener,
            rustls_config,
        ));

        let client = reqwest::Client::builder()
            .tls_danger_accept_invalid_certs(true)
            .tls_info(true)
            .build()
            .unwrap();
        let resp = client.get(format!("https://{addr}/")).send().await.unwrap();
        let presented = resp
            .extensions()
            .get::<reqwest::tls::TlsInfo>()
            .and_then(|info| info.peer_certificate())
            .expect("peer certificate")
            .to_vec();

        let bootstrapped = CertificateDer::from_pem_file(&tls.cert_path).unwrap();
        assert_eq!(presented, bootstrapped.to_vec());

        server.abort();
        tokio::fs::remove_dir_all(&dir).await.unwrap();
    }
}
