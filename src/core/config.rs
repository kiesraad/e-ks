//! Loads runtime configuration from environment variables for AppState.
//! Used by AppState::new to construct service URLs and storage settings.

use std::{env, path::PathBuf, time::Duration};

use secrecy::SecretString;

use super::rate_limit::RateLimits;
use crate::{
    AppError, ElectionConfig, GithubUserId,
    constants::{BRP_PERSONS_ENDPOINT, BRP_TIMEOUT},
};

#[cfg(feature = "dev-features")]
mod dev_defaults {
    #[cfg(feature = "database")]
    pub(super) const STORAGE_URL: &str = "postgres://eks@localhost/eks";

    #[cfg(not(feature = "database"))]
    pub(super) const STORAGE_URL: &str = "memory://ephemeral";

    pub(super) const ID_DERIVATION_KEY: &str = "eks-dev-id-derivation-key-not-for-production";

    pub(super) const DEFAULT_MASTER_ENCRYPTION_KEY: &str =
        "eks-dev-master-encryption-key-not-for-production";

    pub(super) const BRP_API_KEY: &str = "";
    pub(super) const BRP_BASE_URL: &str = "http://localhost:5010";
    pub(super) const DEFAULT_ELECTION: &str = "EK27";

    pub(super) fn lookup(name: &'static str) -> Result<String, std::env::VarError> {
        std::collections::HashMap::from([
            ("STORAGE_URL", STORAGE_URL),
            ("ID_DERIVATION_KEY", ID_DERIVATION_KEY),
            ("MASTER_ENCRYPTION_KEY", DEFAULT_MASTER_ENCRYPTION_KEY),
            ("BRP_BASE_URL", BRP_BASE_URL),
            ("BRP_API_KEY", BRP_API_KEY),
            ("DEFAULT_ELECTION", DEFAULT_ELECTION),
        ])
        .get(name)
        .map(|value| (*value).to_string())
        .ok_or(std::env::VarError::NotPresent)
    }
}

/// TLS configuration for serving HTTPS via rustls.
#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

/// BRP client configuration.
#[derive(Debug, Clone)]
pub struct BrpConfig {
    pub base_url: String,
    /// Held as a secret so it cannot reach a log through `Debug`.
    pub api_key: SecretString,
    pub persons_endpoint: String,
    pub timeout: Duration,
}

/// ACME (Let's Encrypt) certificate-renewal configuration.
#[derive(Debug, Clone)]
pub struct AcmeConfig {
    /// Deliberately no default, so production orders are always explicit.
    pub directory_url: String,
    /// FQDN to order the certificate for.
    pub domain: String,
    /// Account credentials JSON produced by the `create_acme_account` tool
    /// (development crate); contains the account's private key.
    pub account_credentials: SecretString,
    /// Extra trust root for the directory's own TLS (pebble testing only).
    pub root_ca_path: Option<PathBuf>,
}

/// GitHub OAuth configuration for the CSB (central electoral committee) login.
///
/// Fully separate from the political-group login (SAML, auth-service):
/// committee members authenticate against GitHub and must appear on the
/// account-id allowlist.
#[derive(Debug, Clone)]
pub struct GithubOauthConfig {
    /// Client id of the GitHub OAuth app.
    pub client_id: String,
    /// Client secret of the GitHub OAuth app.
    pub client_secret: SecretString,
    /// Numeric GitHub account ids allowed to log in as committee member.
    pub allowed_user_ids: Vec<GithubUserId>,
}

