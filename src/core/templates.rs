//! Askama template wrapper for Axum responses.
//! Used by handlers to render templates with a request-scoped values object.

use askama::Template;
use axum::{
    http::{HeaderValue, header},
    response::{Html, IntoResponse, Response},
};

use crate::{AppError, Locale, Session, SessionExpiry};

/// Values for pages rendered with only a locale (no `Context`).
pub struct LocaleValues {
    pub locale: Locale,
}

impl askama::Values for LocaleValues {
    fn get_value<'a>(&'a self, key: &str) -> Option<&'a dyn std::any::Any> {
        match key {
            "locale" => Some(&self.locale as &dyn std::any::Any),
            _ => None,
        }
    }
}

/// Values for session-backed pages rendered without a store `Context`:
/// locale, the token the `csrf_field` macro reads, and the remaining session
/// lifetime the expiry-warning component renders.
pub struct SessionPageValues {
    pub locale: Locale,
    pub csrf_token: String,
    pub session_expiry: SessionExpiry,
}

impl SessionPageValues {
    pub fn new(session: &Session) -> Self {
        Self {
            locale: session.locale,
            csrf_token: session.csrf_token().0.clone(),
            session_expiry: session.expiry(),
        }
    }
}

impl askama::Values for SessionPageValues {
    fn get_value<'a>(&'a self, key: &str) -> Option<&'a dyn std::any::Any> {
        match key {
            "locale" => Some(&self.locale as &dyn std::any::Any),
            "csrf_token" => Some(&self.csrf_token as &dyn std::any::Any),
            "session_expiry" => Some(&self.session_expiry as &dyn std::any::Any),
            _ => None,
        }
    }
}

/// Askama template wrapper that renders template `T` with a values object `V`
/// (typically the request `Context`), which must implement [`askama::Values`].
pub struct HtmlTemplate<T, V>(pub T, pub V);

impl<T, V> IntoResponse for HtmlTemplate<T, V>
where
    T: Template,
    V: askama::Values,
{
    fn into_response(self) -> Response {
        match self.0.render_with_values(&self.1) {
            Ok(html) => {
                let mut response = Html(html).into_response();
                response
                    .headers_mut()
                    .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
                response
            }
            Err(err) => AppError::TemplateError(err).into_response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use askama::Template;
    use axum::http::StatusCode;

    mod filters {
        #[askama::filter_fn]
        pub fn foo(_val: u32, _values: &dyn askama::Values) -> askama::Result<&'static str> {
            Err(askama::Error::Fmt)
        }
    }

    #[derive(Template)]
    #[template(source = "{{ 123|foo }}", ext = "txt")]
    struct MyTemplate;

    struct NoValues;
    impl askama::Values for NoValues {
        fn get_value<'a>(&'a self, _key: &str) -> Option<&'a dyn std::any::Any> {
            None
        }
    }

    #[tokio::test]
    async fn html_template_returns_500_when_render_fails() {
        let response = HtmlTemplate(MyTemplate, NoValues).into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
