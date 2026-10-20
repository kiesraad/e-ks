//! Runtime configuration, read from the environment once at startup.
//!
//! The `config` crate collects the variables into [`Env`], one field per
//! variable, and [`Config`] validates them. The environment is the only
//! source; `dev-features` builds fill in a default for the variables a
//! development machine has no value for.

use std::{
    fmt::Display,
    net::{Ipv4Addr, SocketAddr},
    num::NonZeroU64,
    path::PathBuf,
    str::FromStr,
    time::Duration,
};

use auth_service::config::{AuthSettings, Environment as TvsEnvironment, PreselectedAd};
use config::Environment;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer, de::Error as _};

use super::{CsbAlertHours, CsbIpAllowList, ProxyProtocolPeers, RateLimits, logging::LogFormat};
use crate::{
    AppError, CsbWebauthnConfig, ElectionConfig,
    constants::{BRP_REQUEST_INTERVAL_MS, BRP_TIMEOUT, STORE_CACHE_IDLE_MINUTES},
};

/// Development defaults for the variables that have none otherwise.
#[cfg(feature = "dev-features")]
const DEV_DEFAULTS: [(&str, &str); 5] = [
    (
        "storage_url",
        if cfg!(feature = "database") {
            "postgres://eks@localhost/eks"
        } else {
            "memory://ephemeral"
        },
    ),
    (
        "id_derivation_key",
        "eks-dev-id-derivation-key-not-for-production",
    ),
    (
        "master_encryption_key",
        "eks-dev-master-encryption-key-not-for-production",
    ),
    (
        "brp_url",
        "http://localhost:5010/haalcentraal/api/brp/personen",
    ),
    ("default_election", "EK27"),
];

/// Defaults for the shared online TVS mock.
#[cfg(feature = "tvs-mock")]
const TVS_MOCK_DEFAULTS: [(&str, &str); 2] =
    [("tvs_env", "test"), ("base_url", "http://localhost:3000")];

/// TLS configuration for serving HTTPS via rustls.
#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

/// BRP client configuration.
#[derive(Debug, Clone)]
pub struct BrpConfig {
    /// The complete URL of the "personen" resource at the tussenpartij.
    pub url: String,
    /// Bearer token, when the tussenpartij issues one. Held as a secret so it
    /// cannot reach a log through `Debug`.
    pub api_key: Option<SecretString>,
    pub timeout: Duration,
    /// Minimum time between the starts of two requests, across all callers.
    pub request_interval: Duration,
    /// Client certificate for mTLS, when the BRP requires one.
    pub client_cert: Option<TlsConfig>,
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
    /// Address the main listener binds to; the binary's first argument
    /// overrides it. Set via `BIND_ADDRESS` (default `0.0.0.0:3000`).
    pub bind_address: String,
    /// Second listener for the CSB section, so it can be published on a domain
    /// of its own; `/csb` is then unreachable on the main listener, and the
    /// `eks_key` gate does not apply to it. Set via `CSB_BIND_ADDRESS`.
    pub csb_bind_address: Option<SocketAddr>,
    /// Domain the CSB listener is published on; with ACME it gets a
    /// certificate of its own. Set via `CSB_DOMAIN`.
    pub csb_domain: Option<String>,
    /// Peer addresses allowed to reach the CSB listener; any other peer is
    /// refused with `403` and reported. Requires `csb_bind_address`, since
    /// only that listener is gated. Set via `CSB_IP_ALLOW_LIST`.
    pub csb_ip_allow_list: Option<CsbIpAllowList>,
    /// Proxies that announce the client with a PROXY protocol v2 header.
    /// Requires `csb_bind_address` and `tls`. Set via
    /// `CSB_PROXY_PROTOCOL_FROM`.
    pub csb_proxy_protocol_from: Option<ProxyProtocolPeers>,
    /// Local hours in which committee activity is reported (not blocked). Set
    /// via `CSB_ALERT_HOURS`, e.g. `22:00-06:00`.
    pub csb_alert_hours: Option<CsbAlertHours>,
    /// Live auth-service (TVS login) settings, from `TVS_ENV`, `CERTS_DIR`,
    /// `BASE_URL` and `PRESELECTED_AD`. `DISABLE_AUTH_SERVICE` sets it to
    /// `None`, skipping the startup IdP-metadata fetch, for environments that
    /// only use the `/dev/login` bypass (e.g. the Playwright container).
    pub auth: Option<AuthSettings>,
    pub brp_client: BrpConfig,
    /// Security-key (WebAuthn) login for CSB users; the `/csb/login` routes
    /// answer 404 when unset. Set via `CSB_WEBAUTHN_ORIGIN` and
    /// `CSB_WEBAUTHN_USERS`. Requires `csb_bind_address` outside
    /// development builds.
    pub csb_webauthn: Option<CsbWebauthnConfig>,
    /// Election a login lands on when the flow has no election selection of
    /// its own (CSB logins, dev logins). Set via `DEFAULT_ELECTION` as the
    /// election code, with the election domain appended after a colon where
    /// the type needs one (e.g. `EK27`, `PS27:prov1`).
    pub default_election: ElectionConfig,
    /// Per-stream rate limits guarding against denial of service through the
    /// regular interface; see [`RateLimits`].
    pub rate_limits: RateLimits,
    /// Evict cached political-group stores not used for this long. Set via
    /// `STORE_CACHE_IDLE_MINUTES` (default 24 hours).
    pub store_cache_idle_timeout: Duration,
    /// How log events are written. Set via `LOG_FORMAT`.
    pub log_format: LogFormat,
}

