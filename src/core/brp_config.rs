//! Configuration of the connection to the BRP: where it is, how this
//! application authenticates, and the client certificate for mTLS.

use std::{fmt, path::PathBuf, str::FromStr, time::Duration};

use secrecy::{ExposeSecret, SecretString};
use url::Url;

use crate::AppError;

/// BRP client configuration.
#[derive(Debug, Clone)]
pub struct BrpConfig {
    pub base_url: String,
    pub auth: BrpAuthConfig,
    pub persons_endpoint: String,
    pub timeout: Duration,
    /// Client certificate presented to the BRP gateway, for a production
    /// connection over mTLS.
    pub client_identity: Option<BrpClientIdentity>,
    /// Extra trust root for the gateway's own certificate, merged with the
    /// platform's, for a gateway that presents a private PKI certificate.
    pub root_ca_path: Option<PathBuf>,
}

impl BrpConfig {
    /// A plain connection to `base_url` without credentials, for tests
    /// serving their own responses.
    #[cfg(test)]
    pub fn new_test(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            auth: BrpAuthConfig::None,
            persons_endpoint: crate::constants::BRP_PERSONS_ENDPOINT.to_string(),
            timeout: Duration::from_secs(5),
            client_identity: None,
            root_ca_path: None,
        }
    }
}

/// How this application authenticates towards the BRP.
#[derive(Debug, Clone)]
pub enum BrpAuthConfig {
    /// No credentials at all, for the `personen-mock`.
    None,
    /// A fixed bearer token, sent as is with every request.
    ApiKey(SecretString),
    /// OAuth 2.0 client credentials, exchanged for a short-lived bearer token
    /// at the token endpoint. This is what the production BRP API uses.
    ClientCredentials(BrpClientCredentials),
}

/// Paths to the PEM-encoded client certificate (chain) and private key for the
/// mTLS connection to the BRP.
#[derive(Debug, Clone)]
pub struct BrpClientIdentity {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

/// OAuth 2.0 client credentials for the BRP token endpoint, configured as one
/// URL: `https://{client_id}:{client_secret}@host/path?scope=...`.
///
/// The user info carries the credentials and every query parameter becomes a
/// field of the token request (the BRP asks for `scope` and `resourceServer`).
/// Both are stripped from the URL the token is requested at.
#[derive(Clone)]
pub struct BrpClientCredentials {
    token_url: Url,
    client_id: String,
    client_secret: SecretString,
    parameters: Vec<(String, String)>,
}

impl BrpClientCredentials {
    /// The token endpoint, without credentials or query.
    pub fn token_url(&self) -> &Url {
        &self.token_url
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub fn client_secret(&self) -> &SecretString {
        &self.client_secret
    }

    /// The parameters sent along with the credentials, in configured order.
    pub fn parameters(&self) -> &[(String, String)] {
        &self.parameters
    }

    /// The `scope` the token is requested for.
    pub fn scope(&self) -> &str {
        self.parameters
            .iter()
            .find(|(name, _)| name == "scope")
            .map(|(_, value)| value.as_str())
            .unwrap_or_default()
    }
}

/// The secret must never reach a log through `Debug`.
impl fmt::Debug for BrpClientCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BrpClientCredentials")
            .field("token_url", &self.token_url.as_str())
            .field("client_id", &self.client_id)
            .field("client_secret", &"[REDACTED]")
            .field("parameters", &self.parameters)
            .finish()
    }
}

fn invalid(reason: impl fmt::Display) -> AppError {
    AppError::ConfigLoadError(format!(
        "BRP_TOKEN_URL: {reason} (expected https://{{client_id}}:{{client_secret}}@host/path?scope=...)"
    ))
}

