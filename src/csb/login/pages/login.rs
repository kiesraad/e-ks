//! `GET /csb/login`: the CSB security-key login page, which starts the
//! ceremony: it mints the challenge, binds it to the browser, and hands the
//! browser script the options for `navigator.credentials.get`. Answers 404
//! unless [`crate::CsbWebauthnConfig`] is present.

use askama::Template;
use axum::{
    extract::{Query, State},
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};

use crate::{
    AppError, AppRequestState, Context, CsbWebauthnConfig, HtmlTemplate, Locale, LocaleValues,
    csb::login::{
        CsbLoginPath, pending_challenge_id, require_webauthn,
        state_cookie::{build_state_cookie, build_state_removal_cookie},
    },
    filters,
};

/// How long the browser waits for the key; the pending-request TTL bounds
/// the challenge server-side.
const CEREMONY_TIMEOUT_MS: u32 = 300_000;

#[derive(Template)]
#[template(path = "csb/login/pages/login.html")]
struct CsbLoginTemplate {
    /// Shows the generic login-failed message and waits for a click instead
    /// of prompting for the key straight away.
    show_error: bool,
    /// The ceremony options as JSON; escaped into a data attribute.
    options: String,
}

#[derive(Debug, Deserialize)]
pub struct CsbLoginQuery {
    error: Option<String>,
}

/// `PublicKeyCredentialRequestOptions` as the browser expects them, with the
/// binary fields base64url (the script decodes them).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestOptions<'a> {
    public_key: PublicKeyOptions<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicKeyOptions<'a> {
    challenge: &'a str,
    rp_id: &'a str,
    timeout: u32,
    user_verification: &'static str,
    /// Every configured key: the one that answers identifies the user.
    allow_credentials: Vec<AllowCredential>,
}

#[derive(Serialize)]
struct AllowCredential {
    #[serde(rename = "type")]
    type_: &'static str,
    id: String,
    transports: [&'static str; 1],
}

fn request_options(config: &CsbWebauthnConfig, challenge: &str) -> Result<String, AppError> {
    let options = RequestOptions {
        public_key: PublicKeyOptions {
            challenge,
            rp_id: &config.rp_id,
            timeout: CEREMONY_TIMEOUT_MS,
            user_verification: "required",
            allow_credentials: config
                .users
                .iter()
                .map(|user| AllowCredential {
                    type_: "public-key",
                    id: BASE64_URL_SAFE_NO_PAD.encode(&user.credential_id),
                    transports: ["usb"],
                })
                .collect(),
        },
    };
    Ok(serde_json::to_string(&options)?)
}

/// GET `/csb/login`: registers a fresh one-shot challenge, binds it to this
/// browser with the challenge cookie, and renders the ceremony page.
pub async fn login_page<S: AppRequestState>(
    _: CsbLoginPath,
    State(state): State<S>,
    Query(query): Query<CsbLoginQuery>,
    jar: CookieJar,
) -> Result<Response, AppError> {
    let config = require_webauthn(state.config())?;

    let challenge = BASE64_URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
    state
        .pending_requests()
        .register(pending_challenge_id(&challenge))
        .await;
    let template = CsbLoginTemplate {
        show_error: query.error.is_some(),
        options: request_options(config, &challenge)?,
    };

    Ok((
        jar.add(build_state_cookie(challenge)),
        HtmlTemplate(
            template,
            LocaleValues {
                locale: Locale::default(),
            },
        ),
    )
        .into_response())
}

/// Clears the challenge cookie and sends the browser back to the login page
/// with a generic error, deliberately not revealing which check failed.
pub(super) fn login_failed(jar: CookieJar) -> Response {
    let login_error = format!("{CsbLoginPath}?error=1");
    (
        jar.remove(build_state_removal_cookie()),
        Redirect::to(&login_error),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    use crate::{
        csb::login::{
            pages::tests::{ceremony_options, cookie_value, test_app},
            pending_challenge_id,
            state_cookie::STATE_COOKIE_NAME,
            test_support,
        },
        test_utils::response_body_string,
    };

    use super::*;

    /// Loading the page yields the ceremony options for every configured
    /// key, the challenge in the cookie, and the challenge registered
    /// one-shot.
    #[tokio::test]
    async fn login_page_starts_ceremony_for_configured_keys() {
        let (state, app) = test_app().await;

        let request = Request::builder()
            .uri("/csb/login")
            .body(Body::empty())
            .expect("valid request");
        let response = app.oneshot(request).await.expect("response");

        let cookie = cookie_value(&response, STATE_COOKIE_NAME).expect("challenge cookie");
        let body = response_body_string(response).await;
        assert!(body.contains("action=\"/csb/login/finish\""));
        let failed_message = crate::trans!("csb.auth.error.message", Locale::default());
        assert!(!body.contains(&failed_message));
        assert!(!body.contains("data-webauthn-autostart"));

        let (options, challenge) = ceremony_options(&body);
        assert_eq!(options["publicKey"]["rpId"], "csb.example.nl");
        assert_eq!(options["publicKey"]["userVerification"], "required");
        assert_eq!(
            options["publicKey"]["allowCredentials"][0]["id"],
            BASE64_URL_SAFE_NO_PAD.encode(test_support::test_credential_id())
        );
        assert_eq!(
            options["publicKey"]["allowCredentials"][0]["type"],
            "public-key"
        );
        assert_eq!(
            options["publicKey"]["allowCredentials"]
                .as_array()
                .map(Vec::len),
            Some(1)
        );

        assert_eq!(
            BASE64_URL_SAFE_NO_PAD.decode(&cookie).expect("base64url"),
            challenge
        );
        assert_eq!(challenge.len(), 32);
        assert!(
            state
                .pending_requests
                .consume_if_pending(&pending_challenge_id(&cookie))
                .await
        );
    }

    #[tokio::test]
    async fn login_page_shows_generic_error_when_flagged() {
        let (_state, app) = test_app().await;

        let request = Request::builder()
            .uri("/csb/login?error=1")
            .body(Body::empty())
            .expect("valid request");
        let response = app.oneshot(request).await.expect("response");

        let body = response_body_string(response).await;
        let failed_message = crate::trans!("csb.auth.error.message", Locale::default());
        assert!(body.contains(&failed_message));
        assert!(body.contains("data-webauthn-autostart=\"false\""));
    }

    #[tokio::test]
    async fn login_page_is_not_found_without_webauthn_config() {
        let state = crate::AppState::new_for_tests().await;

        let err = login_page(
            CsbLoginPath,
            State(state),
            Query(CsbLoginQuery { error: None }),
            CookieJar::new(),
        )
        .await
        .expect_err("404 without config");

        assert!(matches!(err, AppError::GenericNotFound));
    }

    #[test]
    fn request_options_serialise_as_the_browser_expects() {
        let config = test_support::webauthn_test_config();
        let json = request_options(
            config.csb_webauthn.as_ref().expect("config"),
            "Y2hhbGxlbmdl",
        )
        .expect("json");
        assert!(json.contains(r#""challenge":"Y2hhbGxlbmdl""#));
        assert!(json.contains(r#""timeout":300000"#));
        assert!(json.contains(r#""transports":["usb"]"#));
    }
}