/// The variables as set, one field per variable named after it in lowercase
/// (the `config` crate lowercases the keys). A variable set to nothing counts
/// as unset. Each field holds its own parsed value; the checks that span
/// variables are [`Config`]'s.
#[derive(Deserialize)]
struct Env {
    storage_url: SecretString,
    #[serde(deserialize_with = "secret")]
    id_derivation_key: SecretString,
    #[serde(deserialize_with = "secret")]
    master_encryption_key: SecretString,
    tls_cert_path: Option<PathBuf>,
    tls_key_path: Option<PathBuf>,
    acme_directory_url: Option<String>,
    acme_domain: Option<String>,
    acme_account_credentials: Option<SecretString>,
    acme_root_ca_path: Option<PathBuf>,
    server_name: Option<String>,
    eks_key: Option<SecretString>,
    bind_address: Option<String>,
    #[serde(default, deserialize_with = "bind_address")]
    csb_bind_address: Option<SocketAddr>,
    csb_domain: Option<String>,
    #[serde(default, deserialize_with = "parsed")]
    csb_ip_allow_list: Option<CsbIpAllowList>,
    #[serde(default, deserialize_with = "parsed")]
    csb_proxy_protocol_from: Option<ProxyProtocolPeers>,
    #[serde(default, deserialize_with = "parsed")]
    csb_alert_hours: Option<CsbAlertHours>,
    csb_webauthn_origin: Option<String>,
    csb_webauthn_users: Option<String>,
    #[serde(default)]
    disable_auth_service: bool,
    #[serde(default, deserialize_with = "parsed")]
    tvs_env: Option<TvsEnvironment>,
    certs_dir: Option<PathBuf>,
    #[serde(default, deserialize_with = "parsed")]
    preselected_ad: Option<PreselectedAd>,
    base_url: Option<String>,
    brp_url: String,
    brp_api_key: Option<SecretString>,
    brp_timeout: Option<u64>,
    brp_request_interval_ms: Option<u64>,
    brp_client_cert_path: Option<PathBuf>,
    brp_client_key_path: Option<PathBuf>,
    #[serde(deserialize_with = "election")]
    default_election: ElectionConfig,
    rate_limit_downloads: Option<usize>,
    rate_limit_downloads_window_secs: Option<u64>,
    rate_limit_events: Option<usize>,
    rate_limit_events_window_secs: Option<u64>,
    rate_limit_events_total: Option<usize>,
    store_cache_idle_minutes: Option<NonZeroU64>,
    #[serde(default)]
    log_format: LogFormat,
}

/// Secret key material. HKDF would accept whitespace as a key, so that
/// stops startup instead; a value set to nothing counts as unset.
fn secret<'de, D: Deserializer<'de>>(deserializer: D) -> Result<SecretString, D::Error> {
    let secret = SecretString::deserialize(deserializer)?;
    if secret.expose_secret().trim().is_empty() {
        return Err(D::Error::custom("blank secret key material"));
    }
    Ok(secret)
}

/// An optional setting parsed with `FromStr`, so the type keeps its own error
/// message.
fn parsed<'de, D: Deserializer<'de>, T: FromStr<Err: Display>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    String::deserialize(deserializer)?
        .parse()
        .map(Some)
        .map_err(D::Error::custom)
}

/// `CSB_BIND_ADDRESS`: an `address:port` with a numeric address, or a bare
/// port number, which binds on `0.0.0.0`.
fn bind_address<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<SocketAddr>, D::Error> {
    let raw = String::deserialize(deserializer)?;
    let raw = raw.trim();
    if let Ok(port) = raw.parse::<u16>() {
        return Ok(Some(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port))));
    }
    raw.parse().map(Some).map_err(|_| {
        D::Error::custom(format!(
            "{raw:?} is neither a port number nor an address:port with a numeric address \
             (e.g. 3001, 0.0.0.0:3001, 127.0.0.1:3001)"
        ))
    })
}

/// `DEFAULT_ELECTION`: the election code, with the election domain appended
/// after a colon where the election type needs one (e.g. `EK27`, `PS27:prov1`).
fn election<'de, D: Deserializer<'de>>(deserializer: D) -> Result<ElectionConfig, D::Error> {
    let raw = String::deserialize(deserializer)?;
    let (code, domain) = raw
        .split_once(':')
        .map_or((raw.as_str(), None), |(code, domain)| (code, Some(domain)));
    ElectionConfig::from_code_and_domain(code.trim(), domain.map(str::trim)).ok_or_else(|| {
        D::Error::custom(format!(
            "{raw:?} is not a known election (expected e.g. EK27 or PS27:prov1)"
        ))
    })
}

/// A configuration error with `message`.
fn invalid(message: &str) -> AppError {
    AppError::ConfigLoadError(message.to_string())
}

/// A certificate and key path pair, from the variables `cert_var` and
/// `key_var`: both or neither.
fn cert_pair(
    cert_path: &Option<PathBuf>,
    key_path: &Option<PathBuf>,
    cert_var: &str,
    key_var: &str,
) -> Result<Option<TlsConfig>, AppError> {
    match (cert_path, key_path) {
        (Some(cert_path), Some(key_path)) => Ok(Some(TlsConfig {
            cert_path: cert_path.clone(),
            key_path: key_path.clone(),
        })),
        (None, None) => Ok(None),
        _ => Err(invalid(&format!(
            "{cert_var} and {key_var} must both be set, or both unset"
        ))),
    }
}

