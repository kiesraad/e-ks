use crate::{
    bindings::http_post::autosubmit_post_response,
    config::{AuthConfig, Environment},
    error::Result,
    keys::KeySet,
    saml::messages::{AuthnRequestSpec, CreatedMessage, create_authn_request},
    state::{AuthFailure, AuthServiceState, AuthState},
    types::{EndpointUrl, MessageId},
};
use axum::{
    extract::{FromRef, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use axum_extra::extract::CookieJar;
use tracing::{debug, error, info, warn};

/// Initiate SAML authentication per eID §7.1 step 2 / §3.1.1.
///
/// Creates a signed AuthnRequest (eID §7.3) and POSTs it to the RD's SSO endpoint
/// via HTTP-POST binding (eID §3.1.1). The AuthnRequest ID is stored for replay
/// protection (eID §9.7) via the embedding application so it can be shared
/// across instances.
pub async fn handle_login<S>(
    State(state): State<S>,
    State(auth_state): State<AuthServiceState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> Response
where
    S: AuthState,
    AuthServiceState: FromRef<S>,
{
    debug!("[login] Handler entered");
    // No RD descriptor means we cannot build an AuthnRequest (no SSO endpoint to
    // POST to, eID §9.2). The RD was unreachable at startup and no background
    // refresh has succeeded yet; surface it as a transient outage rather than
    // emitting a broken request.
    let Some(rd) = auth_state.rd_metadata() else {
        warn!("[login] RD metadata not loaded; SAML login unavailable");
        return state
            .on_authentication_failed(AuthFailure::Unavailable, jar, &headers, true)
            .await;
    };

    match build_login_response(&auth_state, &rd.sso_url, jar.clone(), &headers) {
        Ok((request_id, response)) => {
            // Register the AuthnRequest ID for replay protection (eID §9.7)
            // before handing the browser the form that starts the flow.
            state.register_pending_request(request_id).await;
            response
        }
        Err(e) => {
            error!("[login] Failed to initiate SAML login: {e}");
            state
                .on_authentication_failed(AuthFailure::from(&e), jar, &headers, true)
                .await
        }
    }
}

/// Build the signed AuthnRequest (eID §7.3) and the HTTP-POST autosubmit
/// response (eID §3.1.1) delivering it to the RD's SSO endpoint, with the
/// flow-binding cookie attached. Returns the AuthnRequest ID so the caller can
/// register it as pending for replay protection (eID §9.7).
fn build_login_response(
    auth_state: &AuthServiceState,
    sso_url: &EndpointUrl,
    jar: CookieJar,
    headers: &HeaderMap,
) -> Result<(MessageId, Response)> {
    let config = auth_state.auth_config();
    let msg = signed_authn_request(config, auth_state.dv_keys(), sso_url)?;
    info!("[login] AuthnRequest created: {}", msg.id.log_prefix());

    // Bind this SSO flow to the current browser (login-CSRF / forced-login
    // defense): the matching ACS callback must present this cookie, whose
    // value is checked against the assertion's InResponseTo. The cookie
    // cannot be set on another browser cross-origin, so an attacker cannot
    // make a victim's browser complete a flow the attacker started.
    let jar = jar.add(crate::handlers::flow::flow_cookie(
        &config.dv.acs_url,
        &msg.id,
        headers,
    ));

    debug!("[login] Returning HTTP-POST autosubmit form");
    let resp = autosubmit_post_response(sso_url, &msg.xml, "SAMLRequest")?;
    Ok((msg.id, (jar, resp).into_response()))
}

/// Create the signed AuthnRequest (eID §7.3) for the RD's SSO endpoint.
fn signed_authn_request(
    config: &AuthConfig,
    keys: &KeySet,
    sso_url: &EndpointUrl,
) -> Result<CreatedMessage> {
    let preselected_ad_entity_id = config.preselected_ad_entity_id();

    // In the Test environment the IdP is the standalone TVS mock, which resolves
    // the SP callback from the request. The real TVS forbids this (eID §7.3) and
    // resolves the ACS from the registered metadata via the index, so we only
    // send the URL for Test, letting one shared mock serve many ephemeral test
    // environments without per-environment registration.
    let request_acs_url = (config.environment == Environment::Test).then_some(&config.dv.acs_url);

    debug!(
        "[login] entity_id={}, service_uuid={}, sso_url={}, preselected_ad={:?} ({:?}), request_acs_url={:?}, signing_keys={}",
        config.dv.entity_id,
        config.dv.service_uuid,
        sso_url,
        config.preselected_ad,
        preselected_ad_entity_id,
        request_acs_url,
        keys.signing.len(),
    );

    create_authn_request(&AuthnRequestSpec {
        entity_id: &config.dv.entity_id,
        service_uuid: &config.dv.service_uuid,
        sso_url,
        signing_key: keys.primary_signing()?,
        preselected_ad_entity_id: preselected_ad_entity_id.as_ref(),
        acs_url: request_acs_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::AuthConfig,
        handlers::test_support::MockAuthState,
        saml::idp_metadata::IdpMetadata,
        types::{EntityId, ServiceUuid},
    };
    use axum::{
        body::to_bytes,
        http::{StatusCode, header},
    };

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
    }

    fn state_with_rd() -> AuthServiceState {
        let mut cfg = AuthConfig::default().with_certs_dir(fixtures_dir());
        cfg.environment = Environment::Test;
        cfg.dv.entity_id = EntityId::from_static("urn:test:dv");
        cfg.dv.service_uuid = ServiceUuid::from_static("f847dc11-ac24-47b2-84a8-a057440ce56d");
        cfg.dv.acs_url = EndpointUrl::from_base_url("https://dv.example.com/saml/sp/acs", "ACS")
            .expect("test ACS URL");
        let keys =
            crate::keys::load_key_set(&cfg.dv.signing, &cfg.dv.encryption).expect("load fixtures");
        AuthServiceState::new(cfg, keys, Some(IdpMetadata::for_tests()))
    }

    #[tokio::test]
    async fn login_without_rd_metadata_is_unavailable() {
        // No descriptor loaded: the SAML flow reports itself unavailable rather
        // than emitting a broken AuthnRequest.
        let mock = MockAuthState::empty();
        let resp = handle_login(
            State(mock.clone()),
            State(mock.auth.clone()),
            CookieJar::new(),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn login_builds_signed_authn_request_autosubmit_form() {
        let mock = MockAuthState::new(state_with_rd());
        let resp = handle_login(
            State(mock.clone()),
            State(mock.auth.clone()),
            CookieJar::new(),
            HeaderMap::new(),
        )
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/html"
        );
        // The response sets the SSO-flow binding cookie (login-CSRF defense).
        assert!(
            resp.headers().get_all(header::SET_COOKIE).iter().count() > 0,
            "the flow-binding cookie must be set"
        );

        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        // The auto-submit form POSTs to the RD's SSO endpoint.
        assert!(
            html.contains(r#"action="https://rd.example.com/sso""#),
            "{html}"
        );
        assert!(html.contains(r#"name="SAMLRequest""#));
    }
}
