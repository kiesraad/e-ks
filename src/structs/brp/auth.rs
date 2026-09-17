//! Authentication towards the BRP: none, a fixed bearer token, or OAuth 2.0
//! client credentials exchanged for a short-lived token that is cached and
//! shared between requests.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::{AppError, BrpAuthConfig, BrpClientCredentials};

/// A token is renewed this long before it expires, so a request never sets
/// off with one that expires in flight.
const EXPIRY_MARGIN: Duration = Duration::from_secs(30);

/// The BRP states a token is valid for ten minutes, whatever `expires_in`
/// says, so a longer lifetime is never trusted.
const MAX_TOKEN_LIFETIME: Duration = Duration::from_secs(10 * 60);

/// The lifetime assumed for a token that came without `expires_in`.
const DEFAULT_TOKEN_LIFETIME: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub(super) enum BrpAuth {
    None,
    ApiKey(SecretString),
    ClientCredentials(TokenSource),
}

impl BrpAuth {
    pub(super) fn new(config: &BrpAuthConfig) -> Self {
        match config {
            BrpAuthConfig::None => Self::None,
            BrpAuthConfig::ApiKey(key) => Self::ApiKey(key.clone()),
            BrpAuthConfig::ClientCredentials(credentials) => {
                Self::ClientCredentials(TokenSource::new(credentials.clone()))
            }
        }
    }

    /// The bearer token to send with the next request, fetching one first if
    /// there is none that is still valid.
    pub(super) async fn bearer_token(
        &self,
        http_client: &Client,
    ) -> Result<Option<SecretString>, AppError> {
        match self {
            Self::None => Ok(None),
            Self::ApiKey(key) => Ok(Some(key.clone())),
            Self::ClientCredentials(source) => source.token(http_client).await.map(Some),
        }
    }

    /// Whether a rejected token can be replaced by a fresh one, in which case
    /// the cached token has been dropped.
    pub(super) async fn discard_rejected_token(&self) -> bool {
        match self {
            Self::None | Self::ApiKey(_) => false,
            Self::ClientCredentials(source) => {
                source.discard().await;
                true
            }
        }
    }
}

/// OAuth 2.0 client-credentials token source with one shared cached token.
///
/// The lock is held while a token is being fetched, so concurrent requests
/// that find the cache empty wait for one fetch instead of each doing their
/// own.
#[derive(Clone)]
pub(super) struct TokenSource {
    credentials: BrpClientCredentials,
    cached: Arc<Mutex<Option<CachedToken>>>,
}

struct CachedToken {
    access_token: SecretString,
    renew_at: Instant,
}

/// The token endpoint's answer (RFC 6749 §5.1). The token is a secret from
/// the moment it is deserialized.
#[derive(Deserialize)]
struct TokenResponse {
    access_token: SecretString,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

impl TokenSource {
    fn new(credentials: BrpClientCredentials) -> Self {
        Self {
            credentials,
            cached: Arc::new(Mutex::new(None)),
        }
    }

    async fn token(&self, http_client: &Client) -> Result<SecretString, AppError> {
        let mut cached = self.cached.lock().await;

        if let Some(token) = cached
            .as_ref()
            .filter(|token| Instant::now() < token.renew_at)
        {
            return Ok(token.access_token.clone());
        }

        let token = self.fetch(http_client).await?;
        let access_token = token.access_token.clone();
        *cached = Some(token);
        Ok(access_token)
    }

    async fn discard(&self) {
        *self.cached.lock().await = None;
    }

    async fn fetch(&self, http_client: &Client) -> Result<CachedToken, AppError> {
        let credentials = &self.credentials;
        debug!(
            "Requesting a BRP access token for client {} with scope {}",
            credentials.client_id(),
            credentials.scope()
        );

        let mut form: Vec<(&str, &str)> = vec![
            ("grant_type", "client_credentials"),
            ("client_id", credentials.client_id()),
            ("client_secret", credentials.client_secret().expose_secret()),
        ];
        form.extend(
            credentials
                .parameters()
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
        );

        let response = http_client
            .post(credentials.token_url().clone())
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&form)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            // The body describes the refusal (RFC 6749 §5.2) without holding
            // any secret, so it can be logged.
            let body = response.text().await.unwrap_or_default();
            warn!("The BRP token endpoint answered {status}: {body}");
            return Err(AppError::BrpError(format!(
                "the token endpoint refused the client credentials with status {status}"
            )));
        }

        let token: TokenResponse = response.json().await.map_err(|err| {
            AppError::BrpError(format!("the token endpoint answered unreadably: {err}"))
        })?;
        if let Some(token_type) = token.token_type.as_deref()
            && !token_type.eq_ignore_ascii_case("bearer")
        {
            return Err(AppError::BrpError(format!(
                "the token endpoint issued a {token_type} token instead of a bearer token"
            )));
        }
        if token.access_token.expose_secret().trim().is_empty() {
            return Err(AppError::BrpError(
                "the token endpoint issued an empty access token".to_string(),
            ));
        }

        Ok(CachedToken {
            access_token: token.access_token,
            renew_at: Instant::now() + renewal_delay(token.expires_in),
        })
    }
}

