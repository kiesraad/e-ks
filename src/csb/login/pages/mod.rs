use axum::Router;
use axum_extra::routing::RouterExt;

use crate::AppRequestState;

mod finish;
mod login;

/// Routes mounted outside the session middleware: they run before a session
/// exists. The page is a GET, the finish step a same-origin form post.
pub fn public_router<S: AppRequestState>() -> Router<S> {
    Router::new()
        .typed_get(login::login_page::<S>)
        .typed_post(finish::login_finish::<S>)
}

#[cfg(test)]
pub(super) mod tests {
    use axum::{
        Router,
        body::Body,
        http::{Method, Request, StatusCode, header},
        response::Response,
    };
    use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
    use tower::ServiceExt;

    use crate::{
        AppState,
        csb::login::{state_cookie::STATE_COOKIE_NAME, test_support},
        test_utils::response_body_string,
    };

    pub(in crate::csb::login) async fn test_app() -> (AppState, Router) {
        let state = AppState::new_for_tests_with_config(test_support::webauthn_test_config()).await;
        let app = crate::app::router::create(state.clone()).with_state(state.clone());
        (state, app)
    }

    /// A form post as the browser sends it, optionally carrying the
    /// challenge cookie.
    pub(in crate::csb::login) fn form_post(
        uri: &str,
        fields: &[(&str, &str)],
        state_cookie: Option<&str>,
    ) -> Request<Body> {
        let body = serde_urlencoded::to_string(fields).expect("form body");
        let mut request = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
        if let Some(value) = state_cookie {
            request = request.header(header::COOKIE, format!("{STATE_COOKIE_NAME}={value}"));
        }
        request.body(Body::from(body)).expect("valid request")
    }

    /// The value of the named cookie from the response's `Set-Cookie` headers.
    pub(in crate::csb::login) fn cookie_value(response: &Response, name: &str) -> Option<String> {
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter_map(|value| value.split(';').next())
            .filter_map(|pair| pair.split_once('='))
            .find(|(cookie, _)| *cookie == name)
            .map(|(_, value)| value.to_string())
    }

    pub(in crate::csb::login) fn location(response: &Response) -> &str {
        response
            .headers()
            .get(header::LOCATION)
            .expect("redirect location")
            .to_str()
            .expect("ascii location")
    }

    /// The ceremony options the page embeds for the browser script, and the
    /// challenge in them.
    pub(in crate::csb::login) fn ceremony_options(body: &str) -> (serde_json::Value, Vec<u8>) {
        let marker = "data-webauthn-options=\"";
        let attribute = body
            .split(marker)
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("ceremony options attribute");
        // askama escapes with numeric entities.
        let json = attribute
            .replace("&#34;", "\"")
            .replace("&#39;", "'")
            .replace("&#60;", "<")
            .replace("&#62;", ">")
            .replace("&#38;", "&");
        let options: serde_json::Value = serde_json::from_str(&json).expect("options json");
        let challenge = options["publicKey"]["challenge"]
            .as_str()
            .expect("challenge")
            .to_string();
        let challenge = BASE64_URL_SAFE_NO_PAD
            .decode(challenge)
            .expect("base64url challenge");
        (options, challenge)
    }

    /// Loads the login page: the challenge cookie and the challenge the key
    /// must sign.
    pub(in crate::csb::login) async fn start_login(app: &Router) -> (String, Vec<u8>) {
        let request = Request::builder()
            .uri("/csb/login")
            .body(Body::empty())
            .expect("valid request");
        let response = app.clone().oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = cookie_value(&response, STATE_COOKIE_NAME).expect("challenge cookie");
        let (_, challenge) = ceremony_options(&response_body_string(response).await);
        (cookie, challenge)
    }

    #[tokio::test]
    async fn login_routes_answer_not_found_when_unconfigured() {
        let state = AppState::new_for_tests().await;
        let app = crate::app::router::create(state.clone()).with_state(state);

        let request = Request::builder()
            .uri("/csb/login")
            .body(Body::empty())
            .expect("valid request");
        let response = app.clone().oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let request = form_post("/csb/login/finish", &[("credential", "{}")], None);
        let response = app.oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
