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
