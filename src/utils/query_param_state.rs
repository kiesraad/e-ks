//! Query parameter state for UI feedback and highlighting.
use axum::response::{IntoResponse, Redirect, Response};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Default, Serialize, Deserialize)]
pub struct QueryParamState {
    #[serde(default)]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    initial: bool,
    #[serde(default)]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    highlight: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    highlight_last: Option<usize>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    redirect_to: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    overlay: bool,
    #[serde(default)]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    max_candidates_reached: bool,
    #[serde(default)]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    import_capped: bool,
}

impl QueryParamState {
    pub fn is_initial(&self) -> bool {
        self.initial
    }

    pub fn should_warn(&self) -> bool {
        !self.initial
    }

    pub fn is_max_candidates_reached(&self) -> bool {
        self.max_candidates_reached
    }

    pub fn is_import_capped(&self) -> bool {
        self.import_capped
    }

    pub fn initial() -> Self {
        Self {
            initial: true,
            ..Default::default()
        }
    }

    pub fn created() -> Self {
        Self {
            initial: true,
            success: true,
            ..Default::default()
        }
    }

    pub fn success() -> Self {
        Self {
            success: true,
            ..Default::default()
        }
    }

    pub fn highlight(id: Uuid) -> Self {
        Self {
            highlight: Some(id),
            ..Default::default()
        }
    }

    pub fn highlight_success(id: Uuid) -> Self {
        Self {
            success: true,
            highlight: Some(id),
            ..Default::default()
        }
    }

    pub fn highlight_last(last: usize) -> Self {
        Self {
            highlight_last: Some(last),
            ..Default::default()
        }
    }

    pub fn highlight_last_success(last: usize) -> Self {
        Self {
            success: true,
            highlight_last: Some(last),
            ..Default::default()
        }
    }

    pub fn max_candidates_reached() -> Self {
        Self {
            max_candidates_reached: true,
            ..Default::default()
        }
    }

    pub fn import_capped() -> Self {
        Self {
            import_capped: true,
            ..Default::default()
        }
    }

    pub fn redirect_to(url: String) -> Self {
        Self {
            redirect_to: Some(url),
            ..Default::default()
        }
    }

    /// Query params for links between pages of an already-open overlay:
    /// `overlay=true` suppresses the open animation on the target page.
    pub fn overlay(redirect_to: Option<String>, initial: bool) -> Self {
        Self {
            overlay: true,
            redirect_to,
            initial,
            ..Default::default()
        }
    }

    pub fn redirect_url(&self) -> Option<&str> {
        self.redirect_to.as_deref().filter(|url| is_local_path(url))
    }

    /// Builds the redirect URL: the `redirect_to` query param if present (and a
    /// valid relative path), otherwise the default path with success query params.
    fn redirect_url_or(&self, default: impl std::fmt::Display) -> String {
        let mut url = self
            .redirect_url()
            .map_or_else(|| default.to_string(), str::to_string);

        if !url.contains('?') {
            url.push_str("?&success=true");
        }

        // Keep the overlay marker across the redirect so a save that lands on
        // another overlay page does not replay the open animation.
        if self.overlay && !url.contains("overlay=") {
            url.push_str("&overlay=true");
        }

        url
    }

    /// Redirect to `redirect_to` query param if present (and a valid relative path),
    /// otherwise redirect to the default path with success query params.
    pub fn redirect_or(&self, default: impl std::fmt::Display) -> Response {
        Redirect::to(&self.redirect_url_or(default)).into_response()
    }

    /// Like `redirect_or`, highlighting `id` on the target page.
    pub fn redirect_or_highlighting(&self, default: impl std::fmt::Display, id: Uuid) -> Response {
        // `redirect_url_or` always yields a query string
        let url = format!("{}&highlight={id}", self.redirect_url_or(default));
        Redirect::to(&url).into_response()
    }

    /// Like `redirect_or`, but preserves `initial=true` in the redirect URL when set.
    /// Use this for inter-step saves within the general information section.
    pub fn redirect_or_preserving_initial(&self, default: impl std::fmt::Display) -> Response {
        let mut url = self.redirect_url_or(default);

        if self.initial && !url.contains("initial=") {
            if url.contains('?') {
                url.push_str("&initial=true");
            } else {
                url.push_str("?initial=true");
            }
        }

        Redirect::to(&url).into_response()
    }
}

/// Accept only local absolute paths as redirect targets: browsers resolve a
/// leading `//` (or `/\`) as a protocol-relative URL to another host, and
/// control characters could split the `Location` header.
pub(crate) fn is_local_path(url: &str) -> bool {
    url.starts_with('/')
        && !url.starts_with("//")
        && !url.contains('\\')
        && !url.bytes().any(|byte| byte.is_ascii_control())
}

#[cfg(test)]
mod tests {
    use axum::http::header::LOCATION;

    use super::*;

    fn location(response: axum::response::Response) -> String {
        response
            .headers()
            .get(LOCATION)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn redirect_or_preserving_initial_does_not_duplicate_initial_param() {
        // redirect_to already carries initial=true
        let state = QueryParamState {
            initial: true,
            redirect_to: Some("/foo?initial=true".to_string()),
            ..Default::default()
        };
        let loc = location(state.redirect_or_preserving_initial("/fallback"));
        assert_eq!(loc, "/foo?initial=true");

        // default path already carries initial=true
        let state = QueryParamState::initial();
        let loc = location(state.redirect_or_preserving_initial("/bar?initial=true"));
        assert_eq!(loc, "/bar?initial=true");
    }

    #[test]
    fn redirect_rejects_non_local_targets() {
        for evil in [
            "https://evil.example",
            "//evil.example",
            "//evil.example/path",
            "/\\evil.example",
            "/foo\\bar",
            "/foo\rSet-Cookie: x",
            "foo",
            "",
        ] {
            let state = QueryParamState::redirect_to(evil.to_string());
            assert_eq!(state.redirect_url(), None, "{evil:?} must be rejected");
            assert_eq!(
                location(state.redirect_or("/fallback")),
                "/fallback?&success=true",
                "{evil:?} must fall back to the default"
            );
        }

        let state = QueryParamState::redirect_to("/safe/path?x=1".to_string());
        assert_eq!(state.redirect_url(), Some("/safe/path?x=1"));
    }

    #[test]
    fn redirect_or_propagates_overlay() {
        // redirect_to target gets the overlay marker appended
        let state = QueryParamState {
            overlay: true,
            redirect_to: Some("/foo?bar=1".to_string()),
            ..Default::default()
        };
        assert_eq!(
            location(state.redirect_or("/fallback")),
            "/foo?bar=1&overlay=true"
        );

        // but not duplicated when already present
        let state = QueryParamState {
            overlay: true,
            redirect_to: Some("/foo?overlay=true".to_string()),
            ..Default::default()
        };
        assert_eq!(
            location(state.redirect_or("/fallback")),
            "/foo?overlay=true"
        );

        // without the flag nothing is appended
        let state = QueryParamState::default();
        assert_eq!(
            location(state.redirect_or("/fallback")),
            "/fallback?&success=true"
        );
    }
}
