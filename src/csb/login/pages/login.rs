//! `GET /csb/login`: the CSB security-key login page, which starts the
//! ceremony: it mints the challenge, binds it to the browser, and hands the
//! browser script the options for `navigator.credentials.get` (step 1 of
//! §7.2, <https://w3c.github.io/webauthn/#sctn-verifying-assertion>).
//! Answers 404 unless [`crate::CsbWebauthnConfig`] is present.

use askama::Template;
use axum::{
    extract::{Query, State},
    response::{IntoResponse, Redirect, Response},
};
use serde::{Deserialize, Serialize};

use crate::{
    AnyLocale, AppError, AppRequestState, Context, CsbWebauthnConfig, ElectionConfig, HtmlTemplate,
    Locale, LocaleValues, Province, WaterCouncil,
    auth::csb_webauthn::Challenge,
    csb::login::{CsbLoginPath, require_webauthn},
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
    /// The elections to pick from; empty when `DEFAULT_ELECTION` pins one.
    /// The ceremony then waits for a click instead of starting straight away.
    elections: Vec<ElectionConfig>,
    title_locale: AnyLocale,
    provinces: &'static [Province],
    water_councils: &'static [WaterCouncil],
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
    /// Random, one-shot (§13.4.3); checked in step 11.
    challenge: &'a str,
    /// Checked as its hash in step 15.
    rp_id: &'a str,
    timeout: u32,
    /// `required`, so step 17 demands the UV flag (the key's PIN).
    user_verification: &'static str,
    /// Every configured key: the one that answers identifies the user
    /// (steps 5 and 6).
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
                    id: user.credential_id.to_string(),
                    transports: ["usb"],
                })
                .collect(),
        },
    };
    Ok(serde_json::to_string(&options)?)
}

/// GET `/csb/login`: registers a fresh one-shot challenge and renders the
/// ceremony page.
pub async fn login_page<S: AppRequestState>(
    _: CsbLoginPath,
    State(state): State<S>,
    Query(query): Query<CsbLoginQuery>,
) -> Result<Response, AppError> {
    let config = require_webauthn(state.config())?;

    let challenge = Challenge::issue(state.pending_requests()).await;
    let template = CsbLoginTemplate {
        show_error: query.error.is_some(),
        options: request_options(config, challenge.as_str())?,
        elections: match state.config().default_election {
            Some(_) => Vec::new(),
            None => ElectionConfig::type_options(),
        },
        title_locale: AnyLocale::from(Locale::default()),
        provinces: Province::ALL,
        water_councils: WaterCouncil::ALL,
    };

    Ok(HtmlTemplate(
        template,
        LocaleValues {
            locale: Locale::default(),
        },
    )
    .into_response())
}

/// Sends the browser back to the login page with a generic error,
/// deliberately not revealing which check failed.
pub(super) fn login_failed() -> Response {
    Redirect::to(&format!("{CsbLoginPath}?error=1")).into_response()
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};

    use crate::{
        auth::csb_webauthn::{consume_challenge, test_support},
        csb::login::pages::tests::{ceremony_options, test_app},
        test_utils::response_body_string,
    };

    use super::*;

    /// Loading the page yields the ceremony options for every configured
    /// key, with the challenge registered one-shot.
    #[tokio::test]
    async fn login_page_starts_ceremony_for_configured_keys() {
        let (state, app) = test_app().await;

        let request = Request::builder()
            .uri("/csb/login")
            .body(Body::empty())
            .expect("valid request");
        let response = app.oneshot(request).await.expect("response");

        let body = response_body_string(response).await;
        assert!(body.contains("action=\"/csb/login/finish\""));
        let failed_message = crate::trans!("csb.auth.error.message", Locale::default());
        assert!(!body.contains(&failed_message));
        assert!(!body.contains("data-webauthn-autostart"));
        assert!(!body.contains("name=\"election\""));
        let start = crate::trans!("csb.auth.ceremony.start", Locale::default());
        assert!(body.contains(&start));

        let (options, challenge) = ceremony_options(&body);
        assert_eq!(options["publicKey"]["rpId"], "csb.example.nl");
        assert_eq!(options["publicKey"]["userVerification"], "required");
        assert_eq!(
            options["publicKey"]["allowCredentials"][0]["id"],
            test_support::test_credential_id().to_string()
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

        assert_eq!(challenge.len(), 32);
        let issued = BASE64_URL_SAFE_NO_PAD.encode(&challenge);
        assert!(
            consume_challenge(&state.pending_requests, &issued)
                .await
                .is_ok()
        );
    }

    /// Without `DEFAULT_ELECTION` the page asks which election to log in
    /// to, so the ceremony waits for the button.
    #[tokio::test]
    async fn login_page_offers_elections_when_not_pinned() {
        let mut config = test_support::webauthn_test_config();
        config.default_election = None;
        let state = crate::AppState::new_for_tests_with_config(config).await;
        let app = crate::app::router::create(state.clone()).with_state(state);

        let request = Request::builder()
            .uri("/csb/login")
            .body(Body::empty())
            .expect("valid request");
        let response = app.oneshot(request).await.expect("response");

        let body = response_body_string(response).await;
        assert!(body.contains("name=\"election\""));
        assert!(body.contains("name=\"domain_province\""));
        assert!(body.contains("data-webauthn-autostart=\"false\""));
        let start = crate::trans!("csb.auth.ceremony.start", Locale::default());
        assert!(body.contains(&start));
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
        let retry = crate::trans!("csb.auth.ceremony.retry", Locale::default());
        assert!(body.contains(&retry));
    }

    #[tokio::test]
    async fn login_page_is_not_found_without_webauthn_config() {
        let state = crate::AppState::new_for_tests().await;

        let err = login_page(
            CsbLoginPath,
            State(state),
            Query(CsbLoginQuery { error: None }),
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
