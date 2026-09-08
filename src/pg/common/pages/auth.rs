//! Pre-session pages for the SAML login flow: login start, logout confirmation
//! (TVS T7), and cancelled/failed authentication (T3/L10), plus the
//! [`AuthState`] hooks the auth-service invokes on login and logout.

use askama::Template;
use auth_service::{AuthFailure, AuthServiceState, AuthState, RedirectTarget, handle_logout};
use axum::{
    extract::{FromRef, State},
    http::{HeaderMap, HeaderName, HeaderValue},
    response::{IntoResponse, Response},
};
use axum_extra::extract::CookieJar;

use axum_extra::routing::TypedPath;

use super::{LoggedOutPath, LogoutPath};
use crate::{
    AppRequestState, Context, HtmlTemplate, Locale, LocaleValues, Session, SessionPageValues,
    filters,
};

#[derive(Template)]
#[template(path = "pg/common/pages/login_start.html")]
struct LoginStartTemplate;

#[derive(Template)]
#[template(path = "pg/common/pages/logged_out.html")]
struct LoggedOutTemplate;

#[derive(Template)]
#[template(path = "pg/common/pages/logout_confirm.html")]
struct LogoutConfirmTemplate;

#[derive(Template)]
#[template(path = "pg/common/pages/auth_error.html")]
struct AuthErrorTemplate;

#[derive(Template)]
#[template(path = "pg/common/pages/auth_cancelled.html")]
struct AuthCancelledTemplate;

#[derive(Template)]
#[template(path = "pg/common/pages/auth_unavailable.html")]
struct AuthUnavailableTemplate;

/// GET `/login`: DigiD start page with login button and flow explanation.
pub async fn login_start(headers: HeaderMap) -> impl IntoResponse {
    HtmlTemplate(
        LoginStartTemplate,
        LocaleValues {
            locale: Locale::from_headers(&headers),
        },
    )
}

/// GET `/logout`: the logout prompt; runs behind the session middleware, which
/// supplies the session and CSRF-checks the prompt's POST.
pub async fn logout(_: LogoutPath, session: Session) -> Response {
    HtmlTemplate(
        LogoutConfirmTemplate,
        SessionPageValues {
            locale: session.locale,
            csrf_token: session.csrf_token().0.clone(),
        },
    )
    .into_response()
}

/// Where the auth-service sends the browser once logout completes. Built from
/// [`LoggedOutPath`]'s own `PATH`, so the redirect and the route that serves it
/// cannot drift.
const LOGGED_OUT: RedirectTarget = RedirectTarget::from_static(LoggedOutPath::PATH);

/// POST `/logout`: starts SP-initiated logout (eID §7.7.1); the session
/// middleware has already verified the CSRF token.
pub async fn logout_submit<S>(_: LogoutPath, State(state): State<S>, jar: CookieJar) -> Response
where
    S: AppRequestState + AuthState,
    AuthServiceState: FromRef<S>,
{
    handle_logout(&state, jar, &LOGGED_OUT).await
}

/// GET `/logged-out`: post-logout confirmation page (TVS T7, also the SLO
/// landing). Public: by definition there is no session left to show it with.
///
/// `Clear-Site-Data` drops what the session left on the client. `"cookies"` is
/// left out: browsers widen it to the whole registrable domain, which would sign
/// the user out of unrelated `kiesraad.nl` sites.
pub async fn logged_out(_: LoggedOutPath, headers: HeaderMap) -> Response {
    let mut response = HtmlTemplate(
        LoggedOutTemplate,
        LocaleValues {
            locale: Locale::from_headers(&headers),
        },
    )
    .into_response();

    response.headers_mut().insert(
        HeaderName::from_static("clear-site-data"),
        HeaderValue::from_static("\"cache\", \"storage\""),
    );

    response
}

