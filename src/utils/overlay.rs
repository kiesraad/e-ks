use std::fmt;

use axum_extra::routing::TypedPath;

use super::QueryParamState;

/// Navigation context for an overlay, carrying an optional `redirect_to` URL
#[derive(Default)]
pub struct Overlay {
    redirect_to: Option<String>,
    initial: bool,
}

impl Overlay {
    pub fn new(query: &QueryParamState) -> Self {
        Self {
            redirect_to: query.redirect_url().map(str::to_string),
            initial: query.is_initial(),
        }
    }

    /// Returns `redirect_to` if set, otherwise the given default path, keeping
    /// `initial=true` across the close just like a save would, so closing
    /// without saving does not lose the initial-data-entry state
    pub fn close_url(&self, default: impl fmt::Display) -> String {
        let mut url = self
            .redirect_to
            .clone()
            .unwrap_or_else(|| default.to_string());

        if self.initial && !url.contains("initial=") {
            url.push_str(if url.contains('?') {
                "&initial=true"
            } else {
                "?initial=true"
            });
        }

        url
    }

    /// Returns `path` with `overlay=true` appended (the target is another page
    /// of the already-open overlay, so it skips the open animation), while
    /// preserving `redirect_to` and `initial=true` query params when set,
    /// so the target step can return to the right place after saving
    pub fn forward(&self, path: impl TypedPath) -> String {
        path.with_query_params(QueryParamState::overlay(
            self.redirect_to.clone(),
            self.initial,
        ))
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use axum_extra::routing::TypedPath;

    use super::*;

    #[derive(TypedPath)]
    #[typed_path("/foo")]
    struct FooPath;

    #[test]
    fn forward_preserves_initial() {
        let query = QueryParamState::initial();
        let overlay = Overlay::new(&query);

        assert_eq!(overlay.forward(FooPath), "/foo?&initial=true&overlay=true");
    }

    #[test]
    fn forward_without_initial_does_not_add_it() {
        let query = QueryParamState::default();
        let overlay = Overlay::new(&query);

        assert_eq!(overlay.forward(FooPath), "/foo?&overlay=true");
    }

    #[test]
    fn close_url_preserves_initial() {
        let query = QueryParamState::initial();
        let overlay = Overlay::new(&query);

        assert_eq!(overlay.close_url("/persons"), "/persons?initial=true");
        assert_eq!(
            overlay.close_url("/persons?highlight=1"),
            "/persons?highlight=1&initial=true"
        );
        assert_eq!(
            overlay.close_url("/persons?initial=true"),
            "/persons?initial=true"
        );
    }

    #[test]
    fn close_url_without_initial() {
        let query = QueryParamState::default();
        let overlay = Overlay::new(&query);

        assert_eq!(overlay.close_url("/persons"), "/persons");
    }

    #[test]
    fn close_url_redirect_preserves_initial() {
        let query: QueryParamState =
            serde_urlencoded::from_str("initial=true&redirect_to=%2Ffoo").expect("query params");
        let overlay = Overlay::new(&query);

        assert_eq!(overlay.close_url("/persons"), "/foo?initial=true");
    }

    #[test]
    fn close_url_redirect_without_initial() {
        let query: QueryParamState = QueryParamState::redirect_to("/foo".into());
        let overlay = Overlay::new(&query);

        assert_eq!(overlay.close_url("/persons"), "/foo");
    }
}
