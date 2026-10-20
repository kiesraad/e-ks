//! The SLS endpoint: receiving and verifying the RD's LogoutResponse (eID §7.7.2).

use crate::{
    saml::{
        constants::NS_SAMLP,
        validation::validate_logout_response,
        verification::{ExpectedRoot, verify_xml_signature},
    },
    state::{AuthServiceState, AuthState},
};
use axum::{
    Form,
    extract::{FromRef, State},
    response::{IntoResponse, Redirect, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// POST /saml/sp/logout: receives the RD's LogoutResponse per eID §7.7.2.
///
/// Enforces the §7.7.2 mandatory checks as a gate on treating the response as a
/// genuine RD confirmation: it MUST be a `samlp:LogoutResponse`, MUST carry a
/// valid RD signature, MUST have an `@InResponseTo` matching a LogoutRequest this
/// DV issued (consumed once, so a replayed response is rejected), and SHOULD
/// report `Success`.
///
/// The user's local session was already terminated by [`super::handle_logout`] *before*
/// any LogoutResponse arrives, so this endpoint holds no session state. A
/// failed/forged/replayed response is therefore logged and dropped but never
/// strands the user: the browser is always redirected to the post-logout page.
pub async fn handle_sls<S>(
    _: crate::SamlLogoutPath,
    State(state): State<S>,
    State(auth_state): State<AuthServiceState>,
    Form(params): Form<HashMap<String, String>>,
) -> Response
where
    S: AuthState,
    AuthServiceState: FromRef<S>,
{
    debug!("[SLS] Handler entered, form fields: {}", params.len());
    process_logout_response(&state, &auth_state, &params).await;

    let target = &auth_state.auth_config().post_logout_redirect;
    debug!("[SLS] Redirecting browser to {target}");
    Redirect::to(target.as_str()).into_response()
}

/// Verify and correlate an incoming LogoutResponse (eID §7.7.2), logging the
/// outcome. Every failure is logged and dropped rather than surfaced: the local
/// session was already terminated in [`super::handle_logout`], so nothing hinges on
/// this confirmation and the caller always redirects to the post-logout page.
async fn process_logout_response<S: AuthState>(
    state: &S,
    auth_state: &AuthServiceState,
    params: &HashMap<String, String>,
) {
    let Some(saml_response) = decode_saml_response(params) else {
        return;
    };
    info!("[SLS] LogoutResponse received");

    // Signature verification is CPU-bound and runs before any authentication
    // of the caller, so it is kept off the async workers.
    let verify_state = auth_state.clone();
    let verified =
        tokio::task::spawn_blocking(move || verified_logout_fields(&verify_state, &saml_response))
            .await;
    let Ok(Some(fields)) = verified else {
        if verified.is_err() {
            warn!("[SLS] LogoutResponse verification task failed; ignoring");
        }
        return;
    };

    // eID §7.7.2: @InResponseTo MUST match a LogoutRequest this DV issued. Consume
    // it once (only after the signature verified, so a forged response cannot burn
    // a pending ID) to reject replays of a captured LogoutResponse.
    if !state.consume_if_pending(fields.in_response_to).await {
        warn!(
            "[SLS] LogoutResponse InResponseTo did not match an outstanding LogoutRequest \
             (unknown, expired, or replayed); ignoring"
        );
        return;
    }

    // eID §7.7.2: report the status. A non-Success status does not change the
    // outcome (the local logout already completed) but is surfaced for diagnostics.
    if fields.status_is_success {
        info!("[SLS] LogoutResponse confirmed by RD (signed, correlated, status Success)");
    } else {
        warn!("[SLS] LogoutResponse status not Success; local logout already completed");
    }
}

/// Verify the RD signature over the LogoutResponse and lift out its §7.7.2
/// correlation fields, or `None` (logged) when it fails either gate.
fn verified_logout_fields(
    auth_state: &AuthServiceState,
    saml_response: &str,
) -> Option<crate::saml::validation::LogoutResponseFields> {
    // Without the RD descriptor we cannot verify the signature; the user is
    // already logged out locally, so complete the logout rather than erroring.
    let Some(rd) = auth_state.rd_metadata() else {
        warn!("[SLS] RD metadata not loaded; cannot verify LogoutResponse, redirecting");
        return None;
    };

    // eID §7.7.2: the LogoutResponse MUST carry a valid RD signature.
    // `validate_logout_response` re-parses these bytes, so both parses must agree
    // on which element is verified and read.
    let expected_root = ExpectedRoot {
        namespace: NS_SAMLP,
        local_name: "LogoutResponse",
        id: None,
    };
    let sig_result = verify_xml_signature(saml_response, &rd.signing_keys, &expected_root);
    if !sig_result.is_valid() {
        warn!(
            "[SLS] LogoutResponse signature invalid ({:?}); ignoring (local logout already done)",
            sig_result.errors
        );
        return None;
    }

    // eID §7.7.2: bind the (signature-verified) response to the RD and our SLS
    // endpoint and lift out the correlation fields.
    let cfg = auth_state.auth_config();
    match validate_logout_response(saml_response, &rd.entity_id, &cfg.dv.slo_url) {
        Ok(fields) => Some(fields),
        Err(reason) => {
            warn!("[SLS] {reason}; ignoring");
            None
        }
    }
}

/// The base64-decoded `SAMLResponse` form field, or `None` (logged) when absent
/// or undecodable. Like every malformed-LogoutResponse case: log, drop, and
/// never strand the user (the local logout already happened).
fn decode_saml_response(params: &HashMap<String, String>) -> Option<String> {
    let Some(encoded) = params.get("SAMLResponse") else {
        // POST without a SAMLResponse field: nothing to verify; accept.
        info!("[SLS] LogoutResponse received without SAMLResponse");
        return None;
    };
    match BASE64.decode(encoded.as_bytes()) {
        Ok(bytes) => {
            debug!("[SLS] SAMLResponse base64 decoded ({} bytes)", bytes.len());
            Some(String::from_utf8_lossy(&bytes).to_string())
        }
        Err(e) => {
            warn!("[SLS] Invalid SAMLResponse base64: {e}; ignoring");
            None
        }
    }
}