/// How long a token that came with `expires_in` is used before a new one is
/// requested.
fn renewal_delay(expires_in: Option<u64>) -> Duration {
    expires_in
        .map_or(DEFAULT_TOKEN_LIFETIME, Duration::from_secs)
        .min(MAX_TOKEN_LIFETIME)
        .saturating_sub(EXPIRY_MARGIN)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        net::SocketAddr,
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    use axum::{
        Form, Json, Router,
        extract::State,
        http::{HeaderMap, StatusCode, header::AUTHORIZATION},
        routing::post,
    };
    use serde_json::{Value, json};
    use tokio::net::TcpListener;

    use super::*;
    use crate::{
        BrpConfig, constants,
        structs::brp::{BrpClient, BrpField, client::BrpQuery},
    };

    /// A token endpoint handing out numbered tokens next to a `personen`
    /// endpoint that only accepts the token issued last.
    #[derive(Clone, Default)]
    struct OAuthStub {
        issued: Arc<AtomicUsize>,
        token_requests: Arc<parking_lot::Mutex<Vec<HashMap<String, String>>>>,
        /// Set to make the `personen` endpoint reject the current token once,
        /// as a token that expired earlier than announced would be.
        reject_once: Arc<AtomicBool>,
    }

    impl OAuthStub {
        fn current_token(&self) -> String {
            format!("token-{}", self.issued.load(Ordering::SeqCst))
        }
    }

    async fn token_endpoint(
        State(stub): State<OAuthStub>,
        Form(form): Form<HashMap<String, String>>,
    ) -> Json<Value> {
        stub.issued.fetch_add(1, Ordering::SeqCst);
        stub.token_requests.lock().push(form);
        Json(json!({
            "access_token": stub.current_token(),
            "token_type": "bearer",
            "expires_in": 3599,
            "scope": "brp",
        }))
    }

    async fn persons_endpoint(
        State(stub): State<OAuthStub>,
        headers: HeaderMap,
    ) -> Result<Json<Value>, StatusCode> {
        let expected = format!("Bearer {}", stub.current_token());
        let presented = headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok());
        if presented != Some(expected.as_str()) || stub.reject_once.swap(false, Ordering::SeqCst) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        Ok(Json(json!({
            "type": "RaadpleegMetBurgerservicenummer",
            "personen": [],
        })))
    }

    async fn serve(stub: OAuthStub) -> SocketAddr {
        let router = Router::new()
            .route("/token", post(token_endpoint))
            .route(
                &format!("/{}", constants::BRP_PERSONS_ENDPOINT),
                post(persons_endpoint),
            )
            .with_state(stub);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        addr
    }

    async fn client_against(stub: OAuthStub) -> (BrpClient, OAuthStub) {
        let addr = serve(stub.clone()).await;
        let mut config = BrpConfig::new_test(&format!("http://{addr}"));
        config.auth = BrpAuthConfig::ClientCredentials(
            format!("http://my-client:s3cret@{addr}/token?scope=brp&resourceServer=RS01")
                .parse()
                .unwrap(),
        );
        (BrpClient::new(&config).unwrap(), stub)
    }

    fn query() -> BrpQuery {
        BrpQuery::ConsultWithBsn {
            bsn: vec!["999993653".parse().unwrap()],
            fields: vec![BrpField::Bsn],
        }
    }

    #[tokio::test]
    async fn the_token_request_carries_the_credentials_and_parameters() {
        let (client, stub) = client_against(OAuthStub::default()).await;

        client.get_persons(&query()).await.expect("answers");

        let requests = stub.token_requests.lock();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0],
            HashMap::from([
                ("grant_type".to_string(), "client_credentials".to_string()),
                ("client_id".to_string(), "my-client".to_string()),
                ("client_secret".to_string(), "s3cret".to_string()),
                ("scope".to_string(), "brp".to_string()),
                ("resourceServer".to_string(), "RS01".to_string()),
            ])
        );
    }

    #[tokio::test]
    async fn one_token_serves_many_requests() {
        let (client, stub) = client_against(OAuthStub::default()).await;

        for _ in 0..3 {
            client.get_persons(&query()).await.expect("answers");
        }

        assert_eq!(stub.issued.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn concurrent_first_requests_share_one_token_fetch() {
        let (client, stub) = client_against(OAuthStub::default()).await;

        let requests: Vec<_> = (0..5)
            .map(|_| {
                let client = client.clone();
                tokio::spawn(async move { client.get_persons(&query()).await })
            })
            .collect();
        for request in requests {
            request.await.unwrap().expect("answers");
        }

        assert_eq!(stub.issued.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_rejected_token_is_replaced_once() {
        let (client, stub) = client_against(OAuthStub::default()).await;
        client.get_persons(&query()).await.expect("answers");

        stub.reject_once.store(true, Ordering::SeqCst);
        client
            .get_persons(&query())
            .await
            .expect("a fresh token is fetched and the request repeated");

        assert_eq!(stub.issued.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_refused_token_request_is_an_error() {
        let router = Router::new().route(
            "/token",
            post(|| async {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({ "error": "invalid_client" })),
                )
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut config = BrpConfig::new_test(&format!("http://{addr}"));
        config.auth = BrpAuthConfig::ClientCredentials(
            format!("http://my-client:wrong@{addr}/token?scope=brp")
                .parse()
                .unwrap(),
        );
        let client = BrpClient::new(&config).unwrap();

        let result = client.get_persons(&query()).await;

        assert!(matches!(result, Err(AppError::BrpError(_))), "{result:?}");
    }

    #[test]
    fn a_token_is_renewed_before_it_expires_and_never_trusted_past_ten_minutes() {
        assert_eq!(renewal_delay(Some(3599)), Duration::from_secs(570));
        assert_eq!(renewal_delay(Some(120)), Duration::from_secs(90));
        assert_eq!(renewal_delay(Some(10)), Duration::ZERO);
        assert_eq!(renewal_delay(None), Duration::from_secs(30));
    }
}
