//! `GET /csb/login`: start page for the CSB GitHub login, and
//! `GET /csb/login/start`: the flow initiation its button links to. Both
//! answer 404 unless [`crate::GithubOauthConfig`] is present.

use askama::Template;
use axum::{
    extract::{Query, State},
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;
use serde::Deserialize;

use crate::{
    AppError, AppRequestState, Context, HtmlTemplate, Locale, LocaleValues,
    csb::login::{
        CsbLoginPath, CsbLoginStartPath, github, pending_state_id, require_github_oauth,
        state_cookie::build_state_cookie,
    },
    filters,
    form::generate_csrf_token,
};

#[derive(Template)]
#[template(path = "csb/login/pages/login.html")]
struct CsbLoginTemplate {
    /// Shows the generic login-failed message (set after a failed callback).
    show_error: bool,
}

#[derive(Debug, Deserialize)]
pub struct CsbLoginQuery {
    error: Option<String>,
}

/// GET `/csb/login`: start page with the GitHub login button.
pub async fn login_start<S: AppRequestState>(
    _: CsbLoginPath,
    State(state): State<S>,
    Query(query): Query<CsbLoginQuery>,
) -> Result<Response, AppError> {
    require_github_oauth(state.config())?;
    Ok(HtmlTemplate(
        CsbLoginTemplate {
            show_error: query.error.is_some(),
        },
        LocaleValues {
            locale: Locale::default(),
        },
    )
    .into_response())
}

/// GET `/csb/login/start`: registers a one-shot `state` nonce, binds it to
/// this browser with a short-lived cookie, and redirects to GitHub's consent
/// page. A link rather than a form post, so the cross-origin redirect is an
/// ordinary navigation that `form-action 'self'` never sees.
pub async fn login_redirect<S: AppRequestState>(
    _: CsbLoginStartPath,
    State(state): State<S>,
    jar: CookieJar,
) -> Result<Response, AppError> {
    let config = require_github_oauth(state.config())?;

    // Random 32-char base62 value (~190 bits), same generator as CSRF tokens.
    let nonce = generate_csrf_token().0;
    state
        .pending_requests()
        .register(pending_state_id(&nonce))
        .await;
    let authorize_url = github::authorize_url(config, &nonce)?;

    Ok((
        jar.add(build_state_cookie(nonce)),
        Redirect::to(&authorize_url),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    use crate::test_utils::response_body_string;

    fn query(error: Option<&str>) -> Query<CsbLoginQuery> {
        Query(CsbLoginQuery {
            error: error.map(str::to_string),
        })
    }

    #[tokio::test]
    async fn login_start_is_not_found_without_github_config() {
        let state = crate::AppState::new_for_tests().await;

        let err = login_start(CsbLoginPath, State(state), query(None))
            .await
            .expect_err("404 without config");

        assert!(matches!(err, AppError::GenericNotFound));
    }

    #[tokio::test]
    async fn login_redirect_is_not_found_without_github_config() {
        let state = crate::AppState::new_for_tests().await;

        let err = login_redirect(CsbLoginStartPath, State(state), CookieJar::new())
            .await
            .expect_err("404 without config");

        assert!(matches!(err, AppError::GenericNotFound));
    }

    #[tokio::test]
    async fn login_start_shows_github_button() {
        let state = crate::AppState::new_for_tests_with_config(
            crate::csb::login::test_support::github_test_config(),
        )
        .await;

        let response = login_start(CsbLoginPath, State(state), query(None))
            .await
            .expect("page");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // A link, not a form: a form post would be blocked by `form-action`
        // once it redirected to GitHub.
        assert!(body.contains("href=\"/csb/login/start\""));
        assert!(!body.contains("<form"));
        assert!(body.contains("GitHub"));
        assert!(!body.contains("alert-error"));
    }

    #[tokio::test]
    async fn login_start_shows_generic_error_when_flagged() {
        let state = crate::AppState::new_for_tests_with_config(
            crate::csb::login::test_support::github_test_config(),
        )
        .await;

        let response = login_start(CsbLoginPath, State(state), query(Some("github")))
            .await
            .expect("page");

        let body = response_body_string(response).await;
        assert!(body.contains("alert-error"));
    }
}
