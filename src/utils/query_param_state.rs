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
    /// Comma-separated, since a query string cannot carry a sequence.
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    ignored_columns: Option<String>,
}

/// Column names come from an upload; keep the `Location` header bounded.
const MAX_IGNORED_COLUMNS_LEN: usize = 200;

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

    pub fn ignored_columns(&self) -> Vec<&str> {
        self.ignored_columns
            .as_deref()
            .into_iter()
            .flat_map(|names| names.split(','))
            .filter(|name| !name.is_empty())
            .collect()
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

    /// Warnings for a successful import; only whole names that fit the cap are kept.
    pub fn import_warnings(capped: bool, ignored_columns: &[String]) -> Self {
        let mut joined = String::new();
        for name in ignored_columns {
            let next_len = joined.len() + name.len() + usize::from(!joined.is_empty());
            if next_len > MAX_IGNORED_COLUMNS_LEN {
                break;
            }
            if !joined.is_empty() {
                joined.push(',');
            }
            joined.push_str(name);
        }

        Self {
            import_capped: capped,
            ignored_columns: (!joined.is_empty()).then_some(joined),
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
    fn import_warnings_round_trip_through_query_string() {
        let names = ["geboortedatm".to_string(), "achter naam".to_string()];
        let state = QueryParamState::import_warnings(true, &names);
        let query = serde_urlencoded::to_string(&state).unwrap();
        assert_eq!(
            query,
            "import_capped=true&ignored_columns=geboortedatm%2Cachter+naam"
        );

        let parsed: QueryParamState = serde_urlencoded::from_str(&query).unwrap();
        assert!(parsed.is_import_capped());
        assert_eq!(
            parsed.ignored_columns(),
            vec!["geboortedatm", "achter naam"]
        );

        let none = QueryParamState::import_warnings(false, &[]);
        assert_eq!(serde_urlencoded::to_string(&none).unwrap(), "");
        assert!(none.ignored_columns().is_empty());
    }

    /// Only whole names fit under the cap; a single oversized name is dropped
    /// rather than cut in half.
    #[test]
    fn import_warnings_cap_the_column_list() {
        let names: Vec<String> = (0..100).map(|i| format!("column{i:03}")).collect();
        let state = QueryParamState::import_warnings(false, &names);
        let joined = state.ignored_columns.clone().unwrap();
        assert!(joined.len() <= MAX_IGNORED_COLUMNS_LEN);
        // 20 names of 9 characters plus 19 commas is 199; a 21st would not fit.
        assert_eq!(joined.len(), 199);
        assert!(joined.ends_with("column019"));
        assert_eq!(state.ignored_columns().len(), 20);

        let huge = vec!["x".repeat(MAX_IGNORED_COLUMNS_LEN + 1)];
        assert_eq!(
            QueryParamState::import_warnings(false, &huge).ignored_columns,
            None
        );
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
