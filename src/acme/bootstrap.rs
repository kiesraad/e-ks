//! Self-signed placeholder certificate for a first boot without provisioned
//! cert/key files; the renewer replaces it with a real one right away.

use crate::{
    AcmeConfig, AppError, TlsConfig,
    acme::renewer::{cert_not_after, renewal_due, write_atomic},
};

/// Within the renewal window, so the renewer replaces the placeholder on its
/// first pass.
const PLACEHOLDER_VALIDITY: time::Duration = time::Duration::days(7);

/// Write a self-signed placeholder for `acme.domain` when neither TLS file
/// exists. Exactly one existing file is ambiguous: fail, overwrite nothing.
pub async fn bootstrap_certificate(acme: &AcmeConfig, tls: &TlsConfig) -> Result<(), AppError> {
    let cert_exists = tokio::fs::try_exists(&tls.cert_path)
        .await
        .map_err(AppError::IoError)?;
    let key_exists = tokio::fs::try_exists(&tls.key_path)
        .await
        .map_err(AppError::IoError)?;

    match (cert_exists, key_exists) {
        (true, true) => return Ok(()),
        (false, false) => {}
        _ => {
            return Err(AppError::ConfigLoadError(format!(
                "one of {} / {} exists without the other; provision both or remove both",
                tls.cert_path.display(),
                tls.key_path.display(),
            )));
        }
    }

    let (cert_pem, key_pem) = self_signed_placeholder(&acme.domain)?;
    write_atomic(&tls.key_path, key_pem.as_bytes(), 0o600)
        .await
        .map_err(AppError::IoError)?;
    write_atomic(&tls.cert_path, cert_pem.as_bytes(), 0o644)
        .await
        .map_err(AppError::IoError)?;

    tracing::info!(
        "no TLS certificate found at {}; wrote a self-signed placeholder for {} — \
         the ACME renewer will order a real certificate right away",
        tls.cert_path.display(),
        acme.domain
    );

    Ok(())
}

fn self_signed_placeholder(domain: &str) -> Result<(String, String), AppError> {
    let mut params =
        rcgen::CertificateParams::new(vec![domain.to_string()]).map_err(placeholder_error)?;
    params.not_before = time::OffsetDateTime::now_utc();
    params.not_after = params.not_before + PLACEHOLDER_VALIDITY;

    let key = rcgen::KeyPair::generate().map_err(placeholder_error)?;
    let cert = params.self_signed(&key).map_err(placeholder_error)?;

    let cert_pem = cert.pem();
    debug_assert!(
        renewal_due(cert_not_after(cert_pem.as_bytes())?, chrono::Utc::now()),
        "placeholder must trigger an immediate renewal"
    );

    Ok((cert_pem, key.serialize_pem()))
}

fn placeholder_error(err: rcgen::Error) -> AppError {
    AppError::ConfigLoadError(format!(
        "could not generate a self-signed placeholder certificate: {err}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::server;

    fn test_config(dir: &std::path::Path) -> (AcmeConfig, TlsConfig) {
        (
            AcmeConfig {
                directory_url: "https://acme.example/dir".to_string(),
                domain: "eks.example.nl".to_string(),
                account_credentials: secrecy::SecretString::from("{}"),
                root_ca_path: None,
            },
            TlsConfig {
                cert_path: dir.join("cert.pem"),
                key_path: dir.join("key.pem"),
            },
        )
    }

    async fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("eks-acme-boot-{name}-{}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        dir
    }

    #[tokio::test]
    async fn writes_a_placeholder_the_server_accepts_and_the_renewer_replaces() {
        let dir = temp_dir("fresh").await;
        let (acme, tls) = test_config(&dir);

        bootstrap_certificate(&acme, &tls).await.unwrap();

        let cert = tokio::fs::read(&tls.cert_path).await.unwrap();
        let key = tokio::fs::read(&tls.key_path).await.unwrap();
        server::server_config_from_pem(&cert, &key).expect("valid cert/key pair");
        assert!(renewal_due(
            cert_not_after(&cert).unwrap(),
            chrono::Utc::now()
        ));

        tokio::fs::remove_dir_all(&dir).await.unwrap();
    }

    #[tokio::test]
    async fn leaves_existing_files_untouched() {
        let dir = temp_dir("existing").await;
        let (acme, tls) = test_config(&dir);
        tokio::fs::write(&tls.cert_path, b"provisioned cert")
            .await
            .unwrap();
        tokio::fs::write(&tls.key_path, b"provisioned key")
            .await
            .unwrap();

        bootstrap_certificate(&acme, &tls).await.unwrap();

        assert_eq!(
            tokio::fs::read(&tls.cert_path).await.unwrap(),
            b"provisioned cert"
        );
        assert_eq!(
            tokio::fs::read(&tls.key_path).await.unwrap(),
            b"provisioned key"
        );

        tokio::fs::remove_dir_all(&dir).await.unwrap();
    }

    #[tokio::test]
    async fn rejects_half_provisioned_state() {
        let dir = temp_dir("half").await;
        let (acme, tls) = test_config(&dir);
        tokio::fs::write(&tls.cert_path, b"provisioned cert")
            .await
            .unwrap();

        let err = bootstrap_certificate(&acme, &tls).await.expect_err("err");
        assert!(matches!(err, AppError::ConfigLoadError(_)));
        assert_eq!(
            tokio::fs::read(&tls.cert_path).await.unwrap(),
            b"provisioned cert"
        );
        assert!(!tokio::fs::try_exists(&tls.key_path).await.unwrap());

        tokio::fs::remove_dir_all(&dir).await.unwrap();
    }
}
