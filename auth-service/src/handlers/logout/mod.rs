//! SP-initiated logout: building and sending the LogoutRequest (eID §7.7.1).
//!
//! The inbound half -- the SLS endpoint that receives the RD's LogoutResponse --
//! lives in `sls`, so the module the embedding application depends on for
//! [`handle_logout`] stays small.

mod sls;

pub(crate) use sls::handle_sls;

use crate::{
    bindings::http_post::autosubmit_post_response,
    error::Result,
    saml::messages::create_logout_request,
    state::{AuthServiceState, AuthState, LoggedOutSession},
    types::{EndpointUrl, MessageId, NameId, RedirectTarget},
};
use axum::{
    extract::FromRef,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;
use tracing::{debug, error, info, warn};

/// SP-initiated logout per eID §7.7.1 / §3.1.1.1, mounted by the embedding
/// application on a logout route of its choosing.
///
/// eID §3.1.1.1: only SP-initiated logout is supported. Asks the embedding
/// application to terminate its session (`AuthState::logout_session`) and, if
/// a session was active, builds a signed LogoutRequest (§7.7.1) for the
/// browser to POST to the RD's SLO endpoint. Otherwise redirects to
/// `post_logout_redirect`.
pub async fn handle_logout<S>(
    state: &S,
    jar: CookieJar,
    post_logout_redirect: &RedirectTarget,
) -> Response
where
    S: AuthState,
    AuthServiceState: FromRef<S>,
{
    let auth_state = AuthServiceState::from_ref(state);
    debug!("[logout] Handler entered, calling AuthState::logout_session");
    // The local session is torn down first; a failure there is the response.
    // The NameID is only present if one was recorded at login (needed to build
    // the LogoutRequest, eID §7.7.1).
    let (cleared_jar, ended) = match state.logout_session(jar).await {
        Ok(ended) => ended,
        Err(response) => return response,
    };
    let LoggedOutSession::WithSamlSubject(name_id) = ended else {
        debug!(
            "[logout] No SAML session to end ({ended:?}); completed local logout, redirecting to post-logout page"
        );
        return (cleared_jar, Redirect::to(post_logout_redirect.as_str())).into_response();
    };

    // SECURITY: never log `name_id` itself; it is a SAML TransientID linkable
    // to a specific authentication session (`NameId`'s `Debug` redacts it).
    debug!("[logout] Active SAML session terminated; {name_id:?}");

    // The local session is already gone; if the RD descriptor is not loaded we
    // cannot build a LogoutRequest (no SLO endpoint), but the user is logged out
    // locally, so complete the logout by redirecting home rather than erroring.
    let Some(rd) = auth_state.rd_metadata() else {
        warn!("[logout] RD metadata not loaded; completed local logout, skipping SAML SLO");
        return (cleared_jar, Redirect::to(post_logout_redirect.as_str())).into_response();
    };

    match build_slo_form(&name_id, &auth_state, &rd.slo_url) {
        Ok((request_id, form)) => {
            // eID §7.7.2: record the LogoutRequest ID so the matching
            // LogoutResponse's @InResponseTo can be validated and consumed once in
            // `handle_sls`. Reuses the outstanding-request store used for
            // AuthnRequest IDs (both are one-time IDs this DV issued); no extra
            // `AuthState` method is needed.
            state.register_pending_request(request_id).await;
            (cleared_jar, form).into_response()
        }
        Err(e) => {
            // The local session is already gone, so the user *is* logged out;
            // complete the logout by redirecting home rather than erroring.
            error!("[logout] Failed to start SAML SLO: {e}; skipping");
            (cleared_jar, Redirect::to(post_logout_redirect.as_str())).into_response()
        }
    }
}

/// Build the signed LogoutRequest (eID §7.7.1) and the autosubmit form that
/// POSTs it to the RD's SLO endpoint. Returns the request ID so the caller can
/// register it for the `InResponseTo` consume-once check (eID §7.7.2).
fn build_slo_form(
    name_id: &NameId,
    auth_state: &AuthServiceState,
    slo_url: &EndpointUrl,
) -> Result<(MessageId, Response)> {
    let cfg = auth_state.auth_config();
    debug!(
        "[logout] Building LogoutRequest: entity_id={}, slo_url={}",
        cfg.dv.entity_id, slo_url
    );
    let msg = create_logout_request(
        name_id,
        &cfg.dv.entity_id,
        slo_url,
        auth_state.dv_keys().primary_signing()?,
    )?;
    info!("[logout] LogoutRequest created: {}", msg.id.log_prefix());
    debug!("[logout] LogoutRequest XML built; returning autosubmit form");
    let form = autosubmit_post_response(slo_url, &msg.xml, "SAMLRequest")?;
    Ok((msg.id, form))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::test_support::MockAuthState;
    use axum::{Form, extract::State};
    use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
    use std::collections::HashMap;

    // --- handler early-exit branches (no RD metadata loaded) ---------------

    #[tokio::test]
    async fn logout_without_active_session_redirects_home() {
        let mock = MockAuthState::empty();
        let resp = handle_logout(&mock, CookieJar::new(), &RedirectTarget::root()).await;
        assert!(resp.status().is_redirection());
    }

    #[tokio::test]
    async fn logout_with_session_but_no_rd_metadata_completes_local_logout() {
        // A session exists (logout_session returns a NameID) but no RD descriptor
        // is loaded, so no SAML SLO is built: the browser is redirected home.
        let mut mock = MockAuthState::empty();
        mock.session = LoggedOutSession::WithSamlSubject(
            NameId::parse("_transient-name-id").expect("test NameID"),
        );
        let resp = handle_logout(&mock, CookieJar::new(), &RedirectTarget::root()).await;
        assert!(resp.status().is_redirection());
    }

    #[tokio::test]
    async fn sls_without_saml_response_redirects() {
        let mock = MockAuthState::empty();
        let resp = handle_sls(
            crate::SamlLogoutPath,
            State(mock.clone()),
            State(mock.auth.clone()),
            Form(HashMap::new()),
        )
        .await;
        assert!(resp.status().is_redirection());
    }

    #[tokio::test]
    async fn sls_with_invalid_base64_redirects() {
        // Malformed input is dropped like any other invalid LogoutResponse: the
        // local logout already happened, so the user is never stranded.
        let mock = MockAuthState::empty();
        let mut params = HashMap::new();
        params.insert("SAMLResponse".to_string(), "!!!not-base64!!!".to_string());
        let resp = handle_sls(
            crate::SamlLogoutPath,
            State(mock.clone()),
            State(mock.auth.clone()),
            Form(params),
        )
        .await;
        assert!(resp.status().is_redirection());
    }

    #[tokio::test]
    async fn sls_without_rd_metadata_redirects() {
        // Valid base64 body, but no RD descriptor to verify against: drop and
        // redirect (the local logout already happened in `handle_logout`).
        let mock = MockAuthState::empty();
        let mut params = HashMap::new();
        params.insert(
            "SAMLResponse".to_string(),
            BASE64.encode(b"<samlp:LogoutResponse/>"),
        );
        let resp = handle_sls(
            crate::SamlLogoutPath,
            State(mock.clone()),
            State(mock.auth.clone()),
            Form(params),
        )
        .await;
        assert!(resp.status().is_redirection());
    }
}
