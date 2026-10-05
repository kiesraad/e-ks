//! Request-scoped template context for the CSB domain, carrying locale and
//! helpers. Extracted from requests and passed into Askama templates.

use axum::{extract::FromRequestParts, http::request::Parts};

use crate::{AppError, AppRequestState, CsbUser, ElectionConfig, Session};

#[cfg(test)]
use crate::Locale;

/// Request-scoped template context used by CSB Askama templates.
#[derive(Clone)]
pub struct CsbContext {
    /// Election the session is currently working on. Every CSB route sits
    /// behind `csb_store_middleware`, which guarantees the session has picked
    /// an election, so pages and templates can rely on it being present.
    pub election: ElectionConfig,
    /// Session data for locale and CSRF.
    pub session: Session,
    /// Short identifier of the server this instance runs on (e.g. "S1"),
    /// rendered next to the version in the layout footer when set.
    pub server_name: Option<&'static str>,
    /// Whether to show the success alert based on the request query.
    pub show_success_alert: bool,
    /// Whether the page is part of an already-open overlay (suppresses animation).
    pub overlay_active: bool,
    /// Whether this deployment has the passkey login, so the layout can
    /// offer the passkey management page.
    pub passkeys_enabled: bool,
}

impl CsbContext {
    pub fn new(session: Session, election: ElectionConfig) -> Self {
        Self {
            election,
            session,
            server_name: None,
            show_success_alert: false,
            overlay_active: false,
            passkeys_enabled: false,
        }
    }

    /// The committee member behind this request, recorded on CSB events.
    pub fn user(&self) -> Result<CsbUser, AppError> {
        self.session.require_csb_user()
    }

    #[cfg(test)]
    pub fn new_test() -> Self {
        let session = Session::for_committee(CsbUser::new_test(), ElectionConfig::EK27, Locale::En);
        Self::new(session, ElectionConfig::EK27)
    }
}

impl askama::Values for CsbContext {
    fn get_value<'a>(&'a self, key: &str) -> Option<&'a dyn std::any::Any> {
        match key {
            "election" => Some(&self.election as &dyn std::any::Any),
            "locale" => Some(&self.session.locale as &dyn std::any::Any),
            "csrf_token" => Some(&self.session.csrf_token().0 as &dyn std::any::Any),
            "server_name" => Some(&self.server_name as &dyn std::any::Any),
            "show_success_alert" => Some(&self.show_success_alert as &dyn std::any::Any),
            "overlay_active" => Some(&self.overlay_active as &dyn std::any::Any),
            "passkeys_enabled" => Some(&self.passkeys_enabled as &dyn std::any::Any),
            _ => None,
        }
    }
}

impl<S: AppRequestState> FromRequestParts<S> for CsbContext {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let session = Session::from_request_parts(parts, state).await?;
        let election = session.require_current_election()?;
        let mut context = CsbContext::new(session, election);

        context.server_name = state.config().server_name.as_deref();
        context.passkeys_enabled = state.passkeys().is_some();

        context.show_success_alert = crate::success_alert_requested(parts);
        context.overlay_active = crate::overlay_active(parts);

        Ok(context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_context_carries_session_locale_and_election() {
        let context = CsbContext::new_test();
        assert_eq!(context.session.locale, Locale::En);
        assert_eq!(context.election, crate::ElectionConfig::EK27);
        assert_eq!(context.session.user.election(), Some(context.election));
    }
}
