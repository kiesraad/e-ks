//! HTTP handlers for the SP endpoints, one module per endpoint. `flow` is the
//! shared browser-binding of the SSO flow (login-CSRF defense), not a handler.

pub mod acs;
pub mod autosubmit;
pub mod flow;
pub mod login;
pub mod logout;
pub mod metadata;

/// Shared scaffolding for the handler unit tests: one [`AuthState`] mock
/// instead of a copy per handler module.
#[cfg(test)]
pub(crate) mod test_support {
    use crate::{
        saml::subject::SubjectId,
        state::{AuthFailure, AuthServiceState, AuthState, LoggedOutSession},
        types::{MessageId, NameId},
    };
    use axum::{
        extract::FromRef,
        http::{HeaderMap, StatusCode},
        response::{IntoResponse, Response},
    };
    use axum_extra::extract::CookieJar;

    /// Minimal [`AuthState`] wrapping an [`AuthServiceState`], used to drive the
    /// handlers directly without a router. Encodes each [`AuthFailure`] kind as
    /// a distinct status code so tests can assert on the failure path taken.
    #[derive(Clone)]
    pub(crate) struct MockAuthState {
        pub auth: AuthServiceState,
        /// What `logout_session` reports it tore down.
        pub session: LoggedOutSession,
    }

    impl MockAuthState {
        pub(crate) fn new(auth: AuthServiceState) -> Self {
            Self {
                auth,
                session: LoggedOutSession::None,
            }
        }

        pub(crate) fn empty() -> Self {
            Self::new(AuthServiceState::new_empty())
        }
    }

    impl FromRef<MockAuthState> for AuthServiceState {
        fn from_ref(m: &MockAuthState) -> Self {
            m.auth.clone()
        }
    }

    impl AuthState for MockAuthState {
        async fn on_authenticated(
            &self,
            _subject_id: SubjectId,
            _name_id: NameId,
            _jar: CookieJar,
            _headers: &HeaderMap,
        ) -> Response {
            StatusCode::OK.into_response()
        }

        async fn on_authentication_failed(
            &self,
            failure: AuthFailure,
            _jar: CookieJar,
            _headers: &HeaderMap,
            end_session: bool,
        ) -> Response {
            let status = match failure {
                AuthFailure::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
                AuthFailure::Cancelled => StatusCode::FORBIDDEN,
                AuthFailure::Error => StatusCode::UNAUTHORIZED,
            };
            // lets tests see the flag
            (status, [("x-test-end-session", end_session.to_string())]).into_response()
        }

        async fn logout_session(&self, jar: CookieJar) -> (CookieJar, LoggedOutSession) {
            (jar, self.session.clone())
        }

        async fn register_pending_request(&self, _id: MessageId) {}

        async fn consume_if_pending(&self, _id: MessageId) -> bool {
            false
        }
    }
}
