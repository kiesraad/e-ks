//! Server startup and graceful shutdown for the Axum application.
//! Supports plain HTTP via `axum::serve`, and (with the `tls` feature) HTTPS
//! via `axum-server` with rustls. Called from binaries to run the router
//! with AppState.

use std::net::SocketAddr;

use axum::Router;
use tokio::{net::TcpListener, signal};

use crate::{AppError, Config};

/// Start the HTTP(S) server. With the `tls` feature, dispatches to the
/// rustls-backed server when `config.tls` is `Some`; otherwise serves plain
/// HTTP via `axum::serve`.
pub async fn serve(router: Router, listener: TcpListener, config: &Config) -> Result<(), AppError> {
    #[cfg(feature = "tls")]
    if let Some(tls_config) = config.tls.as_ref() {
        let rustls_config = build_rustls_config(tls_config).await?;
        return serve_tls(router, listener, rustls_config).await;
    }

    #[cfg(not(feature = "tls"))]
    let _ = config;

    let addr = listener.local_addr().map_err(AppError::ServerError)?;

    tracing::info!("Starting server on http://{addr}");

    // Peer addresses feed the CSB IP allow list.
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .map_err(AppError::ServerError)?;

    Ok(())
}

#[cfg(feature = "acme")]
pub(crate) use tls::server_config_from_pem;
#[cfg(feature = "tls")]
pub use tls::{build_rustls_config, serve_tls};

#[cfg(feature = "tls")]
mod tls {
    #[cfg(not(debug_assertions))]
    use std::time::Duration;
    use std::{net::SocketAddr, sync::Arc};

    use axum::{
        Router,
        http::{HeaderValue, header},
    };
    use axum_server::{Handle, tls_rustls::RustlsConfig};
    use rustls::{
        ServerConfig,
        crypto::aws_lc_rs::{self, cipher_suite, kx_group},
        pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    };
    use tokio::net::TcpListener;
    use tower_http::set_header::SetResponseHeaderLayer;

    use crate::{AppError, TlsConfig};

    /// Rustls listener config from the configured cert/key files; the ACME
    /// renewer hot-reloads renewed certificates through a clone.
    pub async fn build_rustls_config(tls: &TlsConfig) -> Result<RustlsConfig, AppError> {
        Ok(RustlsConfig::from_config(Arc::new(
            build_server_config(tls).await?,
        )))
    }

    /// Serve HTTPS with a pre-built [`RustlsConfig`] (HSTS + graceful shutdown).
    pub async fn serve_tls(
        router: Router,
        listener: TcpListener,
        config: RustlsConfig,
    ) -> Result<(), AppError> {
        let addr = listener.local_addr().map_err(AppError::ServerError)?;

        // HSTS: served only over TLS, so the header is meaningful here.
        // 1 year + includeSubDomains matches NCSC HTTPS guidance; preload is
        // intentionally omitted (requires explicit registration).
        // `overriding`, so no handler can hand out a shorter max-age.
        let router = router.layer(SetResponseHeaderLayer::overriding(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        ));

        tracing::info!("Starting server on https://{addr}");

        let std_listener = listener.into_std().map_err(AppError::ServerError)?;

        let handle: Handle<SocketAddr> = Handle::new();
        tokio::spawn(graceful_shutdown_handle(handle.clone()));

        axum_server::from_tcp_rustls(std_listener, config)
            .map_err(AppError::ServerError)?
            .handle(handle)
            .serve(router.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .map_err(AppError::ServerError)?;

        Ok(())
    }

    async fn build_server_config(tls: &TlsConfig) -> Result<ServerConfig, AppError> {
        let cert_bytes = tokio::fs::read(&tls.cert_path)
            .await
            .map_err(AppError::ServerError)?;
        let key_bytes = tokio::fs::read(&tls.key_path)
            .await
            .map_err(AppError::ServerError)?;

        server_config_from_pem(&cert_bytes, &key_bytes)
    }

    /// Builds a rustls `ServerConfig` aligned with NCSC TLS 2025-05:
    /// TLS 1.3 only ("Goed"), AEAD cipher suites, post-quantum hybrid key
    /// exchange (X25519MLKEM768) preferred.
    pub(crate) fn server_config_from_pem(
        cert_bytes: &[u8],
        key_bytes: &[u8],
    ) -> Result<ServerConfig, AppError> {
        let cert_chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(cert_bytes)
            .collect::<Result<_, _>>()
            .map_err(|e| AppError::ConfigLoadError(format!("invalid TLS cert PEM: {e}")))?;
        let key = PrivateKeyDer::from_pem_slice(key_bytes)
            .map_err(|e| AppError::ConfigLoadError(format!("invalid TLS key PEM: {e}")))?;

        let mut provider = aws_lc_rs::default_provider();
        // TLS 1.3 AEAD cipher suites: Goed first, Voldoende last.
        provider.cipher_suites = vec![
            cipher_suite::TLS13_AES_256_GCM_SHA384,
            cipher_suite::TLS13_CHACHA20_POLY1305_SHA256,
            cipher_suite::TLS13_AES_128_GCM_SHA256,
        ];
        // X25519MLKEM768 is "Goed" (post-quantum hybrid); the rest are "Voldoende".
        provider.kx_groups = vec![
            kx_group::X25519MLKEM768,
            kx_group::X25519,
            kx_group::SECP256R1,
            kx_group::SECP384R1,
        ];

        ServerConfig::builder_with_provider(Arc::new(provider))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|e| AppError::ConfigLoadError(format!("invalid TLS protocol versions: {e}")))?
            .with_no_client_auth()
            .with_single_cert(cert_chain, key)
            .map_err(|e| AppError::ConfigLoadError(format!("invalid TLS cert/key: {e}")))
    }

