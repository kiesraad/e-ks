//! Request-scoped template context carrying locale and helpers.
//! Extracted from requests and passed into Askama templates.

use axum::{extract::FromRequestParts, http::request::Parts};

use crate::{AppError, AppRequestState, ElectionConfig, Locale, PgStore, Session, SessionExpiry};

/// Route prefixes on which the shared layout shows the "documents were already
/// downloaded" warning, and the CSB route that leaves paper-corrections mode.
///
/// `pg` and `csb` each carry a test asserting their typed paths still match
/// these.
pub(crate) const DOWNLOAD_WARNING_PREFIXES: [&str; 3] =
    ["/political-group", "/candidate-lists", "/persons"];
pub(crate) const CSB_PAPER_CORRECTIONS_STOP_PREFIX: &str = "/csb/examination";

/// State for CSB paper-corrections mode.
#[derive(Clone)]
pub struct PaperCorrectionMode {
    /// URL that leaves paper-corrections mode.
    pub exit_path: String,
    /// Appellation of the political group being corrected,
    /// shown in the corrections banner.
    pub group_name: String,
}

/// Request-scoped template context used by Askama.
#[derive(Clone)]
pub struct Context {
    /// Election configuration for this stream.
    pub election: ElectionConfig,
    /// Maximum number of candidates allowed for this political group.
    pub max_candidates: usize,
    /// Hard cap on the number of candidates a list may hold. Unlimited
    /// (`usize::MAX`) while correcting paper documents.
    pub candidate_limit: usize,
    /// Multiple candidate lists present
    pub multiple_candidate_lists: bool,
    /// Whether to show the success alert based on the request query.
    pub show_success_alert: bool,
    /// Whether to show a warning that documents were downloaded and changes won't be reflected.
    pub show_download_warning: bool,
    /// Whether the page is part of an already-open overlay (suppresses animation).
    pub overlay_active: bool,
    /// Session data for locale and CSRF.
    pub session: Session,
    /// Remaining session lifetime at render time, for the expiry warning.
    pub session_expiry: SessionExpiry,
    /// Short identifier of the server this instance runs on (e.g. "S1"),
    /// rendered next to the version in the layout footer when set.
    pub server_name: Option<&'static str>,
    /// URL for the "General information" nav link. Includes `initial=true` when
    /// general information is still empty, so the first-visit flow suppresses warnings.
    pub general_information_path: String,
    /// Set when a CSB session is correcting a stream's paper documents.
    pub paper_correction_mode: Option<PaperCorrectionMode>,
}

impl Context {
    pub fn new(store: &PgStore, session: Session) -> Self {
        let election = store.get_election();
        let political_group = store.get_political_group();
        let max_candidates = political_group.get_max_candidates();
        let candidate_limit = store.candidate_limit();
        let multiple_candidate_lists = store.get_candidate_list_count() > 1;

        let general_information_path = political_group.general_information_path(store);

        let paper_correction_mode =
            store
                .paper_corrections_stream_id()
                .map(|stream_id| PaperCorrectionMode {
                    exit_path: format!(
                        "{CSB_PAPER_CORRECTIONS_STOP_PREFIX}/{stream_id}/paper-corrections/stop"
                    ),
                    group_name: political_group
                        .csb_appellation(store.get_first_candidate_name().as_ref()),
                });

        Self {
            election,
            max_candidates,
            candidate_limit,
            multiple_candidate_lists,
            show_success_alert: false,
            show_download_warning: false,
            overlay_active: false,
            session_expiry: session.expiry(),
            session,
            server_name: None,
            general_information_path,
            paper_correction_mode,
        }
    }

    #[cfg(test)]
    pub fn new_test_without_db() -> Self {
        let store = PgStore::new_for_test();
        Self::new(&store, Session::new_test_with_locale(Locale::En))
    }

    #[cfg(test)]
    pub fn new_test_from_store(store: &PgStore) -> Self {
        Self::new(store, Session::new_test_with_locale(Locale::En))
    }

    pub fn livereload_enabled() -> bool {
        cfg!(feature = "livereload")
    }
}

impl askama::Values for Context {
    fn get_value<'a>(&'a self, key: &str) -> Option<&'a dyn std::any::Any> {
        match key {
            "locale" => Some(&self.session.locale as &dyn std::any::Any),
            "csrf_token" => Some(&self.session.csrf_token().0 as &dyn std::any::Any),
            "session_expiry" => Some(&self.session_expiry as &dyn std::any::Any),
            "election" => Some(&self.election as &dyn std::any::Any),
            "max_candidates" => Some(&self.max_candidates as &dyn std::any::Any),
            "candidate_limit" => Some(&self.candidate_limit as &dyn std::any::Any),
            "show_success_alert" => Some(&self.show_success_alert as &dyn std::any::Any),
            "show_download_warning" => Some(&self.show_download_warning as &dyn std::any::Any),
            "multiple_candidate_lists" => {
                Some(&self.multiple_candidate_lists as &dyn std::any::Any)
            }
            "overlay_active" => Some(&self.overlay_active as &dyn std::any::Any),
            "server_name" => Some(&self.server_name as &dyn std::any::Any),
            "general_information_path" => {
                Some(&self.general_information_path as &dyn std::any::Any)
            }
            // `is_some()` yields a temporary, so borrow promoted constants instead
            "paper_correction_mode" => Some(if self.paper_correction_mode.is_some() {
                &true
            } else {
                &false
            }),
            "paper_corrections_exit_path" => self
                .paper_correction_mode
                .as_ref()
                .map(|mode| &mode.exit_path as &dyn std::any::Any),
            "paper_corrections_group_name" => self
                .paper_correction_mode
                .as_ref()
                .map(|mode| &mode.group_name as &dyn std::any::Any),
            _ => None,
        }
    }
}

impl<S: AppRequestState> FromRequestParts<S> for Context {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let session = Session::from_request_parts(parts, state).await?;
        let store = PgStore::from_request_parts(parts, state).await?;
        let mut context = Context::new(&store, session);

        context.server_name = state.config().server_name.as_deref();

        let path = parts.uri.path();
        context.show_download_warning = store.should_show_download_warning()
            && DOWNLOAD_WARNING_PREFIXES
                .iter()
                .any(|prefix| path.starts_with(prefix));

        context.show_success_alert = crate::success_alert_requested(parts);
        context.overlay_active = crate::overlay_active(parts);

        Ok(context)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn new_context_sets_locale() {
        let context = Context::new_test_without_db();
        assert_eq!(context.session.locale, Locale::En);
    }

    #[test]
    fn livereload_flag_matches_feature() {
        assert_eq!(Context::livereload_enabled(), cfg!(feature = "livereload"));
    }
}