/// `ACME_DIRECTORY_URL` and `ACME_DOMAIN`: both or neither, and renewal
/// requires TLS plus credentials bound to the directory.
fn acme(env: &Env, has_tls: bool) -> Result<Option<AcmeConfig>, AppError> {
    let (directory_url, domain) = match (&env.acme_directory_url, &env.acme_domain) {
        (Some(directory_url), Some(domain)) => (directory_url.clone(), domain.clone()),
        (None, None) => return Ok(None),
        _ => {
            return Err(invalid(
                "ACME_DIRECTORY_URL and ACME_DOMAIN must both be set, or both unset",
            ));
        }
    };
    if !has_tls {
        return Err(invalid(
            "ACME renewal requires TLS_CERT_PATH and TLS_KEY_PATH",
        ));
    }
    let account_credentials = env
        .acme_account_credentials
        .clone()
        .ok_or_else(|| invalid("ACME_ACCOUNT_CREDENTIALS is required for ACME renewal"))?;
    check_account_credentials(account_credentials.expose_secret(), &directory_url)?;
    Ok(Some(AcmeConfig {
        directory_url,
        domain,
        account_credentials,
        root_ca_path: env.acme_root_ca_path.clone(),
    }))
}

/// Offline sanity check; the credentials are bound to their directory, so a
/// staging account can never be deployed against production (or vice versa).
fn check_account_credentials(credentials: &str, directory_url: &str) -> Result<(), AppError> {
    let value: serde_json::Value = serde_json::from_str(credentials).map_err(|e| {
        AppError::ConfigLoadError(format!("ACME_ACCOUNT_CREDENTIALS is not valid JSON: {e}"))
    })?;
    if value.get("directory").and_then(|v| v.as_str()) != Some(directory_url) {
        return Err(invalid(
            "ACME_ACCOUNT_CREDENTIALS was not created for ACME_DIRECTORY_URL; \
             run `create_acme_account` against this directory",
        ));
    }
    Ok(())
}

/// The CSB guards gate the dedicated listener only, so without one they are
/// refused rather than silently gating nothing; the PROXY protocol header is
/// read by the TLS server only. Outside development the listener is refused
/// without the allow list as well: it has no CDN or `x-eks-key` gate in front.
fn check_csb_guards(env: &Env, has_tls: bool) -> Result<(), AppError> {
    let has_listener = env.csb_bind_address.is_some();
    if env.csb_ip_allow_list.is_some() && !has_listener {
        return Err(invalid(
            "CSB_IP_ALLOW_LIST requires CSB_BIND_ADDRESS: the allow list gates the dedicated \
             CSB listener only",
        ));
    }
    if env.csb_ip_allow_list.is_none() && has_listener && !cfg!(feature = "dev-features") {
        return Err(invalid(
            "CSB_BIND_ADDRESS requires CSB_IP_ALLOW_LIST: the CSB listener has no CDN or \
             x-eks-key gate in front",
        ));
    }
    if env.csb_proxy_protocol_from.is_some() && !has_listener {
        return Err(invalid(
            "CSB_PROXY_PROTOCOL_FROM requires CSB_BIND_ADDRESS: the PROXY protocol header is \
             read on the dedicated CSB listener only",
        ));
    }
    if env.csb_proxy_protocol_from.is_some() && !has_tls {
        return Err(invalid(
            "CSB_PROXY_PROTOCOL_FROM requires TLS_CERT_PATH and TLS_KEY_PATH: only the TLS \
             server reads the PROXY protocol header",
        ));
    }
    Ok(())
}

/// `CSB_WEBAUTHN_ORIGIN` (the origin the CSB section is served on) enables the
/// security-key login; `CSB_WEBAUTHN_USERS` lists the keys (see
/// [`CsbWebauthnConfig::parse`]). Users without an origin are an error, and
/// outside development so is an origin without `CSB_BIND_ADDRESS`: the login
/// page lists every registered key, so it stays off the main listener, on the
/// one `CSB_IP_ALLOW_LIST` can gate.
fn csb_webauthn(env: &Env) -> Result<Option<CsbWebauthnConfig>, AppError> {
    match (&env.csb_webauthn_origin, &env.csb_webauthn_users) {
        (Some(_), _) if !cfg!(feature = "dev-features") && env.csb_bind_address.is_none() => {
            Err(invalid(
                "CSB_WEBAUTHN_ORIGIN requires CSB_BIND_ADDRESS: the CSB login must not be \
                 served on the main listener",
            ))
        }
        (Some(origin), users) => Ok(Some(CsbWebauthnConfig::parse(origin, users.as_deref())?)),
        (None, None) => Ok(None),
        (None, Some(_)) => Err(invalid("CSB_WEBAUTHN_USERS requires CSB_WEBAUTHN_ORIGIN")),
    }
}

/// The auth-service settings; `tvs-mock` builds carry an embedded bundle, so
/// only they may omit `CERTS_DIR`.
fn auth(env: &Env) -> Result<Option<AuthSettings>, AppError> {
    if env.disable_auth_service {
        return Ok(None);
    }
    let required = |what: &str| {
        invalid(&format!(
            "{what} is required unless DISABLE_AUTH_SERVICE is set"
        ))
    };
    #[cfg(not(feature = "tvs-mock"))]
    if env.certs_dir.is_none() {
        return Err(required("CERTS_DIR"));
    }
    Ok(Some(AuthSettings {
        environment: env.tvs_env.ok_or_else(|| required("TVS_ENV"))?,
        certs_dir: env.certs_dir.clone(),
        preselected_ad: env.preselected_ad.unwrap_or_default(),
        base_url: env.base_url.clone().ok_or_else(|| required("BASE_URL"))?,
    }))
}

/// The BRP client settings; only `BRP_URL` has no default.
fn brp(env: &Env) -> Result<BrpConfig, AppError> {
    Ok(BrpConfig {
        url: env.brp_url.clone(),
        api_key: env.brp_api_key.clone(),
        timeout: Duration::from_secs(env.brp_timeout.unwrap_or(BRP_TIMEOUT)),
        request_interval: Duration::from_millis(
            env.brp_request_interval_ms
                .unwrap_or(BRP_REQUEST_INTERVAL_MS),
        ),
        client_cert: cert_pair(
            &env.brp_client_cert_path,
            &env.brp_client_key_path,
            "BRP_CLIENT_CERT_PATH",
            "BRP_CLIENT_KEY_PATH",
        )?,
    })
}

