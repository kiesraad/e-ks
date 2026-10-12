//! Background task that renews this instance's TLS certificate via ACME
//! http-01 and hot-reloads the running server.

use std::{path::Path, sync::Arc, time::Duration};

use axum_server::tls_rustls::RustlsConfig;
use chrono::{DateTime, Utc};
use instant_acme::{
    Account, AuthorizationStatus, CertificateIdentifier, ChallengeType, Identifier, NewOrder,
    Order, OrderStatus, RetryPolicy,
};
use rustls::pki_types::{CertificateDer, pem::PemObject};
use tokio::io::AsyncWriteExt;

use crate::{
    AcmeConfig, AppError, TlsConfig,
    acme::{AcmeStore, account},
    server,
};

/// Renew when NotAfter is this close; Let's Encrypt certificates last 90 days.
const RENEW_BEFORE_DAYS: i64 = 30;
/// Daily renewal check time (UTC hour).
const CHECK_HOUR_UTC: u32 = 2;
/// Failure backoff (seconds); the last value repeats.
const BACKOFF_SECS: [u64; 4] = [60, 300, 900, 3600];

/// Everything one renewal run works with: the ACME account and domain, where
/// the certificate lives on disk, the serving TLS handle to reload it into, and
/// the store that serves the http-01 challenge responses.
struct Renewal<'a> {
    acme: &'a AcmeConfig,
    tls: &'a TlsConfig,
    rustls_config: &'a RustlsConfig,
    store: &'a AcmeStore,
}

/// Renew this instance's certificate before it expires. `rustls_config` is a
/// clone of the serving handle, so renewals go live without a restart.
pub async fn run_acme_renewer(
    acme: AcmeConfig,
    tls: TlsConfig,
    rustls_config: RustlsConfig,
    store: AcmeStore,
) {
    let renewal = Renewal {
        acme: &acme,
        tls: &tls,
        rustls_config: &rustls_config,
        store: &store,
    };
    let mut consecutive_failures: usize = 0;

    loop {
        match renewal.renew_if_due().await {
            Ok(()) => {
                consecutive_failures = 0;
                let now = Utc::now();
                let wait = (next_check_at(now) - now).to_std().unwrap_or_default();
                tokio::time::sleep(wait).await;
            }
            Err(err) => {
                log_renewal_error(&err);
                let idx = consecutive_failures.min(BACKOFF_SECS.len() - 1);
                consecutive_failures = consecutive_failures.saturating_add(1);
                tokio::time::sleep(Duration::from_secs(BACKOFF_SECS[idx])).await;
            }
        }
    }
}

fn log_renewal_error(err: &AppError) {
    match err {
        AppError::DatabaseError(_) => tracing::error!(
            "ACME renewal failed: {err} (is the ACME schema installed? see deploy/schema.sql)"
        ),
        _ => tracing::error!("ACME renewal failed: {err}"),
    }
}

