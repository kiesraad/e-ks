use crate::{
    saml::metadata::{SignedDvMetadataArgs, build_signed_dv_metadata},
    state::AuthServiceState,
};
use axum::{
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use tracing::{debug, error};

/// GET /saml/sp/metadata: serve the DV's signed SAML metadata (eID §8.3),
/// from cache when available.
pub async fn handle_metadata(
    _: crate::SamlMetadataPath,
    State(auth_state): State<AuthServiceState>,
) -> Response {
    debug!("[metadata] Handler entered");
    if let Some(xml) = auth_state.cached_metadata() {
        debug!("[metadata] Cache hit (xml_len={})", xml.len());
        return xml_response(xml);
    }
    debug!("[metadata] Cache miss; building signed metadata");

    match build_metadata(&auth_state) {
        Ok(xml) => {
            debug!(
                "[metadata] Built signed metadata (xml_len={}); caching",
                xml.len()
            );
            auth_state.set_cached_metadata(xml.clone());
            xml_response(xml)
        }
        Err(e) => {
            error!("Failed to build metadata: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to build metadata",
            )
                .into_response()
        }
    }
}

/// Assemble the SP identity, endpoints and key material from the state and
/// build the signed DV metadata (eID §8.3).
fn build_metadata(auth_state: &AuthServiceState) -> crate::error::Result<String> {
    let dv = &auth_state.auth_config().dv;
    let keys = auth_state.dv_keys();

    debug!(
        "[metadata] entity_id={}, acs_url={}, slo_url={}, service_name={}, \
         service_uuid={}, signing_keys={}, encryption_keys={}, tls_signing_cert={}",
        dv.entity_id,
        dv.acs_url,
        dv.slo_url,
        dv.service_name,
        dv.service_uuid,
        keys.signing.len(),
        keys.encryption.len(),
        auth_state.metadata_tls_cert().is_some(),
    );

    build_signed_dv_metadata(SignedDvMetadataArgs {
        entity_id: &dv.entity_id,
        acs_url: &dv.acs_url,
        slo_url: &dv.slo_url,
        service_name: &dv.service_name,
        service_uuid: &dv.service_uuid,
        signing_keys: &keys.signing,
        tls_signing_cert: auth_state.metadata_tls_cert(),
        encryption_keys: &keys.encryption,
    })
}

fn xml_response(xml: String) -> Response {
    (
        [
            (header::CONTENT_TYPE, "application/xml"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        xml,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::AuthConfig,
        types::{EndpointUrl, EntityId, ServiceUuid},
    };
    use axum::body::to_bytes;

    fn dv_url(url: &str) -> EndpointUrl {
        EndpointUrl::from_base_url(url, "DV endpoint").expect("test DV endpoint")
    }

    /// A state backed by the committed DV fixtures, enough to build real signed
    /// metadata without any env/network access.
    async fn fixture_state() -> AuthServiceState {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
        let mut cfg = AuthConfig::default().with_certs_dir(dir);
        cfg.dv.entity_id = EntityId::from_static("urn:test:dv");
        cfg.dv.acs_url = dv_url("https://dv.example.com/saml/sp/acs");
        cfg.dv.slo_url = dv_url("https://dv.example.com/saml/sp/logout");
        cfg.dv.service_name = "Kiesraad Test".to_string();
        cfg.dv.service_uuid = ServiceUuid::from_static("f847dc11-ac24-47b2-84a8-a057440ce56d");
        let keys = crate::keys::load_key_set(&cfg.dv.signing, &cfg.dv.encryption)
            .await
            .expect("load fixtures");
        let tls_cert = crate::keys::load_metadata_tls_cert(&cfg.tls.client_cert).await;
        AuthServiceState::new(cfg, keys, tls_cert, None)
    }

    async fn body_string(resp: Response) -> String {
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn builds_signs_and_caches_metadata_on_first_request() {
        let state = fixture_state().await;
        assert!(
            state.cached_metadata().is_none(),
            "nothing cached before the first request"
        );

        let resp = handle_metadata(crate::SamlMetadataPath, State(state.clone())).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/xml"
        );
        assert_eq!(
            resp.headers().get(header::X_CONTENT_TYPE_OPTIONS).unwrap(),
            "nosniff"
        );

        let body = body_string(resp).await;
        assert!(body.contains("EntityDescriptor"), "{body}");
        assert!(body.contains("urn:test:dv"));
        // The built metadata is now cached for subsequent requests.
        assert_eq!(state.cached_metadata().as_deref(), Some(body.as_str()));
    }

    #[tokio::test]
    async fn serves_cached_metadata_without_rebuilding() {
        // A state with no keys cannot build metadata, so a served response proves
        // it came from the cache rather than the build path.
        let state = AuthServiceState::new_empty();
        state.set_cached_metadata("<md:EntityDescriptor/>".to_string());

        let resp = handle_metadata(crate::SamlMetadataPath, State(state)).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_string(resp).await, "<md:EntityDescriptor/>");
    }

    #[tokio::test]
    async fn build_failure_without_signing_keys_is_a_500() {
        // No signing keys and no cache: building signed metadata fails and the
        // handler surfaces a 500 rather than panicking.
        let resp = handle_metadata(
            crate::SamlMetadataPath,
            State(AuthServiceState::new_empty()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