    async fn graceful_shutdown_handle(handle: Handle<SocketAddr>) {
        super::wait_for_signal().await;
        #[cfg(not(debug_assertions))]
        {
            tracing::info!("Received shutdown signal, gracefully shutting down.");
            handle.graceful_shutdown(Some(Duration::from_secs(10)));
        }
        #[cfg(debug_assertions)]
        {
            let _ = handle;
            tracing::info!("Received shutdown signal, no graceful shutdown in development mode.");
            std::process::exit(0);
        }
    }
}

async fn wait_for_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

#[cfg(not(debug_assertions))]
async fn shutdown_signal() {
    wait_for_signal().await;
    tracing::info!("Received shutdown signal, gracefully shutting down.");
}

#[cfg(debug_assertions)]
async fn shutdown_signal() {
    wait_for_signal().await;
    tracing::info!("Received shutdown signal, no graceful shutdown in development mode.");
    std::process::exit(0);
}

#[cfg(all(test, feature = "tls"))]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use tokio::{net::TcpListener, time::sleep};

    use crate::{Config, TlsConfig};

    fn fixture_tls() -> TlsConfig {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/fixtures/tls");
        TlsConfig {
            cert_path: root.join("cert.pem"),
            key_path: root.join("key.pem"),
        }
    }

    fn config_with_tls(tls: TlsConfig) -> Config {
        let mut config = Config::new_test();
        config.tls = Some(tls);
        config
    }

    #[test]
    fn server_config_from_pem_accepts_fixture_and_rejects_garbage() {
        let tls = fixture_tls();
        let cert = std::fs::read(&tls.cert_path).unwrap();
        let key = std::fs::read(&tls.key_path).unwrap();

        super::tls::server_config_from_pem(&cert, &key).expect("fixture cert/key");
        assert!(super::tls::server_config_from_pem(b"garbage", &key).is_err());
        assert!(super::tls::server_config_from_pem(&cert, b"garbage").is_err());
    }

    #[tokio::test]
    async fn serve_errors_for_missing_cert_file() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = config_with_tls(TlsConfig {
            cert_path: PathBuf::from("/nonexistent/cert.pem"),
            key_path: PathBuf::from("/nonexistent/key.pem"),
        });

        let err = super::serve(axum::Router::new(), listener, &config)
            .await
            .expect_err("missing cert");
        assert!(matches!(err, crate::AppError::ServerError(_)));
    }

    #[tokio::test]
    async fn serve_starts_with_valid_cert_and_accepts_https_connections() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let config = config_with_tls(fixture_tls());

        let server =
            tokio::spawn(async move { super::serve(axum::Router::new(), listener, &config).await });

        // Hit the server over HTTPS — getting any response proves the server
        // is speaking TLS, not merely accepting TCP. The fixture cert is
        // self-signed, so cert validation is disabled.
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap();

        let mut https_ok = false;
        for _ in 0..50 {
            if client.get(format!("https://{addr}/")).send().await.is_ok() {
                https_ok = true;
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
        assert!(https_ok, "HTTPS request to server never succeeded");
        assert!(!server.is_finished(), "server exited early");

        server.abort();
        let _ = server.await;
    }
}