impl TryFrom<Env> for Config {
    type Error = AppError;

    fn try_from(env: Env) -> Result<Self, AppError> {
        let tls = cert_pair(
            &env.tls_cert_path,
            &env.tls_key_path,
            "TLS_CERT_PATH",
            "TLS_KEY_PATH",
        )?;
        let acme = acme(&env, tls.is_some())?;
        let brp_client = brp(&env)?;
        check_csb_guards(&env, tls.is_some())?;
        let csb_webauthn = csb_webauthn(&env)?;
        let auth = auth(&env)?;
        let limits = RateLimits::default();
        let idle_minutes = env
            .store_cache_idle_minutes
            .unwrap_or(STORE_CACHE_IDLE_MINUTES);

        Ok(Self {
            storage_url: env.storage_url,
            id_derivation_key: env.id_derivation_key,
            master_encryption_key: env.master_encryption_key,
            tls,
            acme,
            server_name: env.server_name,
            eks_key: env.eks_key,
            bind_address: env
                .bind_address
                .unwrap_or_else(|| "0.0.0.0:3000".to_string()),
            csb_bind_address: env.csb_bind_address,
            csb_domain: env.csb_domain,
            csb_ip_allow_list: env.csb_ip_allow_list,
            csb_proxy_protocol_from: env.csb_proxy_protocol_from,
            csb_alert_hours: env.csb_alert_hours,
            auth,
            brp_client,
            csb_webauthn,
            default_election: env.default_election,
            rate_limits: RateLimits {
                downloads: limits.downloads.with(
                    env.rate_limit_downloads,
                    env.rate_limit_downloads_window_secs,
                ),
                events: limits
                    .events
                    .with(env.rate_limit_events, env.rate_limit_events_window_secs),
                events_total: env.rate_limit_events_total.unwrap_or(limits.events_total),
            },
            // Saturate rather than panic on an absurdly large number of minutes.
            store_cache_idle_timeout: Duration::from_secs(idle_minutes.get().saturating_mul(60)),
            log_format: env.log_format,
        })
    }
}

impl Config {
    /// Reads the configuration from the environment.
    pub fn from_env() -> Result<Self, AppError> {
        Self::from_environment(Environment::default())
    }

    /// Reads the configuration from `environment`, with the development and
    /// TVS mock defaults underneath it where the build has them.
    fn from_environment(environment: Environment) -> Result<Self, AppError> {
        let builder = config::Config::builder();
        #[cfg(feature = "dev-features")]
        let builder = DEV_DEFAULTS
            .into_iter()
            .try_fold(builder, |builder, (key, value)| {
                builder.set_default(key, value)
            })?;
        #[cfg(feature = "tvs-mock")]
        let builder = TVS_MOCK_DEFAULTS
            .into_iter()
            .try_fold(builder, |builder, (key, value)| {
                builder.set_default(key, value)
            })?;
        builder
            .add_source(environment.ignore_empty(true))
            .build()?
            .try_deserialize::<Env>()?
            .try_into()
    }