impl Renewal<'_> {
    async fn renew_if_due(&self) -> Result<(), AppError> {
        let cert_pem = tokio::fs::read(&self.tls.cert_path)
            .await
            .map_err(AppError::ServerError)?;
        let not_after = cert_not_after(&cert_pem)?;
        if !renewal_due(not_after, Utc::now()) {
            return Ok(());
        }

        tracing::info!(
            "TLS certificate for {} expires {not_after}; starting ACME renewal",
            self.acme.domain
        );
        self.renew(&cert_pem).await
    }

    async fn renew(&self, current_cert_pem: &[u8]) -> Result<(), AppError> {
        let account = account::load_account(self.acme).await?;
        let mut order = self.new_order(&account, current_cert_pem).await?;

        let mut tokens = Vec::new();
        let result = self.complete_order(&mut order, &mut tokens).await;

        // Tokens are single-use; clean up regardless of outcome.
        for token in &tokens {
            self.store.delete_challenge(token).await;
        }

        result
    }

    /// Order with an ARI `replaces` identifier (RFC 9773) when possible: it
    /// exempts the renewal from the duplicate-certificate rate limit.
    async fn new_order(
        &self,
        account: &Account,
        current_cert_pem: &[u8],
    ) -> Result<Order, AppError> {
        let identifiers = [Identifier::Dns(self.acme.domain.clone())];

        let current_cert = CertificateDer::pem_slice_iter(current_cert_pem)
            .next()
            .and_then(Result::ok);
        if let Some(cert) = &current_cert {
            match CertificateIdentifier::try_from(cert) {
                Ok(cert_id) => {
                    match account
                        .new_order(&NewOrder::new(&identifiers).replaces(cert_id))
                        .await
                    {
                        Ok(order) => return Ok(order),
                        Err(err) => tracing::info!(
                            "ACME order with ARI `replaces` failed ({err}); retrying without"
                        ),
                    }
                }
                Err(err) => tracing::info!(
                    "current certificate has no usable ARI identifier ({err}); ordering without"
                ),
            }
        }

        Ok(account.new_order(&NewOrder::new(&identifiers)).await?)
    }

    async fn complete_order(
        &self,
        order: &mut Order,
        tokens: &mut Vec<String>,
    ) -> Result<(), AppError> {
        self.answer_http_challenges(order, tokens).await?;

        let status = order.poll_ready(&RetryPolicy::default()).await?;
        if status != OrderStatus::Ready {
            return Err(order_failure(order, status).await);
        }

        let key_pem = order.finalize().await?;
        let cert_pem = order.poll_certificate(&RetryPolicy::default()).await?;

        self.install_certificate(&key_pem, &cert_pem).await
    }

    /// Answers every pending authorization on the order with an http-01
    /// challenge.
    ///
    /// Each challenge token is recorded in `tokens` so the caller can clean up.
    async fn answer_http_challenges(
        &self,
        order: &mut Order,
        tokens: &mut Vec<String>,
    ) -> Result<(), AppError> {
        let mut authorizations = order.authorizations();
        while let Some(result) = authorizations.next().await {
            let mut authz = result?;
            match authz.status {
                AuthorizationStatus::Valid => continue,
                AuthorizationStatus::Pending => {}
                status => {
                    return Err(AppError::ConfigLoadError(format!(
                        "unexpected ACME authorization status: {status:?}"
                    )));
                }
            }

            let mut challenge =
                authz
                    .challenge(ChallengeType::Http01)
                    .ok_or(AppError::AcmeError(instant_acme::Error::Str(
                        "server offered no http-01 challenge",
                    )))?;

            // Store the token before set_ready: the CA may validate immediately.
            self.store
                .put_challenge(&challenge.token, challenge.key_authorization().as_str())
                .await?;
            tokens.push(challenge.token.clone());

            challenge.set_ready().await?;
        }

        Ok(())
    }

    /// Installs the issued certificate on the running server and persists it
    /// best-effort.
    async fn install_certificate(&self, key_pem: &str, cert_pem: &str) -> Result<(), AppError> {
        // Same pinned builder as startup: a broken pair can never be installed.
        let new_config = server::server_config_from_pem(cert_pem.as_bytes(), key_pem.as_bytes())?;

        let persisted = async {
            write_atomic(&self.tls.key_path, key_pem.as_bytes(), 0o600).await?;
            write_atomic(&self.tls.cert_path, cert_pem.as_bytes(), 0o644).await
        }
        .await;
        if let Err(err) = persisted {
            tracing::warn!(
                "could not persist the renewed TLS certificate to {} / {}: {err}; \
                 it is only active in memory until the next restart",
                self.tls.cert_path.display(),
                self.tls.key_path.display()
            );
        }

        self.rustls_config.reload_from_config(Arc::new(new_config));

        let not_after = cert_not_after(cert_pem.as_bytes())
            .map(|t| t.to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        tracing::info!(
            "renewed TLS certificate for {}, valid until {not_after}",
            self.acme.domain
        );

        Ok(())
    }
}

