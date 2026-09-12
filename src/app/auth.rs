//! The `AuthState` hooks the auth-service invokes on login and logout.

use auth_service::{AuthFailure, AuthState, LoggedOutSession, MessageId, NameId, SubjectId};
use axum::{
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;

use tracing::{error, info, warn};

use crate::{
    AppError, AppState, CsbMainAction, Locale, PgEvent, PgStore, Session, SessionUser, StreamId,
    auth::session_extractor::{
        SESSION_COOKIE_NAME, build_removal_cookie, establish_session, user_agent_hash,
    },
    common::{PgIndexPath, SelectElectionPath, auth_failure_response},
};

impl AppState {
    /// If the stream already has data for some election, prime its store,
    /// record the login, and return the election to attach to the session.
    async fn existing_election_login(
        &self,
        stream_id: StreamId,
    ) -> Result<Option<crate::ElectionConfig>, AppError> {
        let Some(election) = self
            .existing_elections_for_stream(stream_id)
            .await
            .ok()
            .and_then(|list| list.into_iter().next())
        else {
            return Ok(None);
        };
        let store = self.store_for_stream(stream_id, election, false).await?;
        // the cached store can lag other instances; catch up before snapshotting
        store.load().await?;
        PgStore::own(store)
            .with_limits(self.config.rate_limits)
            .update(PgEvent::Login)
            .await?;
        Ok(Some(election))
    }

    /// Record the logout in the audit log of the session's stream: the shared
    /// CSB main stream for committee sessions, the PG stream otherwise. A
    /// political-group session without an election has no partition to write
    /// to, and the log is the only record.
    async fn record_logout(&self, session: &Session) {
        let recorded = match &session.user {
            SessionUser::CentralElectoralCommittee { user, election, .. } => {
                match self.csb_main_store(*election).await {
                    Ok(store) => store.update(CsbMainAction::Logout.by(user.clone())).await,
                    Err(err) => Err(err),
                }
            }
            SessionUser::PoliticalGroup {
                stream_id,
                election: Some(election),
                ..
            } => match self.store_for_stream(*stream_id, *election, false).await {
                Ok(store) => match store.load().await {
                    // catch up before snapshotting, as in the login above
                    Ok(()) => {
                        PgStore::own(store)
                            .with_limits(self.config.rate_limits)
                            .update(PgEvent::Logout)
                            .await
                    }
                    Err(err) => Err(err),
                },
                Err(err) => Err(err),
            },
            SessionUser::PoliticalGroup { election: None, .. } => return,
        };

        if let Err(err) = recorded {
            error!("failed to record logout: {err}");
        }
    }

    /// Remove the current session (if any) from the store and return a jar with
    /// the session cookie cleared on the client. Used when an authentication
    /// attempt fails so no stale session survives (TVS L10).
    async fn clear_session_cookie(&self, jar: CookieJar) -> CookieJar {
        if let Some(token) = jar.get(SESSION_COOKIE_NAME).map(|c| c.value().to_string()) {
            self.sessions.remove(&token).await;
        }
        jar.remove(build_removal_cookie())
    }
}

impl AuthState for AppState {
    async fn on_authenticated(
        &self,
        subject_id: SubjectId,
        name_id: NameId,
        jar: CookieJar,
        headers: &HeaderMap,
    ) -> Response {
        let id_code = subject_id.value;
        let stream_id = self.id_deriver.derive_stream_id(&id_code);

        let (election, redirect_to) = match self.existing_election_login(stream_id).await {
            Ok(Some(election)) => (Some(election), PgIndexPath.to_string()),
            Ok(None) => (None, SelectElectionPath.to_string()),
            Err(err) => return err.into_response(),
        };

        let mut session = Session::for_political_group(
            stream_id,
            name_id.into_string(),
            election,
            Locale::from_headers(headers),
        );
        session.set_user_agent_hash(user_agent_hash(headers));

        info!(
            event = "auth.login",
            stream_id = %stream_id,
            scope = session.scope().as_str(),
            "user authenticated"
        );

        (
            establish_session(&self.sessions, jar, session).await,
            Redirect::to(&redirect_to),
        )
            .into_response()
    }

    async fn logout_session(&self, jar: CookieJar) -> (CookieJar, LoggedOutSession) {
        // Drop the session (if any) and always clear the cookie.
        let ended = match jar.get(SESSION_COOKIE_NAME).map(|c| c.value().to_string()) {
            Some(token) => match self.sessions.remove(&token).await {
                Some(session) => {
                    self.record_logout(&session).await;
                    info!(
                        event = "auth.logout",
                        user = ?session.user,
                        "user signed out"
                    );
                    ended_session(session.user)
                }
                None => LoggedOutSession::None,
            },
            None => LoggedOutSession::None,
        };
        (jar.remove(build_removal_cookie()), ended)
    }

    async fn on_authentication_failed(
        &self,
        failure: AuthFailure,
        jar: CookieJar,
        headers: &HeaderMap,
        end_session: bool,
    ) -> Response {
        // TVS L10, only for a flow this browser started: a bare link to the
        // error page must not log anyone out.
        let jar = if end_session {
            self.clear_session_cookie(jar).await
        } else {
            jar
        };
        let locale = Locale::from_headers(headers);

        // No session and no stream, so the log is the only place this can land.
        warn!(
            event = "auth.failed",
            reason = ?failure,
            "authentication failed"
        );

        (jar, auth_failure_response(failure, locale)).into_response()
    }

    async fn register_pending_request(&self, id: MessageId) {
        self.pending_requests.register(id.into_string()).await;
    }

    async fn consume_if_pending(&self, id: MessageId) -> bool {
        self.pending_requests.consume_if_pending(id.as_str()).await
    }
}

/// Whether the ended session had a SAML identity to log out at the RD.
///
/// Only political-group sessions take part in SAML, and even one of those has no
/// NameID when it came from the dev-login bypass: both end locally without an
/// SLO round-trip.
fn ended_session(user: SessionUser) -> LoggedOutSession {
    match user {
        SessionUser::PoliticalGroup { saml_name_id, .. } => match NameId::parse(saml_name_id) {
            Ok(name_id) => LoggedOutSession::WithSamlSubject(name_id),
            Err(_) => LoggedOutSession::WithoutSamlSubject,
        },
        SessionUser::CentralElectoralCommittee { .. } => LoggedOutSession::WithoutSamlSubject,
    }
}