/// Response page for a cancelled (T3), failed (L10), or unavailable auth attempt.
pub fn auth_failure_response(failure: AuthFailure, locale: Locale) -> Response {
    let values = LocaleValues { locale };
    match failure {
        AuthFailure::Cancelled => HtmlTemplate(AuthCancelledTemplate, values).into_response(),
        AuthFailure::Error => HtmlTemplate(AuthErrorTemplate, values).into_response(),
        AuthFailure::Unavailable => HtmlTemplate(AuthUnavailableTemplate, values).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use auth_service::{LoggedOutSession, NameId, SubjectId};
    use axum::{
        body::Body,
        http::{Request, StatusCode, header},
    };
    use axum_extra::extract::cookie::Cookie;
    use secrecy::SecretString;
    use tower::ServiceExt;

    // Only the fixtures-gated test below redirects to the index.
    #[cfg(feature = "fixtures")]
    use crate::common::PgIndexPath;

    use crate::{
        auth::session_extractor::SESSION_COOKIE_NAME, common::SelectElectionPath,
        test_utils::response_body_string,
    };

    #[tokio::test]
    async fn login_start_shows_digid_button_and_explanation() {
        let response = login_start(HeaderMap::new()).await.into_response();
        let body = response_body_string(response).await;
        assert!(body.contains("Inloggen"));
        // The button initiates SSO by POSTing back to /login.
        assert!(body.contains("action=\"/login\""));
        assert!(body.contains("method=\"post\""));
        assert!(body.contains("Kandidaatstelling van de Kiesraad"));
    }

    #[tokio::test]
    async fn logged_out_page_confirms_and_offers_login() {
        let response = logged_out(LoggedOutPath, HeaderMap::new()).await;
        assert!(response.status().is_success());
        let body = response_body_string(response).await;
        assert!(body.contains("U bent uitgelogd"));
        assert!(body.contains("Inloggen"));
    }

    /// Without a session the middleware redirects /logout to the login page.
    #[tokio::test]
    async fn logout_without_session_redirects_to_login() {
        let state = crate::AppState::new_for_tests().await;
        let app = crate::app::router::create(state.clone()).with_state(state);

        let request = Request::builder()
            .uri("/logout")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/login");
    }

    /// The full logout flow behind the session middleware: the prompt renders
    /// with the session's token, and the CSRF-checked POST removes the session
    /// and lands on the /logged-out confirmation.
    #[tokio::test]
    async fn logout_flow_prompts_then_removes_session() {
        let state = crate::AppState::new_for_tests().await;
        let app = crate::app::router::create(state.clone()).with_state(state.clone());

        let session = Session::new_test();
        let token = session.token_string();
        let csrf = session.csrf_token().0.clone();
        state.sessions().insert(session).await;
        let cookie = format!("{}={token}", crate::SESSION_COOKIE_NAME);

        let request = Request::builder()
            .uri("/logout")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains(&csrf));

        let request = Request::builder()
            .method("POST")
            .uri("/logout")
            .header(header::COOKIE, &cookie)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(format!("csrf_token={csrf}")))
            .unwrap();
        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response.headers().get(header::LOCATION).unwrap(),
            &LoggedOutPath.to_string()
        );
        assert!(
            state
                .sessions
                .get_existing(Some(&token))
                .await
                .expect("load session")
                .is_none()
        );
    }

    /// True when the jar emits an expiring `Set-Cookie` for the session cookie.
    fn clears_session_cookie(jar: CookieJar) -> bool {
        let response = (jar, axum::http::StatusCode::OK).into_response();
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .any(|cookie| cookie.starts_with(SESSION_COOKIE_NAME) && cookie.contains("Max-Age=0"))
    }

    /// Jar with the session cookie as an *original* request cookie: `remove`
    /// only emits a removal `Set-Cookie` for cookies present on the request.
    fn jar_with_session_cookie(token: &str) -> CookieJar {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("{SESSION_COOKIE_NAME}={token}").parse().unwrap(),
        );
        CookieJar::from_headers(&headers)
    }

    fn subject(value: &str) -> SubjectId {
        SubjectId {
            value: SecretString::from(value.to_string()),
            name_qualifier: "DV".to_string(),
        }
    }

    #[tokio::test]
    async fn on_authenticated_redirects_to_select_election_when_no_data() {
        let state = crate::AppState::new_for_tests().await;

        let response = state
            .on_authenticated(
                subject("999999990"),
                NameId::parse("name-id-xyz").unwrap(),
                CookieJar::new(),
                &HeaderMap::new(),
            )
            .await;

        let location = response
            .headers()
            .get(header::LOCATION)
            .expect("redirect location")
            .to_str()
            .unwrap();
        assert_eq!(location, SelectElectionPath.to_string());
    }

    #[cfg(feature = "fixtures")]
    #[tokio::test]
    async fn on_authenticated_attaches_existing_election() {
        let state = crate::AppState::new_for_tests().await;
        let id_code = SecretString::from("999999990");
        let stream_id = state.id_deriver().derive_stream_id(&id_code);

        // Load fixtures so the registry sees events for this (stream, election).
        state
            .store_for_stream(stream_id, crate::ElectionConfig::EK27, true)
            .await
            .unwrap();

        let response = state
            .on_authenticated(
                subject("999999990"),
                NameId::parse("name-id-xyz").unwrap(),
                CookieJar::new(),
                &HeaderMap::new(),
            )
            .await;

        let location = response
            .headers()
            .get(header::LOCATION)
            .expect("redirect location")
            .to_str()
            .unwrap();
        assert_eq!(location, PgIndexPath.to_string());
    }

    /// Signing in and out is recorded on the stream's audit log.
    #[cfg(feature = "fixtures")]
    #[tokio::test]
    async fn login_and_logout_are_recorded_on_the_stream() {
        use crate::{ElectionConfig, PgEvent, PgStore, store::StoreData};

        let state = crate::AppState::new_for_tests().await;
        let id_code = SecretString::from("999999990");
        let stream_id = state.id_deriver().derive_stream_id(&id_code);
        let store = PgStore::own(
            state
                .store_for_stream(stream_id, ElectionConfig::EK27, true)
                .await
                .unwrap(),
        );

        let response = state
            .on_authenticated(
                subject("999999990"),
                NameId::parse("name-id-xyz").unwrap(),
                CookieJar::new(),
                &HeaderMap::new(),
            )
            .await;
        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .expect("session cookie")
            .to_str()
            .unwrap();
        let token = Cookie::parse(cookie).unwrap().value().to_string();

        let recorded = |store: &PgStore, want: fn(&PgEvent) -> bool| {
            store.data.read().events().iter().any(|e| want(&e.payload))
        };
        assert!(
            recorded(&store, |e| matches!(e, PgEvent::Login)),
            "login must be recorded"
        );

        let _ = state.logout_session(jar_with_session_cookie(&token)).await;

        assert!(
            recorded(&store, |e| matches!(e, PgEvent::Logout)),
            "logout must be recorded"
        );
    }

    #[tokio::test]
    async fn logout_session_without_cookie_reports_no_session() {
        // Nothing to clear when the request carried no session cookie.
        let state = crate::AppState::new_for_tests().await;
        let (_jar, ended) = state.logout_session(CookieJar::new()).await;
        assert_eq!(ended, LoggedOutSession::None);
    }

    #[tokio::test]
    async fn logout_session_unknown_session_clears_and_reports_no_session() {
        let state = crate::AppState::new_for_tests().await;
        let jar = jar_with_session_cookie("no-such-token");
        let (jar, ended) = state.logout_session(jar).await;
        assert_eq!(ended, LoggedOutSession::None);
        assert!(clears_session_cookie(jar));
    }

    #[tokio::test]
    async fn logout_session_returns_name_id_and_clears_cookie() {
        let state = crate::AppState::new_for_tests().await;
        let session = Session::for_political_group(
            crate::StreamId::new(),
            "name-id-xyz".to_string(),
            None,
            crate::Locale::default(),
        );
        let token = session.token_string();
        state.sessions().insert(session).await;

        let jar = jar_with_session_cookie(&token);
        let (jar, ended) = state.logout_session(jar).await;
        assert_eq!(
            ended,
            LoggedOutSession::WithSamlSubject(NameId::parse("name-id-xyz").unwrap())
        );
        assert!(clears_session_cookie(jar));
        assert!(
            state
                .sessions
                .get_existing(Some(&token))
                .await
                .expect("load session")
                .is_none()
        );
    }

    #[tokio::test]
    async fn logout_session_with_empty_name_id_still_clears_and_ends_session() {
        // A session with no SAML NameID (the dev-login bypass) must still clear
        // the cookie and end the session, but has no RD session to log out: no
        // LogoutRequest is built for it.
        let state = crate::AppState::new_for_tests().await;
        let session = Session::new_test(); // saml_name_id defaults to ""
        let token = session.token_string();
        state.sessions().insert(session).await;

        let jar = jar_with_session_cookie(&token);
        let (jar, ended) = state.logout_session(jar).await;
        assert_eq!(ended, LoggedOutSession::WithoutSamlSubject);
        assert!(clears_session_cookie(jar));
        assert!(
            state
                .sessions
                .get_existing(Some(&token))
                .await
                .expect("load session")
                .is_none()
        );
    }

    #[tokio::test]
    async fn on_authenticated_invalidates_prior_session() {
        // A fresh login must not leave the pre-login session alive server-side.
        let state = crate::AppState::new_for_tests().await;
        let old = Session::new_test();
        let old_token = old.token_string();
        state.sessions().insert(old).await;

        let jar = CookieJar::new().add(Cookie::new(SESSION_COOKIE_NAME, old_token.clone()));
        let _ = state
            .on_authenticated(
                subject("999999990"),
                NameId::parse("name-id-xyz").unwrap(),
                jar,
                &HeaderMap::new(),
            )
            .await;

        assert!(
            state
                .sessions
                .get_existing(Some(&old_token))
                .await
                .expect("load session")
                .is_none()
        );
    }

    #[tokio::test]
    async fn on_authentication_failed_terminates_local_session() {
        // TVS L10: a failed authentication must end any existing local session.
        let state = crate::AppState::new_for_tests().await;
        let session = Session::new_test();
        let token = session.token_string();
        state.sessions().insert(session).await;

        let jar = CookieJar::new().add(Cookie::new(SESSION_COOKIE_NAME, token.clone()));
        let response = state
            .on_authentication_failed(AuthFailure::Error, jar, &HeaderMap::new())
            .await;

        assert!(
            state
                .sessions
                .get_existing(Some(&token))
                .await
                .expect("load session")
                .is_none()
        );
        assert!(response.status().is_success());
    }

    // TVS L10 requires this literal text on a DigiD result error.
    const L10_MESSAGE: &str = "Inloggen bij deze organisatie is niet gelukt. \
        Probeert u het later nog een keer. Lukt het nog steeds niet? Log in bij \
        Mijn DigiD. Zo controleert u of uw DigiD goed werkt. Mogelijk is er een \
        storing bij de organisatie waar u inlogt.";

    #[tokio::test]
    async fn error_page_shows_mandated_digid_message() {
        let response = auth_failure_response(AuthFailure::Error, Locale::Nl);
        let body = response_body_string(response).await;
        assert!(body.contains(L10_MESSAGE), "{body}");
    }

    #[tokio::test]
    async fn cancelled_page_shows_cancellation_notice() {
        let response = auth_failure_response(AuthFailure::Cancelled, Locale::Nl);
        let body = response_body_string(response).await;
        assert!(body.contains("Inloggen geannuleerd"));
    }

    #[tokio::test]
    async fn unavailable_page_shows_temporary_notice() {
        let response = auth_failure_response(AuthFailure::Unavailable, Locale::Nl);
        let body = response_body_string(response).await;
        assert!(body.contains("Inloggen tijdelijk niet mogelijk"));
    }
}