/// NotAfter of the first (end-entity) certificate in a PEM bundle.
pub(super) fn cert_not_after(cert_pem: &[u8]) -> Result<DateTime<Utc>, AppError> {
    let cert = CertificateDer::pem_slice_iter(cert_pem)
        .next()
        .ok_or_else(|| {
            AppError::ConfigLoadError("TLS cert PEM contains no certificate".to_string())
        })?
        .map_err(|e| AppError::ConfigLoadError(format!("invalid TLS cert PEM: {e}")))?;
    let (_, parsed) = x509_parser::parse_x509_certificate(cert.as_ref())
        .map_err(|e| AppError::ConfigLoadError(format!("invalid TLS certificate: {e}")))?;

    DateTime::from_timestamp(parsed.validity().not_after.timestamp(), 0).ok_or_else(|| {
        AppError::ConfigLoadError("TLS certificate NotAfter out of range".to_string())
    })
}

pub(super) fn renewal_due(not_after: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    not_after - now < chrono::Duration::days(RENEW_BEFORE_DAYS)
}

/// Next daily check moment at [`CHECK_HOUR_UTC`].
pub(super) fn next_check_at(now: DateTime<Utc>) -> DateTime<Utc> {
    let today = now
        .date_naive()
        .and_hms_opt(CHECK_HOUR_UTC, 0, 0)
        .expect("valid check time")
        .and_utc();
    if today > now {
        today
    } else {
        today + chrono::Duration::days(1)
    }
}

// Collect problem details from the CA
async fn order_failure(order: &mut Order, status: OrderStatus) -> AppError {
    let mut problems = Vec::new();
    let mut authorizations = order.authorizations();
    while let Some(result) = authorizations.next().await {
        let Ok(mut authz) = result else { continue };
        let Ok(state) = authz.refresh().await else {
            continue;
        };
        for challenge in &state.challenges {
            if let Some(problem) = &challenge.error {
                problems.push(problem.to_string());
            }
        }
    }

    let detail = match problems.is_empty() {
        true => "the CA reported no problem details".to_string(),
        false => problems.join("; "),
    };
    AppError::ConfigLoadError(format!("ACME order became {status:?}: {detail}"))
}