/// Runtime configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    pub storage_url: SecretString,
    pub id_derivation_key: SecretString,
    pub master_encryption_key: SecretString,
    pub tls: Option<TlsConfig>,
    /// ACME certificate renewal; requires `tls`.
    pub acme: Option<AcmeConfig>,
    /// Short identifier of the server this instance runs on (e.g. "S1"),
    /// rendered next to the version in the layout footer.
    pub server_name: Option<String>,
    /// When set, every request must carry an `x-eks-key` header whose value
    /// matches this secret. Intended for gating the app behind a known
    /// upstream (e.g. a load balancer that injects the header).
    pub eks_key: Option<SecretString>,
    /// When true, opts this instance out of the live auth-service (so
    /// `AuthServiceState::new_empty` is used instead of
    /// `AuthServiceState::new_from_env`, skipping the startup IdP-metadata
    /// fetch). Intended for environments that only ever use the `/dev/login`
    /// bypass and must boot without outbound connectivity, e.g. the Playwright
    /// container. Set via `DISABLE_AUTH_SERVICE` (`1`, `true`, or `yes`,
    /// case-insensitive); anything else leaves the auth-service enabled.
    pub disable_auth_service: bool,
    pub brp_client: BrpConfig,
    /// GitHub OAuth login for CSB users; the `/csb/login` routes answer 404
    /// when unset.
    pub github_oauth: Option<GithubOauthConfig>,
    /// Election a login lands on when the flow has no election selection of
    /// its own (CSB logins, dev logins). Set via `DEFAULT_ELECTION` as the
    /// election code, with the region appended after a colon where the type
    /// needs one (e.g. `EK27`, `PS27:prov1`).
    pub default_election: ElectionConfig,
    /// Per-stream rate limits guarding against denial of service through the
    /// regular interface; see [`RateLimits`].
    pub rate_limits: RateLimits,
}

