//! Server startup and graceful shutdown for the Axum application.
//! Supports plain HTTP via `axum::serve`, and (with the `tls` feature) HTTPS
//! via `axum-server` with rustls. Called from binaries to run the router
//! with AppState.

use std::net::SocketAddr;

use axum::Router;
use tokio::{net::TcpListener, signal};

use crate::{AppError, Config, ProxyProtocolPeers};

/// Start the HTTP(S) server. With the `tls` feature, dispatches to the
/// rustls-backed server when `config.tls` is `Some`; otherwise serves plain
/// HTTP via `axum::serve`. `proxies` send a PROXY protocol header; only the
/// TLS server reads it.
pub async fn serve(
    router: Router,
    listener: TcpListener,
    config: &Config,
    proxies: Option<ProxyProtocolPeers>,
) -> Result<(), AppError> {
    #[cfg(feature = "tls")]
    if let Some(tls_config) = config.tls.as_ref() {
        let rustls_config = build_rustls_config(tls_config).await?;
        return serve_tls(router, listener, rustls_config, proxies).await;
    }

    #[cfg(not(feature = "tls"))]
    let _ = config;

    if proxies.is_some() {
        return Err(AppError::ConfigLoadError(
            "the PROXY protocol header is only read by the TLS server".to_string(),
        ));
    }

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
    use std::{future::Future, io, net::SocketAddr, pin::Pin, sync::Arc, time::Duration};

    use axum::{
        Router,
        extract::ConnectInfo,
        http::{HeaderValue, header},
    };
    use axum_server::{
        Handle,
        accept::Accept,
        tls_rustls::{RustlsAcceptor, RustlsConfig},
    };
    use rustls::{
        ServerConfig,
        crypto::aws_lc_rs::{self, cipher_suite, kx_group},
        pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    };
    use tokio::{
        io::AsyncReadExt,
        net::{TcpListener, TcpStream},
        time::timeout,
    };
    use tower_http::{add_extension::AddExtension, set_header::SetResponseHeaderLayer};
    use tracing::{debug, warn};

    use crate::{
        AppError, ProxyProtocolPeers, TlsConfig,
        core::proxy_protocol::{PREFIX_LEN, ProxyHeader, parse_v2, payload_len},
    };

    /// A listed proxy must send its header within this time.
    const PROXY_HEADER_TIMEOUT: Duration = Duration::from_secs(5);

    /// Rustls listener config from the configured cert/key files; the ACME
    /// renewer hot-reloads renewed certificates through a clone.
    pub async fn build_rustls_config(tls: &TlsConfig) -> Result<RustlsConfig, AppError> {
        Ok(RustlsConfig::from_config(Arc::new(
            build_server_config(tls).await?,
        )))
    }

    /// Serve HTTPS with a pre-built [`RustlsConfig`] (HSTS + graceful
    /// shutdown). Connections from `proxies` open with a PROXY protocol v2
    /// header naming the client.
    pub async fn serve_tls(
        router: Router,
        listener: TcpListener,
        config: RustlsConfig,
        proxies: Option<ProxyProtocolPeers>,
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

        // The acceptor records the peer address before the TLS handshake.
        let acceptor = RustlsAcceptor::new(config).acceptor(PeerAddressAcceptor::new(proxies));
        axum_server::from_tcp(std_listener)
            .map_err(AppError::ServerError)?
            .acceptor(acceptor)
            .handle(handle)
            .serve(router.into_make_service())
            .await
            .map_err(AppError::ServerError)?;

        Ok(())
    }

    /// Records each connection's peer as [`ConnectInfo`]: the TCP peer, or
    /// the client a listed proxy announces. Runs on the raw stream, before
    /// TLS. A listed proxy without a header is dropped; an unlisted peer's
    /// header is left for TLS to reject.
    #[derive(Clone)]
    pub(crate) struct PeerAddressAcceptor {
        proxies: Option<Arc<ProxyProtocolPeers>>,
    }

    impl PeerAddressAcceptor {
        pub(crate) fn new(proxies: Option<ProxyProtocolPeers>) -> Self {
            Self {
                proxies: proxies.map(Arc::new),
            }
        }
    }

    impl<S> Accept<TcpStream, S> for PeerAddressAcceptor
    where
        S: Send + 'static,
    {
        type Stream = TcpStream;
        type Service = AddExtension<S, ConnectInfo<SocketAddr>>;
        type Future = Pin<Box<dyn Future<Output = io::Result<(TcpStream, Self::Service)>> + Send>>;

        fn accept(&self, mut stream: TcpStream, service: S) -> Self::Future {
            let proxies = self.proxies.clone();
            Box::pin(async move {
                let tcp_peer = stream.peer_addr()?;
                let peer = match proxies {
                    Some(proxies) if proxies.contains(tcp_peer.ip()) => {
                        read_proxy_header(&mut stream, tcp_peer).await?
                    }
                    _ => tcp_peer,
                };
                let service = AddExtension::new(service, ConnectInfo(peer));
                Ok((stream, service))
            })
        }
    }

    /// Reads a listed proxy's header and returns the announced client, or
    /// the TCP peer for `LOCAL`. Rejections log
    /// `event = "csb.proxy_protocol_rejected"`.
    async fn read_proxy_header(
        stream: &mut TcpStream,
        tcp_peer: SocketAddr,
    ) -> io::Result<SocketAddr> {
        let read = timeout(PROXY_HEADER_TIMEOUT, async {
            let mut prefix = [0u8; PREFIX_LEN];
            let first = stream.read(&mut prefix).await?;
            if first == 0 {
                return Ok(None);
            }
            stream.read_exact(&mut prefix[first..]).await?;
            let len = payload_len(&prefix).map_err(io::Error::other)?;
            let mut payload = vec![0u8; len];
            stream.read_exact(&mut payload).await?;
            parse_v2(&prefix, &payload)
                .map(Some)
                .map_err(io::Error::other)
        })
        .await;

        let error = match read {
            Ok(Ok(Some(ProxyHeader::Proxied(client)))) => return Ok(client),
            Ok(Ok(Some(ProxyHeader::Local))) => return Ok(tcp_peer),
            // A connection closed without a byte (port probe) is no alert.
            Ok(Ok(None)) => {
                debug!(peer = %tcp_peer, "proxy closed the connection before the PROXY header");
                return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
            }
            Ok(Err(error)) => error,
            Err(_elapsed) => io::Error::new(
                io::ErrorKind::TimedOut,
                format!("no PROXY protocol header within {PROXY_HEADER_TIMEOUT:?}"),
            ),
        };
        warn!(
            event = "csb.proxy_protocol_rejected",
            peer = %tcp_peer,
            error = %error,
            "connection from a listed proxy without a valid PROXY protocol header refused"
        );
        Err(error)
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
    use std::{net::SocketAddr, path::PathBuf, time::Duration};

    use axum::{Router, body::Body, extract::ConnectInfo, http::Request, routing::get};
    use axum_server::accept::Accept;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        time::sleep,
    };
    use tower::ServiceExt;

    use super::tls::PeerAddressAcceptor;
    use crate::{
        Config, TlsConfig,
        core::proxy_protocol::test_support::{local_header, proxy_header},
        test_utils::response_body_string,
    };

    /// A router whose one route answers with its peer address.
    fn peer_echo() -> Router {
        Router::new().route(
            "/peer",
            get(|ConnectInfo(peer): ConnectInfo<SocketAddr>| async move { peer.to_string() }),
        )
    }

    /// A connected client and the server side of that connection.
    async fn connected_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let client = TcpStream::connect(listener.local_addr().expect("addr"))
            .await
            .expect("connect");
        let (server, _) = listener.accept().await.expect("accept");
        (client, server)
    }

    /// Runs the acceptor and returns the recorded peer and the remaining
    /// stream bytes.
    async fn accept_and_echo(
        acceptor: &PeerAddressAcceptor,
        server: TcpStream,
    ) -> std::io::Result<(String, Vec<u8>)> {
        let (mut stream, service) = acceptor.accept(server, peer_echo()).await?;
        let request = Request::get("/peer").body(Body::empty()).expect("request");
        let response = service.oneshot(request).await.expect("response");
        let peer = response_body_string(response).await;
        let mut rest = vec![0; 5];
        stream.read_exact(&mut rest).await?;
        Ok((peer, rest))
    }

    fn proxies(raw: &str) -> PeerAddressAcceptor {
        PeerAddressAcceptor::new(Some(raw.parse().expect("peers")))
    }

    /// A listed proxy's header names the client; later bytes stay in the
    /// stream.
    #[tokio::test]
    async fn acceptor_takes_the_client_from_a_listed_proxy() {
        let (mut client, server) = connected_pair().await;
        let mut bytes = proxy_header(
            "203.0.113.7:51234".parse().unwrap(),
            "172.16.12.1:8443".parse().unwrap(),
            &[],
        );
        bytes.extend_from_slice(b"hello");
        client.write_all(&bytes).await.unwrap();

        let (peer, rest) = accept_and_echo(&proxies("127.0.0.0/8"), server)
            .await
            .expect("accepted");

        assert_eq!(peer, "203.0.113.7:51234");
        assert_eq!(rest, b"hello");
    }

    /// A LOCAL header keeps the proxy's own address.
    #[tokio::test]
    async fn acceptor_keeps_the_tcp_peer_for_a_local_header() {
        let (mut client, server) = connected_pair().await;
        let mut bytes = local_header();
        bytes.extend_from_slice(b"hello");
        client.write_all(&bytes).await.unwrap();
        let tcp_peer = client.local_addr().unwrap();

        let (peer, rest) = accept_and_echo(&proxies("127.0.0.1"), server)
            .await
            .expect("accepted");

        assert_eq!(peer, tcp_peer.to_string());
        assert_eq!(rest, b"hello");
    }

    /// An unlisted peer's header is not read, so it cannot pick its address.
    #[tokio::test]
    async fn acceptor_leaves_an_unlisted_peer_untouched() {
        for acceptor in [PeerAddressAcceptor::new(None), proxies("203.0.113.9")] {
            let (mut client, server) = connected_pair().await;
            client.write_all(b"hello").await.unwrap();
            let tcp_peer = client.local_addr().unwrap();

            let (peer, rest) = accept_and_echo(&acceptor, server).await.expect("accepted");

            assert_eq!(peer, tcp_peer.to_string());
            assert_eq!(rest, b"hello");
        }
    }

    /// A listed proxy without a valid header is refused.
    #[tokio::test]
    async fn acceptor_refuses_a_listed_proxy_without_a_header() {
        let (mut client, server) = connected_pair().await;
        client
            .write_all(b"\x16\x03\x01 a TLS client hello")
            .await
            .unwrap();
        let err = accept_and_echo(&proxies("127.0.0.1"), server)
            .await
            .expect_err("refused");
        assert_eq!(err.kind(), std::io::ErrorKind::Other, "{err}");

        let (client, server) = connected_pair().await;
        drop(client);
        let err = accept_and_echo(&proxies("127.0.0.1"), server)
            .await
            .expect_err("refused");
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof, "{err}");
    }

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

        let err = super::serve(axum::Router::new(), listener, &config, None)
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
            tokio::spawn(async move { super::serve(peer_echo(), listener, &config, None).await });

        // Hit the server over HTTPS — getting any response proves the server
        // is speaking TLS, not merely accepting TCP. The fixture cert is
        // self-signed, so cert validation is disabled.
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap();

        let mut peer = None;
        for _ in 0..50 {
            if let Ok(response) = client.get(format!("https://{addr}/peer")).send().await {
                peer = Some(response.text().await.unwrap());
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
        // The acceptor recorded the TCP peer for the handler.
        let peer = peer.expect("HTTPS request to server never succeeded");
        assert!(peer.starts_with("127.0.0.1:"), "{peer}");
        assert!(!server.is_finished(), "server exited early");

        server.abort();
        let _ = server.await;
    }
}
