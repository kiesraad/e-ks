//! Minimal GitHub OAuth (authorization-code) client for the CSB login: builds
//! the authorize URL, exchanges the callback code for an access token, and
//! resolves the authenticated user's numeric account id. Every GitHub-specific
//! endpoint, header, and parameter lives in the constants below.

use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use tracing::warn;

use crate::{AppError, GithubOauthConfig, GithubUserId};

/// GitHub's OAuth authorization endpoint (step 1: user consent).
const AUTHORIZE_URL: &str = "https://github.com/login/oauth/authorize";
/// GitHub's OAuth token endpoint (step 2: code for access token, server-side).
const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
/// GitHub's REST endpoint for the authenticated user (step 3: identity).
const USER_URL: &str = "https://api.github.com/user";
/// Pinned GitHub REST API version, sent on every API request.
const API_VERSION_HEADER: (&str, &str) = ("x-github-api-version", "2022-11-28");
/// Media type for GitHub REST API responses.
const API_ACCEPT: &str = "application/vnd.github+json";
/// GitHub requires a `User-Agent` identifying the calling application.
const USER_AGENT: &str = concat!("e-KS/", env!("CARGO_PKG_VERSION"));
/// Upper bound on each request to GitHub.
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// The token endpoint reports failures (invalid/expired code) with HTTP 200
/// and an `error` field instead of an `access_token`. The token is a secret
/// from the moment it is deserialized.
#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<SecretString>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct UserResponse {
    id: GithubUserId,
}

/// URL of GitHub's consent page for this app and `state` nonce. No scopes are
/// requested: the flow only reads the authenticated user's public identity.
/// `allow_signup=false` keeps GitHub's sign-up prompt out of the flow.
pub(super) fn authorize_url(
    config: &GithubOauthConfig,
    state_nonce: &str,
) -> Result<String, AppError> {
    let query = serde_urlencoded::to_string([
        ("client_id", config.client_id.as_str()),
        ("state", state_nonce),
        ("allow_signup", "false"),
    ])
    .map_err(|_| AppError::InternalServerError)?;
    Ok(format!("{AUTHORIZE_URL}?{query}"))
}

/// Completes the OAuth code exchange and returns the numeric account id of
/// the user who authorized it.
pub(super) async fn authenticated_user_id(
    config: &GithubOauthConfig,
    code: &str,
) -> Result<GithubUserId, AppError> {
    let client = http_client()?;
    let access_token = exchange_code(&client, config, code).await?;
    fetch_user_id(&client, &access_token).await
}

/// Client with a pinned `User-Agent`, a timeout, and redirects disabled: the
/// GitHub endpoints never redirect, and following one could leak credentials.
fn http_client() -> Result<reqwest::Client, AppError> {
    Ok(reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(HTTP_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}

/// Exchanges the callback `code` for an access token.
async fn exchange_code(
    client: &reqwest::Client,
    config: &GithubOauthConfig,
    code: &str,
) -> Result<SecretString, AppError> {
    let response: TokenResponse = client
        .post(TOKEN_URL)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&[
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.expose_secret()),
            ("code", code),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let Some(access_token) = response.access_token else {
        let error = response.error.as_deref().unwrap_or("unknown error");
        warn!("GitHub token exchange failed: {error}");
        return Err(AppError::Unauthorised);
    };
    Ok(access_token)
}

/// Resolves the numeric account id the access token belongs to.
async fn fetch_user_id(
    client: &reqwest::Client,
    access_token: &SecretString,
) -> Result<GithubUserId, AppError> {
    let user: UserResponse = client
        .get(USER_URL)
        .bearer_auth(access_token.expose_secret())
        .header(reqwest::header::ACCEPT, API_ACCEPT)
        .header(API_VERSION_HEADER.0, API_VERSION_HEADER.1)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(user.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> GithubOauthConfig {
        GithubOauthConfig {
            client_id: "Iv1.abc123".to_string(),
            client_secret: SecretString::from("s3cret"),
            allowed_user_ids: vec!["583231".parse().expect("valid id")],
        }
    }

    /// The authorize URL carries the client id and nonce, percent-encoded.
    #[test]
    fn authorize_url_encodes_client_id_and_state() {
        let url = authorize_url(&test_config(), "n0nce&x=y").expect("authorize url");

        assert!(url.starts_with("https://github.com/login/oauth/authorize?"));
        assert!(url.contains("client_id=Iv1.abc123"));
        assert!(url.contains("state=n0nce%26x%3Dy"));
        assert!(url.contains("allow_signup=false"));
        // No scopes: the flow must not request any repository or org access.
        assert!(!url.contains("scope="));
    }

    #[test]
    fn token_response_parses_success_and_error_payloads() {
        let ok: TokenResponse =
            serde_json::from_str(r#"{"access_token":"gho_abc","token_type":"bearer","scope":""}"#)
                .expect("token json");
        assert_eq!(
            ok.access_token.as_ref().map(ExposeSecret::expose_secret),
            Some("gho_abc")
        );

        let err: TokenResponse = serde_json::from_str(
            r#"{"error":"bad_verification_code","error_description":"The code is incorrect."}"#,
        )
        .expect("error json");
        assert!(err.access_token.is_none());
        assert_eq!(err.error.as_deref(), Some("bad_verification_code"));
    }

    #[test]
    fn user_response_parses_numeric_account_id() {
        let user: UserResponse =
            serde_json::from_str(r#"{"login":"octocat","id":583231,"type":"User"}"#)
                .expect("user json");
        assert_eq!(user.id, "583231".parse().expect("valid id"));
    }
}