fn get_env_with<F>(name: &'static str, lookup: &mut F) -> Result<String, AppError>
where
    F: FnMut(&'static str) -> Result<String, env::VarError>,
{
    match lookup(name) {
        Ok(value) => Ok(value),
        #[cfg(feature = "dev-features")]
        Err(_) => Ok(dev_defaults::lookup(name).map_err(|_| AppError::MissingEnvVar(name))?),
        #[cfg(not(feature = "dev-features"))]
        Err(_) => Err(AppError::MissingEnvVar(name)),
    }
}

/// Secret key material from the environment, refusing a blank value.
///
/// `env::var` returns `Ok("")` for a variable set to nothing, which HKDF would
/// otherwise accept as a key. Refuse to boot instead.
fn get_secret_with<F>(name: &'static str, lookup: &mut F) -> Result<SecretString, AppError>
where
    F: FnMut(&'static str) -> Result<String, env::VarError>,
{
    let value = get_env_with(name, lookup)?;

    if value.trim().is_empty() {
        return Err(AppError::ConfigLoadError(format!(
            "{name} is set but empty; it must hold secret key material"
        )));
    }

    Ok(SecretString::from(value))
}

/// Offline sanity check; the credentials are bound to their directory, so a
/// staging account can never be deployed against production (or vice versa).
fn check_account_credentials(credentials: &str, directory_url: &str) -> Result<(), AppError> {
    let value: serde_json::Value = serde_json::from_str(credentials).map_err(|e| {
        AppError::ConfigLoadError(format!("ACME_ACCOUNT_CREDENTIALS is not valid JSON: {e}"))
    })?;
    if value.get("directory").and_then(|v| v.as_str()) != Some(directory_url) {
        return Err(AppError::ConfigLoadError(
            "ACME_ACCOUNT_CREDENTIALS was not created for ACME_DIRECTORY_URL; \
             run `create_acme_account` against this directory"
                .to_string(),
        ));
    }
    Ok(())
}

/// TLS config from `TLS_CERT_PATH` and `TLS_KEY_PATH`; both or neither must be
/// set.
fn tls_from_env<F>(lookup: &mut F) -> Result<Option<TlsConfig>, AppError>
where
    F: FnMut(&'static str) -> Result<String, env::VarError>,
{
    match (lookup("TLS_CERT_PATH").ok(), lookup("TLS_KEY_PATH").ok()) {
        (Some(cert), Some(key)) => Ok(Some(TlsConfig {
            cert_path: PathBuf::from(cert),
            key_path: PathBuf::from(key),
        })),
        (None, None) => Ok(None),
        _ => Err(AppError::ConfigLoadError(
            "TLS_CERT_PATH and TLS_KEY_PATH must both be set, or both unset".to_string(),
        )),
    }
}

/// ACME config from `ACME_DIRECTORY_URL` and `ACME_DOMAIN`; both or neither
/// must be set, and renewal requires TLS plus directory-bound credentials.
fn acme_from_env<F>(lookup: &mut F, has_tls: bool) -> Result<Option<AcmeConfig>, AppError>
where
    F: FnMut(&'static str) -> Result<String, env::VarError>,
{
    let directory_url = lookup("ACME_DIRECTORY_URL").ok().filter(|s| !s.is_empty());
    let domain = lookup("ACME_DOMAIN").ok().filter(|s| !s.is_empty());

    match (directory_url, domain) {
        (Some(directory_url), Some(domain)) => {
            if !has_tls {
                return Err(AppError::ConfigLoadError(
                    "ACME renewal requires TLS_CERT_PATH and TLS_KEY_PATH".to_string(),
                ));
            }
            let account_credentials = lookup("ACME_ACCOUNT_CREDENTIALS")
                .ok()
                .filter(|s| !s.is_empty())
                .ok_or(AppError::MissingEnvVar("ACME_ACCOUNT_CREDENTIALS"))?;
            check_account_credentials(&account_credentials, &directory_url)?;
            Ok(Some(AcmeConfig {
                directory_url,
                domain,
                account_credentials: SecretString::from(account_credentials),
                root_ca_path: lookup("ACME_ROOT_CA_PATH")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from),
            }))
        }
        (None, None) => Ok(None),
        _ => Err(AppError::ConfigLoadError(
            "ACME_DIRECTORY_URL and ACME_DOMAIN must both be set, or both unset".to_string(),
        )),
    }
}

/// Parses `DEFAULT_ELECTION`: the election code, with the region appended
/// after a colon where the election type needs one (e.g. `EK27`, `PS27:prov1`).
fn parse_default_election(raw: &str) -> Result<ElectionConfig, AppError> {
    let (code, region) = match raw.split_once(':') {
        Some((code, region)) => (code, Some(region)),
        None => (raw, None),
    };
    ElectionConfig::from_code_and_region(code.trim(), region.map(str::trim)).ok_or_else(|| {
        AppError::ConfigLoadError(format!(
            "DEFAULT_ELECTION {raw:?} is not a known election (expected e.g. EK27 or PS27:prov1)"
        ))
    })
}

/// Parses the comma-separated `GITHUB_ALLOWED_USER_IDS` allowlist. Strict: a
/// single malformed entry rejects the whole configuration rather than silently
/// shrinking the allowlist.
fn parse_github_allowlist(raw: &str) -> Result<Vec<GithubUserId>, AppError> {
    let ids = raw
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::parse)
        .collect::<Result<Vec<GithubUserId>, _>>()
        .map_err(|err| AppError::ConfigLoadError(format!("GITHUB_ALLOWED_USER_IDS: {err}")))?;
    if ids.is_empty() {
        return Err(AppError::ConfigLoadError(
            "GITHUB_ALLOWED_USER_IDS must contain at least one GitHub user id".to_string(),
        ));
    }
    Ok(ids)
}

/// GitHub OAuth config from `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET`, and
/// `GITHUB_ALLOWED_USER_IDS` (comma-separated numeric account ids); all three
/// or none must be set.
fn github_oauth_from_env<F>(lookup: &mut F) -> Result<Option<GithubOauthConfig>, AppError>
where
    F: FnMut(&'static str) -> Result<String, env::VarError>,
{
    let mut non_empty = |name| lookup(name).ok().filter(|s: &String| !s.is_empty());
    let client_id = non_empty("GITHUB_CLIENT_ID");
    let client_secret = non_empty("GITHUB_CLIENT_SECRET");
    let allowed_user_ids = non_empty("GITHUB_ALLOWED_USER_IDS");

    match (client_id, client_secret, allowed_user_ids) {
        (Some(client_id), Some(client_secret), Some(allowed_user_ids)) => {
            Ok(Some(GithubOauthConfig {
                client_id,
                client_secret: SecretString::from(client_secret),
                allowed_user_ids: parse_github_allowlist(&allowed_user_ids)?,
            }))
        }
        (None, None, None) => Ok(None),
        _ => Err(AppError::ConfigLoadError(
            "GITHUB_CLIENT_ID, GITHUB_CLIENT_SECRET and GITHUB_ALLOWED_USER_IDS \
             must all be set, or all unset"
                .to_string(),
        )),
    }
}

impl Config {
    pub fn from_env() -> Result<Self, AppError> {
        Self::from_env_with(env::var)
    }

    fn from_env_with<F>(mut lookup: F) -> Result<Self, AppError>
    where
        F: FnMut(&'static str) -> Result<String, env::VarError>,
    {
        let storage_url = get_env_with("STORAGE_URL", &mut lookup)?;
        let id_derivation_key = get_secret_with("ID_DERIVATION_KEY", &mut lookup)?;
        let master_encryption_key = get_secret_with("MASTER_ENCRYPTION_KEY", &mut lookup)?;

        let tls = tls_from_env(&mut lookup)?;
        let acme = acme_from_env(&mut lookup, tls.is_some())?;
        let github_oauth = github_oauth_from_env(&mut lookup)?;
        let default_election =
            parse_default_election(&get_env_with("DEFAULT_ELECTION", &mut lookup)?)?;

        let server_name = lookup("SERVER_NAME").ok().filter(|s| !s.is_empty());

        let eks_key = lookup("EKS_KEY")
            .ok()
            .filter(|s| !s.is_empty())
            .map(SecretString::from);

        let disable_auth_service = lookup("DISABLE_AUTH_SERVICE").is_ok_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            )
        });

        let base_url = get_env_with("BRP_BASE_URL", &mut lookup)?;
        let api_key = SecretString::from(get_env_with("BRP_API_KEY", &mut lookup)?);

        let timeout: u64 = lookup("BRP_TIMEOUT")
            .unwrap_or(BRP_TIMEOUT.to_string())
            .parse()
            .map_err(|_| {
                AppError::ConfigLoadError("Invalid BRP_TIMEOUT; please enter a number".to_string())
            })?;

        let brp_client = BrpConfig {
            base_url,
            api_key,
            persons_endpoint: lookup("BRP_PERSONS_ENDPOINT")
                .unwrap_or(BRP_PERSONS_ENDPOINT.to_string()),
            timeout: Duration::from_secs(timeout),
        };

        let rate_limits = RateLimits::from_env_with(&mut lookup)?;

        Ok(Self {
            storage_url: SecretString::from(storage_url),
            id_derivation_key,
            master_encryption_key,
            tls,
            acme,
            server_name,
            eks_key,
            disable_auth_service,
            brp_client,
            github_oauth,
            default_election,
            rate_limits,
        })
    }

    #[cfg(test)]
    pub fn new_test() -> Self {
        use crate::constants;

        Self {
            storage_url: SecretString::from("memory://"),
            id_derivation_key: SecretString::from("test-secret-123"),
            master_encryption_key: SecretString::from("test-encryption-secret-123"),
            tls: None,
            acme: None,
            server_name: None,
            eks_key: None,
            disable_auth_service: false,
            brp_client: BrpConfig {
                base_url: "http://localhost:5010".to_string(),
                api_key: SecretString::from(""),
                persons_endpoint: constants::BRP_PERSONS_ENDPOINT.to_string(),
                timeout: Duration::from_secs(5),
            },
            github_oauth: None,
            default_election: ElectionConfig::EK27,
            rate_limits: RateLimits::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;
    use std::collections::HashMap;

    fn lookup_from(
        map: &HashMap<&'static str, &'static str>,
    ) -> impl FnMut(&'static str) -> Result<String, env::VarError> {
        move |key| {
            map.get(key)
                .map(|value| (*value).to_string())
                .ok_or(env::VarError::NotPresent)
        }
    }

    #[test]
    fn get_env_returns_value_when_set() {
        let map = HashMap::from([("TEST_CONFIG_ENV", "present")]);
        let mut lookup = lookup_from(&map);

        let value = get_env_with("TEST_CONFIG_ENV", &mut lookup).expect("env value");

        assert_eq!(value, "present");
    }

    #[test]
    fn from_env_uses_env_values() {
        let map = HashMap::from([
            ("STORAGE_URL", "memory://test"),
            ("ID_DERIVATION_KEY", "test-secret-123"),
        ]);
        let lookup = lookup_from(&map);

        let config = Config::from_env_with(lookup).expect("config");

        assert_eq!(config.storage_url.expose_secret(), "memory://test");
    }

    /// A secret that is set but blank must stop startup, not be accepted as key
    /// material. Whitespace counts as blank.
    #[test]
    fn from_env_rejects_blank_secrets() {
        for (name, blank) in [
            ("ID_DERIVATION_KEY", ""),
            ("ID_DERIVATION_KEY", "   "),
            ("MASTER_ENCRYPTION_KEY", ""),
            ("MASTER_ENCRYPTION_KEY", "\t\n"),
        ] {
            let map = HashMap::from([
                ("STORAGE_URL", "memory://test"),
                ("ID_DERIVATION_KEY", "id-derivation-key-123"),
                ("MASTER_ENCRYPTION_KEY", "master-encryption-key-123"),
                (name, blank),
            ]);
            let lookup = lookup_from(&map);

            let err = Config::from_env_with(lookup).expect_err("blank secret must be rejected");

            assert!(
                matches!(err, AppError::ConfigLoadError(ref message) if message.contains(name)),
                "{name}={blank:?} gave {err:?}"
            );
        }
    }

    /// A secret that holds a value still loads.
    #[test]
    fn from_env_accepts_non_blank_secrets() {
        let map = HashMap::from([
            ("STORAGE_URL", "memory://test"),
            ("ID_DERIVATION_KEY", "id-derivation-key-123"),
            ("MASTER_ENCRYPTION_KEY", "master-encryption-key-123"),
        ]);
        let lookup = lookup_from(&map);

        let config = Config::from_env_with(lookup).expect("config");

        assert_eq!(
            config.master_encryption_key.expose_secret(),
            "master-encryption-key-123"
        );
    }

    #[cfg(feature = "dev-features")]
    #[test]
    fn get_env_returns_default_when_missing_in_dev_features() {
        let map = HashMap::new();
        let mut lookup = lookup_from(&map);

        let value = get_env_with("ID_DERIVATION_KEY", &mut lookup).expect("dev default");

        assert_eq!(value, dev_defaults::ID_DERIVATION_KEY);
    }

    #[cfg(not(feature = "dev-features"))]
    #[test]
    fn get_env_errors_when_missing_without_dev_features() {
        let map = HashMap::new();
        let mut lookup = lookup_from(&map);

        let err = get_env_with("TEST_CONFIG_MISSING", &mut lookup).expect_err("missing env");

        assert_eq!(
            err.to_string(),
            AppError::MissingEnvVar("TEST_CONFIG_MISSING").to_string()
        );
    }

    #[cfg(feature = "dev-features")]
    #[test]
    fn from_env_uses_defaults_in_dev_features() {
        let map = HashMap::new();
        let lookup = lookup_from(&map);

        let config = Config::from_env_with(lookup).expect("dev defaults");

        assert_eq!(
            config.storage_url.expose_secret(),
            dev_defaults::STORAGE_URL
        );
    }

    #[cfg(not(feature = "dev-features"))]
    #[test]
    fn from_env_errors_when_storage_missing_without_dev_features() {
        let map = HashMap::new();
        let lookup = lookup_from(&map);

        let err = Config::from_env_with(lookup).expect_err("missing storage");

        assert_eq!(
            err.to_string(),
            AppError::MissingEnvVar("STORAGE_URL").to_string()
        );
    }

    #[test]
    fn from_env_returns_no_tls_when_unset() {
        let map = HashMap::new();
        let lookup = lookup_from(&map);

        let config = Config::from_env_with(lookup).expect("config");

        assert!(config.tls.is_none());
    }

    #[test]
    fn from_env_returns_tls_when_both_set() {
        let map = HashMap::from([
            ("TLS_CERT_PATH", "/etc/tls/cert.pem"),
            ("TLS_KEY_PATH", "/etc/tls/key.pem"),
        ]);
        let lookup = lookup_from(&map);

        let config = Config::from_env_with(lookup).expect("config");
        let tls = config.tls.expect("tls present");

        assert_eq!(tls.cert_path, std::path::PathBuf::from("/etc/tls/cert.pem"));
        assert_eq!(tls.key_path, std::path::PathBuf::from("/etc/tls/key.pem"));
    }

    #[test]
    fn from_env_errors_when_only_tls_cert_set() {
        let map = HashMap::from([("TLS_CERT_PATH", "/etc/tls/cert.pem")]);
        let lookup = lookup_from(&map);

        let err = Config::from_env_with(lookup).expect_err("err");
        assert!(matches!(err, AppError::ConfigLoadError(_)));
    }

    #[test]
    fn from_env_errors_when_only_tls_key_set() {
        let map = HashMap::from([("TLS_KEY_PATH", "/etc/tls/key.pem")]);
        let lookup = lookup_from(&map);

        let err = Config::from_env_with(lookup).expect_err("err");
        assert!(matches!(err, AppError::ConfigLoadError(_)));
    }

    #[test]
    fn from_env_returns_no_acme_when_unset() {
        let map = HashMap::new();
        let lookup = lookup_from(&map);

        let config = Config::from_env_with(lookup).expect("config");

        assert!(config.acme.is_none());
    }

    const TEST_ACME_CREDENTIALS: &str = concat!(
        r#"{"id":"https://acme-staging-v02.api.letsencrypt.org/acme/acct/123","#,
        r#""key_pkcs8":"bm90LWEtcmVhbC1rZXk","#,
        r#""directory":"https://acme-staging-v02.api.letsencrypt.org/directory"}"#
    );

    #[test]
    fn from_env_returns_acme_when_set_with_tls() {
        let map = HashMap::from([
            ("TLS_CERT_PATH", "/etc/tls/cert.pem"),
            ("TLS_KEY_PATH", "/etc/tls/key.pem"),
            (
                "ACME_DIRECTORY_URL",
                "https://acme-staging-v02.api.letsencrypt.org/directory",
            ),
            ("ACME_DOMAIN", "example.nl"),
            ("ACME_ACCOUNT_CREDENTIALS", TEST_ACME_CREDENTIALS),
        ]);
        let lookup = lookup_from(&map);

        let config = Config::from_env_with(lookup).expect("config");
        let acme = config.acme.expect("acme present");

        assert_eq!(
            acme.directory_url,
            "https://acme-staging-v02.api.letsencrypt.org/directory"
        );
        assert_eq!(acme.domain, "example.nl");
        assert_eq!(
            acme.account_credentials.expose_secret(),
            TEST_ACME_CREDENTIALS
        );
        assert!(acme.root_ca_path.is_none());
    }

    #[test]
    fn from_env_errors_when_acme_credentials_missing() {
        let map = HashMap::from([
            ("TLS_CERT_PATH", "/etc/tls/cert.pem"),
            ("TLS_KEY_PATH", "/etc/tls/key.pem"),
            ("ACME_DIRECTORY_URL", "https://localhost:14000/dir"),
            ("ACME_DOMAIN", "example.nl"),
        ]);
        let lookup = lookup_from(&map);

        let err = Config::from_env_with(lookup).expect_err("err");
        assert_eq!(
            err.to_string(),
            AppError::MissingEnvVar("ACME_ACCOUNT_CREDENTIALS").to_string()
        );
    }

    #[test]
    fn from_env_errors_when_acme_credentials_are_not_json() {
        let map = HashMap::from([
            ("TLS_CERT_PATH", "/etc/tls/cert.pem"),
            ("TLS_KEY_PATH", "/etc/tls/key.pem"),
            ("ACME_DIRECTORY_URL", "https://localhost:14000/dir"),
            ("ACME_DOMAIN", "example.nl"),
            ("ACME_ACCOUNT_CREDENTIALS", "not json"),
        ]);
        let lookup = lookup_from(&map);

        let err = Config::from_env_with(lookup).expect_err("err");
        assert!(matches!(err, AppError::ConfigLoadError(_)));
    }

    #[test]
    fn from_env_errors_when_acme_credentials_are_for_another_directory() {
        let map = HashMap::from([
            ("TLS_CERT_PATH", "/etc/tls/cert.pem"),
            ("TLS_KEY_PATH", "/etc/tls/key.pem"),
            ("ACME_DIRECTORY_URL", "https://localhost:14000/dir"),
            ("ACME_DOMAIN", "example.nl"),
            ("ACME_ACCOUNT_CREDENTIALS", TEST_ACME_CREDENTIALS),
        ]);
        let lookup = lookup_from(&map);

        let err = Config::from_env_with(lookup).expect_err("err");
        assert!(matches!(err, AppError::ConfigLoadError(_)));
    }

    #[test]
    fn from_env_errors_when_only_acme_directory_set() {
        let map = HashMap::from([
            ("TLS_CERT_PATH", "/etc/tls/cert.pem"),
            ("TLS_KEY_PATH", "/etc/tls/key.pem"),
            ("ACME_DIRECTORY_URL", "https://localhost:14000/dir"),
        ]);
        let lookup = lookup_from(&map);

        let err = Config::from_env_with(lookup).expect_err("err");
        assert!(matches!(err, AppError::ConfigLoadError(_)));
    }

    #[test]
    fn from_env_errors_when_acme_set_without_tls() {
        let map = HashMap::from([
            ("ACME_DIRECTORY_URL", "https://localhost:14000/dir"),
            ("ACME_DOMAIN", "example.nl"),
        ]);
        let lookup = lookup_from(&map);

        let err = Config::from_env_with(lookup).expect_err("err");
        assert!(matches!(err, AppError::ConfigLoadError(_)));
    }

    #[test]
    fn parse_default_election_accepts_code_and_optional_region() {
        assert_eq!(
            parse_default_election("EK27").expect("EK27"),
            ElectionConfig::EK27
        );
        assert_eq!(
            parse_default_election("PS27:prov1").expect("PS27:prov1"),
            ElectionConfig::PS27(crate::Province::Groningen)
        );
    }

    #[test]
    fn parse_default_election_rejects_unknown_values() {
        for raw in ["", "EK99", "PS27", "PS27:prov37"] {
            assert!(
                matches!(
                    parse_default_election(raw),
                    Err(AppError::ConfigLoadError(_))
                ),
                "{raw:?} must be rejected"
            );
        }
    }

    #[test]
    fn from_env_returns_no_github_oauth_when_unset() {
        let map = HashMap::new();
        let lookup = lookup_from(&map);

        let config = Config::from_env_with(lookup).expect("config");

        assert!(config.github_oauth.is_none());
    }

    #[test]
    fn from_env_returns_github_oauth_when_all_set() {
        let map = HashMap::from([
            ("GITHUB_CLIENT_ID", "Iv1.abc123"),
            ("GITHUB_CLIENT_SECRET", "s3cret"),
            ("GITHUB_ALLOWED_USER_IDS", "583231, 42,7"),
        ]);
        let lookup = lookup_from(&map);

        let config = Config::from_env_with(lookup).expect("config");
        let github = config.github_oauth.expect("github oauth present");

        assert_eq!(github.client_id, "Iv1.abc123");
        assert_eq!(github.client_secret.expose_secret(), "s3cret");
        assert_eq!(
            github.allowed_user_ids,
            ["583231", "42", "7"].map(|id| id.parse().expect("valid id"))
        );
    }

    #[test]
    fn from_env_errors_when_github_oauth_partially_set() {
        let map = HashMap::from([("GITHUB_CLIENT_ID", "Iv1.abc123")]);
        let lookup = lookup_from(&map);

        let err = Config::from_env_with(lookup).expect_err("err");
        assert!(matches!(err, AppError::ConfigLoadError(_)));
    }

    #[test]
    fn from_env_errors_when_github_allowlist_is_malformed() {
        for allowlist in ["", "octocat", "42,0", " , "] {
            let map = HashMap::from([
                ("GITHUB_CLIENT_ID", "Iv1.abc123"),
                ("GITHUB_CLIENT_SECRET", "s3cret"),
                ("GITHUB_ALLOWED_USER_IDS", allowlist),
            ]);
            let lookup = lookup_from(&map);

            let err = Config::from_env_with(lookup).expect_err("err");
            assert!(
                matches!(err, AppError::ConfigLoadError(_)),
                "allowlist {allowlist:?} must be rejected"
            );
        }
    }
}