/// Whether a host is the local machine, where plain `http` cannot leak the
/// secret onto a network.
fn is_loopback(host: Option<url::Host<&str>>) -> bool {
    match host {
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

impl FromStr for BrpClientCredentials {
    type Err = AppError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let mut url = Url::parse(raw.trim()).map_err(invalid)?;

        match url.scheme() {
            "https" => {}
            "http" if is_loopback(url.host()) => {}
            "http" => return Err(invalid("the secret may only travel over https")),
            scheme => return Err(invalid(format!("unsupported scheme {scheme:?}"))),
        }

        // The URL parser keeps the user info percent-encoded, so a secret
        // holding `@` or `:` arrives as `%40` and `%3A`.
        let client_id = urlencoding::decode(url.username())
            .map_err(|_| invalid("the client id is not valid percent-encoded UTF-8"))?
            .into_owned();
        if client_id.is_empty() {
            return Err(invalid("the client id is missing"));
        }
        // Wrapped as soon as it is decoded, so no plain copy outlives this
        // statement.
        let client_secret = url
            .password()
            .map(urlencoding::decode)
            .transpose()
            .map_err(|_| invalid("the client secret is not valid percent-encoded UTF-8"))?
            .map(|secret| SecretString::from(secret.into_owned()))
            .filter(|secret| !secret.expose_secret().is_empty())
            .ok_or_else(|| invalid("the client secret is missing"))?;

        let parameters: Vec<(String, String)> = url
            .query_pairs()
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect();
        if !parameters
            .iter()
            .any(|(name, value)| name == "scope" && !value.trim().is_empty())
        {
            return Err(invalid("the scope query parameter is missing"));
        }

        url.set_username("")
            .map_err(|()| invalid("cannot strip the client id"))?;
        url.set_password(None)
            .map_err(|()| invalid("cannot strip the client secret"))?;
        url.set_query(None);
        url.set_fragment(None);

        Ok(Self {
            token_url: url,
            client_id,
            client_secret,
            parameters,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN_URL: &str = "https://my-client:s3cret@auth.example.nl/nidp/oauth/nam/token\
                             ?scope=brp.raadplegen&resourceServer=ResourceServer01";

    #[test]
    fn credentials_are_taken_from_the_url() {
        let credentials: BrpClientCredentials = TOKEN_URL.parse().expect("valid");

        assert_eq!(credentials.client_id(), "my-client");
        assert_eq!(credentials.client_secret().expose_secret(), "s3cret");
        assert_eq!(credentials.scope(), "brp.raadplegen");
        assert_eq!(
            credentials.parameters(),
            [
                ("scope".to_string(), "brp.raadplegen".to_string()),
                ("resourceServer".to_string(), "ResourceServer01".to_string()),
            ]
        );
    }

    /// Neither the credentials nor the parameters belong in the request URL.
    #[test]
    fn the_token_url_is_stripped_of_credentials_and_query() {
        let credentials: BrpClientCredentials = TOKEN_URL.parse().expect("valid");

        assert_eq!(
            credentials.token_url().as_str(),
            "https://auth.example.nl/nidp/oauth/nam/token"
        );
    }

    #[test]
    fn credentials_are_percent_decoded() {
        let credentials: BrpClientCredentials =
            "https://id%40host:p%3Aass%40word%2F@auth.example.nl/token?scope=brp"
                .parse()
                .expect("valid");

        assert_eq!(credentials.client_id(), "id@host");
        assert_eq!(credentials.client_secret().expose_secret(), "p:ass@word/");
    }

    #[test]
    fn the_secret_does_not_appear_in_debug_output() {
        let credentials: BrpClientCredentials = TOKEN_URL.parse().expect("valid");

        let debug = format!("{credentials:?}");

        assert!(!debug.contains("s3cret"), "{debug}");
        assert!(debug.contains("my-client"), "{debug}");
    }

    #[test]
    fn plain_http_is_only_accepted_for_the_local_machine() {
        for local in [
            "http://id:secret@localhost:5011/token?scope=brp",
            "http://id:secret@127.0.0.1:5011/token?scope=brp",
            "http://id:secret@[::1]:5011/token?scope=brp",
        ] {
            assert!(local.parse::<BrpClientCredentials>().is_ok(), "{local}");
        }

        let remote = "http://id:secret@auth.example.nl/token?scope=brp";
        assert!(
            matches!(
                remote.parse::<BrpClientCredentials>(),
                Err(AppError::ConfigLoadError(_))
            ),
            "{remote}"
        );
    }

    #[test]
    fn incomplete_credentials_are_rejected() {
        for raw in [
            "not a url",
            "ftp://id:secret@auth.example.nl/token?scope=brp",
            "https://auth.example.nl/token?scope=brp",
            "https://id@auth.example.nl/token?scope=brp",
            "https://id:@auth.example.nl/token?scope=brp",
            "https://:secret@auth.example.nl/token?scope=brp",
            "https://id:secret@auth.example.nl/token",
            "https://id:secret@auth.example.nl/token?scope=",
            "https://id:secret@auth.example.nl/token?resourceServer=ResourceServer01",
        ] {
            assert!(
                matches!(
                    raw.parse::<BrpClientCredentials>(),
                    Err(AppError::ConfigLoadError(_))
                ),
                "{raw:?} must be rejected"
            );
        }
    }
}