/// Write via temp file + rename so a partial PEM is never visible.
pub(super) async fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> std::io::Result<()> {
    let mut tmp_name = path.as_os_str().to_owned();
    tmp_name.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp_name);

    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(mode);
    #[cfg(not(unix))]
    let _ = mode;

    let mut file = options.open(&tmp).await?;
    file.write_all(bytes).await?;
    file.sync_all().await?;
    drop(file);

    tokio::fs::rename(&tmp, path).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_CERT: &[u8] = include_bytes!("../fixtures/tls/cert.pem");

    /// Self-signed pair for `eks.example.nl` expiring in `days`.
    fn self_signed_pair(days: i64) -> (String, String) {
        let mut params = rcgen::CertificateParams::new(vec!["eks.example.nl".to_string()]).unwrap();
        params.not_before = time::OffsetDateTime::now_utc() - time::Duration::days(1);
        params.not_after = time::OffsetDateTime::now_utc() + time::Duration::days(days);
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = params.self_signed(&key).unwrap();
        (cert.pem(), key.serialize_pem())
    }

    /// A renewer pointing at a fresh temp dir holding a cert expiring in `days`.
    struct RenewerSetup {
        acme: AcmeConfig,
        tls: TlsConfig,
        rustls_config: RustlsConfig,
        store: AcmeStore,
        dir: std::path::PathBuf,
    }

    impl RenewerSetup {
        async fn new(name: &str, days: i64, credentials: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("eks-acme-renew-{name}-{}", std::process::id()));
            tokio::fs::create_dir_all(&dir).await.unwrap();

            let (cert_pem, key_pem) = self_signed_pair(days);
            let tls = TlsConfig {
                cert_path: dir.join("cert.pem"),
                key_path: dir.join("key.pem"),
            };
            tokio::fs::write(&tls.cert_path, &cert_pem).await.unwrap();
            tokio::fs::write(&tls.key_path, &key_pem).await.unwrap();

            let acme = AcmeConfig {
                directory_url: "https://acme.example/dir".to_string(),
                domain: "eks.example.nl".to_string(),
                account_credentials: secrecy::SecretString::from(credentials),
                root_ca_path: None,
            };
            let config = server::server_config_from_pem(cert_pem.as_bytes(), key_pem.as_bytes())
                .expect("valid pair");

            Self {
                acme,
                tls,
                rustls_config: RustlsConfig::from_config(Arc::new(config)),
                store: AcmeStore::default(),
                dir,
            }
        }

        fn renewal(&self) -> Renewal<'_> {
            Renewal {
                acme: &self.acme,
                tls: &self.tls,
                rustls_config: &self.rustls_config,
                store: &self.store,
            }
        }

        async fn cleanup(self) {
            tokio::fs::remove_dir_all(&self.dir).await.unwrap();
        }
    }

    #[tokio::test]
    async fn renew_if_due_skips_a_certificate_that_is_not_due() {
        let setup = RenewerSetup::new("not-due", 60, "{}").await;

        // Returns before touching credentials or the network.
        setup.renewal().renew_if_due().await.expect("not due");

        setup.cleanup().await;
    }

    #[tokio::test]
    async fn renew_if_due_fails_fast_on_invalid_credentials() {
        let setup = RenewerSetup::new("due", 7, "not json").await;

        // Due, so renewal starts; credential parsing fails before any traffic.
        let err = setup.renewal().renew_if_due().await.expect_err("err");
        assert!(matches!(err, AppError::ConfigLoadError(_)));

        setup.cleanup().await;
    }

    #[tokio::test]
    async fn renew_if_due_reports_a_missing_certificate() {
        let mut setup = RenewerSetup::new("missing", 60, "{}").await;
        setup.tls.cert_path = setup.dir.join("nonexistent.pem");

        let err = setup.renewal().renew_if_due().await.expect_err("err");
        assert!(matches!(err, AppError::ServerError(_)));

        setup.cleanup().await;
    }

    #[test]
    fn log_renewal_error_handles_both_branches() {
        log_renewal_error(&AppError::DatabaseError(sqlx::Error::PoolTimedOut));
        log_renewal_error(&AppError::ConfigLoadError("other".to_string()));
    }

    #[test]
    fn cert_not_after_parses_fixture_cert() {
        let not_after = cert_not_after(FIXTURE_CERT).expect("fixture cert parses");
        assert!(not_after > DateTime::from_timestamp(1_500_000_000, 0).unwrap());
    }

    #[test]
    fn cert_not_after_rejects_garbage() {
        assert!(cert_not_after(b"not a pem").is_err());
        assert!(cert_not_after(b"").is_err());
    }

    #[test]
    fn renewal_due_boundaries() {
        let now = Utc::now();
        let day = chrono::Duration::days(1);

        assert!(!renewal_due(now + day * 31, now));
        assert!(renewal_due(now + day * 29, now));
        assert!(renewal_due(now - day, now));
    }

    #[test]
    fn next_check_at_boundaries() {
        let at = |h, m| {
            DateTime::parse_from_rfc3339(&format!("2026-07-28T{h:02}:{m:02}:00Z"))
                .unwrap()
                .to_utc()
        };

        assert_eq!(next_check_at(at(1, 30)), at(2, 0));
        assert_eq!(
            next_check_at(at(2, 0)),
            at(2, 0) + chrono::Duration::days(1)
        );
        assert_eq!(
            next_check_at(at(14, 45)),
            at(2, 0) + chrono::Duration::days(1)
        );
    }

    #[tokio::test]
    async fn write_atomic_replaces_content() {
        let dir = std::env::temp_dir().join(format!("eks-acme-test-{}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("cert.pem");

        write_atomic(&path, b"first", 0o644).await.unwrap();
        write_atomic(&path, b"second", 0o644).await.unwrap();

        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"second");
        tokio::fs::remove_dir_all(&dir).await.unwrap();
    }
}