    /// ACME and TLS config for the `CSB_DOMAIN` certificate, whose files sit
    /// next to the main certificate's with a `csb-` name prefix.
    #[cfg(feature = "acme")]
    pub fn csb_acme(&self) -> Option<(AcmeConfig, TlsConfig)> {
        let acme = AcmeConfig {
            domain: self.csb_domain.clone()?,
            ..self.acme.clone()?
        };
        let tls = self.tls.as_ref()?;
        let csb_path = |path: &PathBuf| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            path.with_file_name(format!("csb-{name}"))
        };
        let tls = TlsConfig {
            cert_path: csb_path(&tls.cert_path),
            key_path: csb_path(&tls.key_path),
        };
        Some((acme, tls))
    }

    #[cfg(test)]
    pub fn new_test() -> Self {
        Self {
            storage_url: SecretString::from("memory://"),
            id_derivation_key: SecretString::from("test-secret-123"),
            master_encryption_key: SecretString::from("test-encryption-secret-123"),
            tls: None,
            acme: None,
            server_name: None,
            eks_key: None,
            bind_address: "0.0.0.0:3000".to_string(),
            csb_bind_address: None,
            csb_domain: None,
            csb_ip_allow_list: None,
            csb_proxy_protocol_from: None,
            csb_alert_hours: None,
            auth: None,
            brp_client: BrpConfig {
                url: "http://localhost:5010/haalcentraal/api/brp/personen".to_string(),
                api_key: None,
                timeout: Duration::from_secs(5),
                request_interval: Duration::ZERO,
                client_cert: None,
            },
            csb_webauthn: None,
            default_election: ElectionConfig::EK27,
            rate_limits: RateLimits::default(),
            store_cache_idle_timeout: Duration::from_secs(STORE_CACHE_IDLE_MINUTES.get() * 60),
            log_format: LogFormat::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;

    /// The variables with no default outside `dev-features` and `tvs-mock`,
    /// so a test about any other setting passes with the features off.
    const REQUIRED: [(&str, &str); 8] = [
        ("STORAGE_URL", "memory://test"),
        ("ID_DERIVATION_KEY", "id-derivation-key-123"),
        ("MASTER_ENCRYPTION_KEY", "master-encryption-key-123"),
        (
            "BRP_URL",
            "http://localhost:5010/haalcentraal/api/brp/personen",
        ),
        ("DEFAULT_ELECTION", "EK27"),
        ("TVS_ENV", "test"),
        ("CERTS_DIR", "/tmp/certs"),
        ("BASE_URL", "http://localhost:3000"),
    ];

    /// The configuration read from `vars` alone.
    fn from_vars(vars: &[(&str, &str)]) -> Result<Config, AppError> {
        let vars = vars
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect();
        Config::from_environment(Environment::default().source(Some(vars)))
    }

    /// The configuration read from [`REQUIRED`] with `vars` laid over it.
    fn config_from(vars: &[(&str, &str)]) -> Result<Config, AppError> {
        from_vars(&[&REQUIRED, vars].concat())
    }

    /// Whether `result` failed with a configuration error naming `what`.
    fn rejected_naming<T: std::fmt::Debug>(result: Result<T, AppError>, what: &str) -> bool {
        matches!(result, Err(AppError::ConfigLoadError(ref message)) if message.contains(what))
    }

    #[test]
    fn from_env_uses_env_values() {
        let config = config_from(&[("STORAGE_URL", "memory://from-env")]).expect("config");

        assert_eq!(config.storage_url.expose_secret(), "memory://from-env");
        assert_eq!(
            config.master_encryption_key.expose_secret(),
            "master-encryption-key-123"
        );
    }

    /// A secret set to whitespace must stop startup, not be accepted as key
    /// material.
    #[test]
    fn from_env_rejects_blank_secrets() {
        for (name, blank) in [
            ("ID_DERIVATION_KEY", "   "),
            ("MASTER_ENCRYPTION_KEY", "\t\n"),
        ] {
            assert!(
                rejected_naming(config_from(&[(name, blank)]), &name.to_lowercase()),
                "{name}={blank:?} must be rejected"
            );
        }
    }

    #[cfg(feature = "dev-features")]
    #[test]
    fn from_env_uses_defaults_in_dev_features() {
        let config = from_vars(&[]).expect("dev defaults");

        let (_, storage_url) = DEV_DEFAULTS
            .iter()
            .find(|(key, _)| *key == "storage_url")
            .expect("storage default");
        assert_eq!(config.storage_url.expose_secret(), *storage_url);
    }

    #[cfg(not(feature = "dev-features"))]
    #[test]
    fn from_env_errors_when_storage_missing_without_dev_features() {
        assert!(rejected_naming(from_vars(&[]), "storage_url"));
    }

    #[test]
    fn from_env_returns_no_tls_when_unset() {
        let config = config_from(&[]).expect("config");

        assert!(config.tls.is_none());
        assert!(config.acme.is_none());
    }

    #[test]
    fn from_env_returns_tls_when_both_set() {
        let config = config_from(&[
            ("TLS_CERT_PATH", "/etc/tls/cert.pem"),
            ("TLS_KEY_PATH", "/etc/tls/key.pem"),
        ])
        .expect("config");
        let tls = config.tls.expect("tls present");

        assert_eq!(tls.cert_path, PathBuf::from("/etc/tls/cert.pem"));
        assert_eq!(tls.key_path, PathBuf::from("/etc/tls/key.pem"));
    }

    #[test]
    fn from_env_errors_when_only_one_tls_path_set() {
        for var in ["TLS_CERT_PATH", "TLS_KEY_PATH"] {
            assert!(rejected_naming(
                config_from(&[(var, "/etc/tls/file.pem")]),
                "TLS_CERT_PATH"
            ));
        }
    }

    #[test]
    fn from_env_returns_brp_client_cert_when_both_set() {
        let config = config_from(&[
            ("BRP_CLIENT_CERT_PATH", "/etc/brp/cert.pem"),
            ("BRP_CLIENT_KEY_PATH", "/etc/brp/key.pem"),
        ])
        .expect("config");
        let cert = config.brp_client.client_cert.expect("client cert present");

        assert_eq!(cert.cert_path, PathBuf::from("/etc/brp/cert.pem"));
        assert_eq!(cert.key_path, PathBuf::from("/etc/brp/key.pem"));
    }

    #[test]
    fn from_env_errors_when_only_one_brp_client_path_set() {
        for var in ["BRP_CLIENT_CERT_PATH", "BRP_CLIENT_KEY_PATH"] {
            assert!(rejected_naming(
                config_from(&[(var, "/etc/brp/file.pem")]),
                "BRP_CLIENT_CERT_PATH"
            ));
        }
    }

    #[test]
    fn from_env_reads_the_brp_api_key_when_set() {
        let config = config_from(&[("BRP_API_KEY", "brp-api-key-123")]).expect("config");

        let api_key = config.brp_client.api_key.expect("api key present");
        assert_eq!(api_key.expose_secret(), "brp-api-key-123");
    }

    #[test]
    fn from_env_has_no_brp_api_key_by_default() {
        let config = config_from(&[]).expect("config");

        assert!(config.brp_client.api_key.is_none());
    }

    #[test]
    fn from_env_reads_the_brp_request_interval() {
        let config = config_from(&[("BRP_REQUEST_INTERVAL_MS", "250")]).expect("config");

        assert_eq!(
            config.brp_client.request_interval,
            Duration::from_millis(250)
        );
    }

    const TLS: [(&str, &str); 2] = [
        ("TLS_CERT_PATH", "/etc/tls/cert.pem"),
        ("TLS_KEY_PATH", "/etc/tls/key.pem"),
    ];

    const TEST_ACME_CREDENTIALS: &str = concat!(
        r#"{"id":"https://acme-staging-v02.api.letsencrypt.org/acme/acct/123","#,
        r#""key_pkcs8":"bm90LWEtcmVhbC1rZXk","#,
        r#""directory":"https://acme-staging-v02.api.letsencrypt.org/directory"}"#
    );

    const STAGING_ACME: [(&str, &str); 3] = [
        (
            "ACME_DIRECTORY_URL",
            "https://acme-staging-v02.api.letsencrypt.org/directory",
        ),
        ("ACME_DOMAIN", "example.nl"),
        ("ACME_ACCOUNT_CREDENTIALS", TEST_ACME_CREDENTIALS),
    ];

    #[test]
    fn from_env_returns_acme_when_set_with_tls() {
        let config = config_from(&[&TLS[..], &STAGING_ACME].concat()).expect("config");
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

    #[cfg(feature = "acme")]
    #[test]
    fn csb_acme_orders_csb_domain_into_prefixed_files() {
        let vars = [&TLS[..], &STAGING_ACME, &[("CSB_DOMAIN", "csb.example.nl")]].concat();

        let config = config_from(&vars).expect("config");
        let (acme, tls) = config.csb_acme().expect("csb acme");

        assert_eq!(acme.domain, "csb.example.nl");
        assert_eq!(tls.cert_path, PathBuf::from("/etc/tls/csb-cert.pem"));
        assert_eq!(tls.key_path, PathBuf::from("/etc/tls/csb-key.pem"));
    }

    #[test]
    fn from_env_errors_when_acme_credentials_missing() {
        let vars = [
            &TLS,
            &[
                ("ACME_DIRECTORY_URL", "https://localhost:14000/dir"),
                ("ACME_DOMAIN", "example.nl"),
            ][..],
        ]
        .concat();

        assert!(rejected_naming(
            config_from(&vars),
            "ACME_ACCOUNT_CREDENTIALS"
        ));
    }

    /// Malformed credentials, and credentials for another directory, are
    /// refused.
    #[test]
    fn from_env_errors_when_acme_credentials_do_not_fit() {
        for credentials in ["not json", TEST_ACME_CREDENTIALS] {
            let vars = [
                &TLS,
                &[
                    ("ACME_DIRECTORY_URL", "https://localhost:14000/dir"),
                    ("ACME_DOMAIN", "example.nl"),
                    ("ACME_ACCOUNT_CREDENTIALS", credentials),
                ][..],
            ]
            .concat();

            assert!(rejected_naming(
                config_from(&vars),
                "ACME_ACCOUNT_CREDENTIALS"
            ));
        }
    }

    #[test]
    fn from_env_errors_when_only_acme_directory_set() {
        let vars = [
            &TLS,
            &[("ACME_DIRECTORY_URL", "https://localhost:14000/dir")][..],
        ]
        .concat();

        assert!(rejected_naming(config_from(&vars), "ACME_DOMAIN"));
    }

    #[test]
    fn from_env_errors_when_acme_set_without_tls() {
        assert!(rejected_naming(config_from(&STAGING_ACME), "TLS_CERT_PATH"));
    }

    #[test]
    fn from_env_reads_default_election_code_and_optional_domain() {
        let config = config_from(&[("DEFAULT_ELECTION", "EK27")]).expect("EK27");
        assert_eq!(config.default_election, ElectionConfig::EK27);

        let config = config_from(&[("DEFAULT_ELECTION", "PS27:prov1")]).expect("PS27:prov1");
        assert_eq!(
            config.default_election,
            ElectionConfig::PS27(crate::Province::Groningen)
        );
    }

    #[test]
    fn from_env_rejects_unknown_default_elections() {
        for raw in ["EK99", "PS27", "PS27:prov37"] {
            assert!(
                rejected_naming(
                    config_from(&[("DEFAULT_ELECTION", raw)]),
                    "default_election"
                ),
                "{raw:?} must be rejected"
            );
        }
    }

    #[test]
    fn from_env_reads_csb_bind_address_as_port_or_address() {
        for (raw, expected) in [
            ("3001", "0.0.0.0:3001"),
            ("0.0.0.0:3001", "0.0.0.0:3001"),
            (" 127.0.0.1:3001 ", "127.0.0.1:3001"),
            ("[::]:3001", "[::]:3001"),
        ] {
            let config = config_from(&[("CSB_BIND_ADDRESS", raw)]).expect("config");

            assert_eq!(
                config.csb_bind_address.expect("address").to_string(),
                expected,
                "{raw:?}"
            );
        }
    }

    /// Unset or set to nothing leaves the CSB section on the main listener
    /// and both CSB guards off.
    #[test]
    fn from_env_returns_no_csb_listener_or_guards_when_unset_or_empty() {
        for vars in [
            &[][..],
            &[
                ("CSB_BIND_ADDRESS", ""),
                ("CSB_IP_ALLOW_LIST", ""),
                ("CSB_ALERT_HOURS", ""),
            ],
        ] {
            let config = config_from(vars).expect("config");

            assert!(config.csb_bind_address.is_none());
            assert!(config.csb_ip_allow_list.is_none());
            assert!(config.csb_alert_hours.is_none());
        }
    }

    #[test]
    fn from_env_rejects_a_malformed_csb_bind_address() {
        for raw in ["localhost:3001", "0.0.0.0", "99999", "3001:0.0.0.0"] {
            assert!(
                rejected_naming(
                    config_from(&[("CSB_BIND_ADDRESS", raw)]),
                    "csb_bind_address"
                ),
                "{raw:?} must be rejected"
            );
        }
    }

    #[test]
    fn from_env_reads_csb_guards() {
        let config = config_from(&[
            ("CSB_BIND_ADDRESS", "8081"),
            ("CSB_IP_ALLOW_LIST", "203.0.113.7, 10.0.0.0/8"),
            ("CSB_ALERT_HOURS", "22:00-06:00"),
        ])
        .expect("config");

        let allow_list = config.csb_ip_allow_list.expect("allow list");
        assert!(allow_list.allows("10.1.2.3".parse().unwrap()));
        assert!(!allow_list.allows("203.0.113.8".parse().unwrap()));
        let hours = config.csb_alert_hours.expect("hours");
        assert!(hours.contains(chrono::NaiveTime::from_hms_opt(2, 0, 0).unwrap()));
        assert!(!hours.contains(chrono::NaiveTime::from_hms_opt(12, 0, 0).unwrap()));
    }

    /// An allow list without a dedicated listener would gate nothing, so it
    /// is refused rather than silently ignored.
    #[test]
    fn from_env_rejects_an_ip_allow_list_without_csb_bind_address() {
        assert!(rejected_naming(
            config_from(&[("CSB_IP_ALLOW_LIST", "203.0.113.7")]),
            "CSB_BIND_ADDRESS"
        ));
    }

    /// The PROXY protocol peers need the CSB listener and TLS.
    #[test]
    fn from_env_reads_proxy_protocol_peers_with_csb_listener_and_tls() {
        let peers = ("CSB_PROXY_PROTOCOL_FROM", "172.16.12.9");
        let listener = ("CSB_BIND_ADDRESS", "8443");

        let config = config_from(&[&TLS, &[listener, peers][..]].concat()).expect("config");
        let from = config.csb_proxy_protocol_from.expect("peers");
        assert!(from.contains("172.16.12.9".parse().unwrap()));
        assert!(!from.contains("172.16.12.5".parse().unwrap()));

        assert!(rejected_naming(
            config_from(&[&TLS, &[peers][..]].concat()),
            "CSB_BIND_ADDRESS"
        ));
        assert!(rejected_naming(
            config_from(&[listener, peers]),
            "TLS_CERT_PATH"
        ));
    }

    #[test]
    fn from_env_rejects_malformed_csb_guards() {
        for (name, raw) in [
            ("CSB_IP_ALLOW_LIST", "office"),
            ("CSB_PROXY_PROTOCOL_FROM", "lb"),
            ("CSB_ALERT_HOURS", "night"),
        ] {
            assert!(
                rejected_naming(
                    config_from(&[("CSB_BIND_ADDRESS", "8081"), (name, raw)]),
                    &name.to_lowercase()
                ),
                "{name}={raw:?} must be rejected"
            );
        }
    }

    #[test]
    fn from_env_defaults_store_cache_idle_to_24_hours() {
        let config = config_from(&[]).expect("config");

        assert_eq!(
            config.store_cache_idle_timeout,
            Duration::from_secs(24 * 60 * 60)
        );
    }

    #[test]
    fn from_env_reads_store_cache_idle_minutes() {
        let config = config_from(&[("STORE_CACHE_IDLE_MINUTES", "90")]).expect("config");

        assert_eq!(
            config.store_cache_idle_timeout,
            Duration::from_secs(90 * 60)
        );
    }

    #[test]
    fn from_env_rejects_malformed_store_cache_idle_minutes() {
        for raw in ["soon", "-5", "0"] {
            assert!(
                rejected_naming(
                    config_from(&[("STORE_CACHE_IDLE_MINUTES", raw)]),
                    "store_cache_idle_minutes"
                ),
                "{raw:?} must be rejected"
            );
        }
    }

    #[test]
    fn from_env_defaults_rate_limits() {
        let config = config_from(&[]).expect("config");
        let defaults = RateLimits::default();

        assert_eq!(config.rate_limits.downloads.max, defaults.downloads.max);
        assert_eq!(config.rate_limits.events.max, defaults.events.max);
        assert_eq!(config.rate_limits.events.window, defaults.events.window);
        assert_eq!(config.rate_limits.events_total, defaults.events_total);
    }

    #[test]
    fn from_env_reads_rate_limits() {
        let config = config_from(&[
            ("RATE_LIMIT_DOWNLOADS", "3"),
            ("RATE_LIMIT_DOWNLOADS_WINDOW_SECS", "60"),
            ("RATE_LIMIT_EVENTS", "7"),
            ("RATE_LIMIT_EVENTS_WINDOW_SECS", "120"),
            ("RATE_LIMIT_EVENTS_TOTAL", "9"),
        ])
        .expect("config");

        assert_eq!(config.rate_limits.downloads.max, 3);
        assert_eq!(config.rate_limits.downloads.window, TimeDelta::seconds(60));
        assert_eq!(config.rate_limits.events.max, 7);
        assert_eq!(config.rate_limits.events.window, TimeDelta::seconds(120));
        assert_eq!(config.rate_limits.events_total, 9);
    }

    /// A value set to nothing keeps the default, a garbage value stops startup.
    #[test]
    fn from_env_rejects_a_non_numeric_rate_limit() {
        let config = config_from(&[("RATE_LIMIT_EVENTS", "")]).expect("config");
        assert_eq!(
            config.rate_limits.events.max,
            RateLimits::default().events.max
        );

        assert!(rejected_naming(
            config_from(&[("RATE_LIMIT_EVENTS", "many")]),
            "rate_limit_events"
        ));
    }

    #[test]
    fn from_env_reads_flags_and_listener_defaults() {
        let config = config_from(&[]).expect("config");
        assert_eq!(config.bind_address, "0.0.0.0:3000");
        assert_eq!(config.log_format, LogFormat::Pretty);
        assert!(config.auth.is_some());

        let config = config_from(&[
            ("BIND_ADDRESS", "127.0.0.1:8080"),
            ("LOG_FORMAT", "json"),
            ("DISABLE_AUTH_SERVICE", "yes"),
        ])
        .expect("config");
        assert_eq!(config.bind_address, "127.0.0.1:8080");
        assert_eq!(config.log_format, LogFormat::Json);
        assert!(config.auth.is_none());

        assert!(rejected_naming(
            config_from(&[("LOG_FORMAT", "xml")]),
            "log_format"
        ));
    }

    #[test]
    fn from_env_reads_auth_settings() {
        let config = config_from(&[
            ("TVS_ENV", "preproduction"),
            ("CERTS_DIR", "/opt/eks/certs"),
            ("PRESELECTED_AD", "DigiD"),
            ("BASE_URL", "https://preview.kandidaatstellen.nl"),
        ])
        .expect("config");
        let auth = config.auth.expect("auth settings");
        assert_eq!(auth.environment, TvsEnvironment::Preproduction);
        assert_eq!(auth.certs_dir, Some(PathBuf::from("/opt/eks/certs")));
        assert_eq!(auth.preselected_ad, PreselectedAd::DigiD);
        assert_eq!(auth.base_url, "https://preview.kandidaatstellen.nl");
    }

    #[test]
    fn from_env_preselected_ad_defaults_to_select_when_unset_or_empty() {
        let config = config_from(&[]).expect("config");
        assert_eq!(
            config.auth.expect("auth settings").preselected_ad,
            PreselectedAd::Select
        );

        let config = config_from(&[("PRESELECTED_AD", "")]).expect("config");
        assert_eq!(
            config.auth.expect("auth settings").preselected_ad,
            PreselectedAd::Select
        );

        assert!(rejected_naming(
            config_from(&[("PRESELECTED_AD", "nonsense")]),
            "preselected_ad"
        ));
    }

    #[test]
    fn from_env_rejects_an_unknown_tvs_env() {
        assert!(rejected_naming(
            config_from(&[("TVS_ENV", "staging")]),
            "tvs_env"
        ));
    }

    #[test]
    fn from_env_needs_no_auth_settings_when_auth_service_is_disabled() {
        let vars: Vec<_> = REQUIRED
            .into_iter()
            .filter(|(name, _)| !["TVS_ENV", "CERTS_DIR", "BASE_URL"].contains(name))
            .chain([("DISABLE_AUTH_SERVICE", "true")])
            .collect();
        let config = from_vars(&vars).expect("config");
        assert!(config.auth.is_none());
    }

    #[cfg(not(feature = "tvs-mock"))]
    #[test]
    fn from_env_requires_auth_settings_unless_disabled() {
        for missing in ["TVS_ENV", "CERTS_DIR", "BASE_URL"] {
            let vars: Vec<_> = REQUIRED
                .into_iter()
                .filter(|(name, _)| *name != missing)
                .collect();
            assert!(rejected_naming(from_vars(&vars), missing), "{missing}");
        }
    }

    #[cfg(feature = "tvs-mock")]
    #[test]
    fn from_env_defaults_auth_settings_to_the_tvs_mock() {
        let vars: Vec<_> = REQUIRED
            .into_iter()
            .filter(|(name, _)| !["TVS_ENV", "CERTS_DIR", "BASE_URL"].contains(name))
            .collect();
        let auth = from_vars(&vars)
            .expect("config")
            .auth
            .expect("auth settings");
        assert_eq!(auth.environment, TvsEnvironment::Test);
        assert_eq!(auth.certs_dir, None);
        assert_eq!(auth.base_url, "http://localhost:3000");
    }

    #[test]
    fn from_env_returns_no_csb_webauthn_when_unset() {
        let config = config_from(&[]).expect("config");

        assert!(config.csb_webauthn.is_none());
    }

    #[test]
    fn from_env_returns_csb_webauthn_when_set() {
        use super::super::csb_webauthn::test_support::{TEST_ORIGIN, test_credential_encoded};

        let users = format!("alice:{}", test_credential_encoded());
        let config = config_from(&[
            ("CSB_BIND_ADDRESS", "8081"),
            ("CSB_WEBAUTHN_ORIGIN", TEST_ORIGIN),
            ("CSB_WEBAUTHN_USERS", &users),
        ])
        .expect("config");
        let webauthn = config.csb_webauthn.expect("csb webauthn present");

        assert_eq!(webauthn.users.len(), 1);
        assert_eq!(webauthn.users[0].username.as_str(), "alice");
    }

    /// The origin alone enables the login page (nobody can log in); a user
    /// list without it is an error.
    #[test]
    fn from_env_csb_webauthn_origin_is_the_switch() {
        let config = config_from(&[
            ("CSB_BIND_ADDRESS", "8081"),
            ("CSB_WEBAUTHN_ORIGIN", "http://localhost:3000"),
        ])
        .expect("config");
        assert!(config.csb_webauthn.expect("present").users.is_empty());

        assert!(rejected_naming(
            config_from(&[("CSB_WEBAUTHN_USERS", "alice:abc")]),
            "CSB_WEBAUTHN_ORIGIN"
        ));
    }

    #[test]
    fn from_env_errors_when_csb_webauthn_users_are_malformed() {
        for users in [
            "alice",
            "alice:abc",
            "alice:abc:abc",
            "alice:abc:abc:not-a-key",
            "al ice:abc:abc:abc",
        ] {
            let result = config_from(&[
                ("CSB_BIND_ADDRESS", "8081"),
                ("CSB_WEBAUTHN_ORIGIN", "https://csb.example.nl"),
                ("CSB_WEBAUTHN_USERS", users),
            ]);

            assert!(
                rejected_naming(result, "CSB_WEBAUTHN_USERS"),
                "users {users:?} must be rejected"
            );
        }
    }

    /// Development serves the CSB login on the single listener.
    #[cfg(feature = "dev-features")]
    #[test]
    fn from_env_csb_webauthn_without_csb_listener_in_dev_features() {
        let config =
            config_from(&[("CSB_WEBAUTHN_ORIGIN", "http://localhost:3000")]).expect("config");

        assert!(config.csb_webauthn.is_some());
    }

    #[cfg(not(feature = "dev-features"))]
    #[test]
    fn from_env_csb_webauthn_requires_csb_listener_without_dev_features() {
        assert!(rejected_naming(
            config_from(&[("CSB_WEBAUTHN_ORIGIN", "https://csb.example.nl")]),
            "CSB_BIND_ADDRESS"
        ));
    }
}
