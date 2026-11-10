//! The SLS endpoint: receiving and verifying the RD's LogoutResponse (eID §7.7.2).

use crate::{
    saml::{
        constants::NS_SAMLP,
        validation::{LogoutResponseFields, validate_logout_response},
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

    // Validation and signature verification are CPU-bound and run before any
    // authentication of the caller, so they are kept off the async workers.
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

/// Run the §7.7.2 structural checks, then verify the RD signature, and lift out
/// the correlation fields, or `None` (logged) when it fails either gate.
///
/// Cheapest first: this endpoint is unauthenticated, so a response that fails
/// the structural checks is dropped without any signature work. Nothing read
/// from the unverified parse is acted on until the signature has verified.
fn verified_logout_fields(
    auth_state: &AuthServiceState,
    saml_response: &str,
) -> Option<LogoutResponseFields> {
    // Without the RD descriptor we cannot verify the signature; the user is
    // already logged out locally, so complete the logout rather than erroring.
    let Some(rd) = auth_state.rd_metadata() else {
        warn!("[SLS] RD metadata not loaded; cannot verify LogoutResponse, redirecting");
        return None;
    };

    // eID §7.7.2: bind the response to the RD and our SLS endpoint and lift out
    // the correlation fields.
    let cfg = auth_state.auth_config();
    let fields = match validate_logout_response(saml_response, &rd.entity_id, &cfg.dv.slo_url) {
        Ok(fields) => fields,
        Err(reason) => {
            warn!("[SLS] {reason}; ignoring");
            return None;
        }
    };

    // eID §7.7.2: the LogoutResponse MUST carry a valid RD signature. The
    // verifier re-parses these bytes; the root ID binds both parses to the
    // same element (see `ExpectedRoot`).
    let expected_root = ExpectedRoot {
        namespace: NS_SAMLP,
        local_name: "LogoutResponse",
        id: Some(fields.id.as_str()),
    };
    let sig_result = verify_xml_signature(saml_response, &rd.signing_keys, &expected_root);
    if !sig_result.is_valid() {
        warn!(
            "[SLS] LogoutResponse signature invalid ({:?}); ignoring (local logout already done)",
            sig_result.errors
        );
        return None;
    }

    Some(fields)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::AuthConfig,
        handlers::test_support::MockAuthState,
        keys::{CertificateBase64, CertificatePem, KeyPair, PrivateKeyPem, key_pair_paths},
        saml::{
            constants::{NS_SAML, STATUS_SUCCESS},
            idp_metadata::IdpMetadata,
        },
        types::{EndpointUrl, EntityId},
    };
    use std::path::PathBuf;

    const SLS: &str = "https://dv.example.com/saml/sp/logout";
    const RD: &str = "urn:test:rd";
    const REQUEST_ID: &str = "_logout-request-1";
    const RESPONSE_ID: &str = "_logout-response-1";

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
    }

    fn load_key(name: &str) -> KeyPair {
        let paths = key_pair_paths(&fixtures_dir(), name);
        let read = |path: &std::path::Path| {
            std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
        };
        let cert = CertificatePem::parse(read(&paths.cert)).expect("fixture cert parses");
        KeyPair::from_pem(cert, PrivateKeyPem::new(read(&paths.key)))
    }

    /// A DV at [`SLS`] that trusts the `rd-signing-1` fixture as the RD key.
    fn mock_with_rd() -> MockAuthState {
        let mut cfg = AuthConfig::default();
        cfg.dv.entity_id = EntityId::from_static("urn:test:dv");
        cfg.dv.slo_url = EndpointUrl::from_base_url(SLS, "SLS").expect("test SLS URL");
        let rd = IdpMetadata {
            signing_keys: vec![load_key("rd-signing-1")],
            ..IdpMetadata::for_tests()
        };
        MockAuthState::new(AuthServiceState::new(
            cfg,
            crate::keys::KeySet::default(),
            None,
            Some(rd),
        ))
    }

    /// Unfilled signature template over `#ref_id` for [`crate::saml::crypto::sign`].
    fn signature_template(ref_id: &str, cert_b64: &CertificateBase64) -> String {
        format!(
            r##"<dsig:Signature xmlns:dsig="http://www.w3.org/2000/09/xmldsig#"><dsig:SignedInfo><dsig:CanonicalizationMethod Algorithm="http://www.w3.org/2001/10/xml-exc-c14n#"/><dsig:SignatureMethod Algorithm="http://www.w3.org/2001/04/xmldsig-more#rsa-sha256"/><dsig:Reference URI="#{ref_id}"><dsig:Transforms><dsig:Transform Algorithm="http://www.w3.org/2000/09/xmldsig#enveloped-signature"/><dsig:Transform Algorithm="http://www.w3.org/2001/10/xml-exc-c14n#"/></dsig:Transforms><dsig:DigestMethod Algorithm="http://www.w3.org/2001/04/xmlenc#sha256"/><dsig:DigestValue></dsig:DigestValue></dsig:Reference></dsig:SignedInfo><dsig:SignatureValue></dsig:SignatureValue><dsig:KeyInfo><dsig:X509Data><dsig:X509Certificate>{cert_b64}</dsig:X509Certificate></dsig:X509Data></dsig:KeyInfo></dsig:Signature>"##
        )
    }

    /// A §7.7.2-valid LogoutResponse for our SLS answering `in_response_to`.
    fn logout_response(in_response_to: &str, signature: &str) -> String {
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
        format!(
            r#"<samlp:LogoutResponse xmlns:samlp="{NS_SAMLP}" xmlns:saml="{NS_SAML}" ID="{RESPONSE_ID}" Version="2.0" IssueInstant="{now}" Destination="{SLS}" InResponseTo="{in_response_to}"><saml:Issuer>{RD}</saml:Issuer>{signature}<samlp:Status><samlp:StatusCode Value="{STATUS_SUCCESS}"/></samlp:Status></samlp:LogoutResponse>"#
        )
    }

    /// A LogoutResponse genuinely signed by the RD fixture key.
    fn rd_signed_logout_response(in_response_to: &str) -> String {
        let rd_key = load_key("rd-signing-1");
        let xml = logout_response(
            in_response_to,
            &signature_template(RESPONSE_ID, &rd_key.cert_base64),
        );
        crate::saml::crypto::sign(&xml, &rd_key.key_pem).expect("RD signing")
    }

    /// Structurally complete, but the signature was never filled in.
    fn unsigned_logout_response(in_response_to: &str) -> String {
        let rd_key = load_key("rd-signing-1");
        logout_response(
            in_response_to,
            &signature_template(RESPONSE_ID, &rd_key.cert_base64),
        )
    }

    async fn process(mock: &MockAuthState, xml: &str) {
        let params = HashMap::from([("SAMLResponse".to_string(), BASE64.encode(xml.as_bytes()))]);
        process_logout_response(mock, &mock.auth, &params).await;
    }

    #[tokio::test]
    async fn rd_signed_response_to_pending_request_is_consumed_once() {
        let mock = mock_with_rd().with_pending(REQUEST_ID);
        let xml = rd_signed_logout_response(REQUEST_ID);
        assert!(mock.still_pending(REQUEST_ID));

        process(&mock, &xml).await;
        assert!(!mock.still_pending(REQUEST_ID));

        // A replay finds nothing left to match.
        process(&mock, &xml).await;
        assert!(!mock.still_pending(REQUEST_ID));
    }

    #[tokio::test]
    async fn forged_response_to_pending_request_does_not_burn_the_id() {
        let mock = mock_with_rd().with_pending(REQUEST_ID);

        process(&mock, &unsigned_logout_response(REQUEST_ID)).await;
        assert!(mock.still_pending(REQUEST_ID));

        // The genuine response can still arrive.
        process(&mock, &rd_signed_logout_response(REQUEST_ID)).await;
        assert!(!mock.still_pending(REQUEST_ID));
    }
}
