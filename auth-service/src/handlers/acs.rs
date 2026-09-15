//! ACS: resolve the SAML artifact into validated claims and hand off to the
//! embedding application.
//!
//! Every failure maps to a small `AuthFailure`, and the embedding application
//! renders the user-facing page (TVS T3/L10) directly on the ACS response. The
//! technical detail is logged at the failure site. The response is marked
//! `no-store` and `no-referrer`, so the one-time `SAMLart` in the ACS query is
//! neither cached nor leaked via `Referer`. What remains in the address bar is
//! useless on its own: an artifact is consumed at the RD the first time it is
//! resolved, and one that never was can only be accepted together with the flow
//! cookie bound to its AuthnRequest, whose ID only ever travelled in the signed
//! POST body.
//!
//! Whether a failure also ends the local session (TVS L10) depends on how far
//! the callback got, see [`Rejection`].
use crate::{
    SamlAcsPath,
    bindings::soap::{send_soap_request, unwrap_soap},
    config::AuthConfig,
    keys::DecryptionKey,
    saml::{
        idp_metadata::IdpMetadata,
        loa::MINIMUM_LOA,
        messages::{CreatedMessage, create_artifact_resolve},
        validation::{
            Claims, ValidateArtifactResponseOpts, ValidateAssertionOpts, ValidateResponseOpts,
            validate_artifact_response_at, validate_assertion_at, validate_response_at,
        },
        xml_builder::wrap_in_soap_envelope,
        xml_parser::{Document, NodeId, parse},
    },
    state::{AuthFailure, AuthServiceState, AuthState},
    types::{Artifact, MessageId},
};
use axum::{
    extract::{FromRef, Query, State},
    http::{HeaderMap, HeaderValue, header},
    response::Response,
};
use axum_extra::extract::CookieJar;
use std::{collections::HashMap, sync::Arc};
use tracing::{debug, error, info, warn};

/// Why the callback produced no session, split on whether the RD had by then
/// answered this browser's own AuthnRequest.
///
/// Only an answered flow ends the local session (TVS L10). Everything before
/// that point can be provoked cross-site: the flow cookie is `Lax`, so a
/// top-level GET to the ACS from any site carries it, and a garbage `SAMLart`
/// gets past the cookie gate. Ending the session there would let a link log a
/// user out. An RD-signed Response naming the flow cookie's AuthnRequest cannot
/// be provoked that way: the ID never left the signed POST body.
#[derive(Debug, PartialEq, Eq)]
enum Rejection {
    /// No RD-signed Response for the flow cookie's AuthnRequest was seen: bad
    /// or missing input, transport, or a Response to some other request.
    Unanswered(AuthFailure),
    /// An RD-signed Response whose `InResponseTo` is the flow cookie's
    /// AuthnRequest ID, that then failed: a DigiD status (T3/L10) or an
    /// assertion that did not validate.
    Answered(AuthFailure),
}

/// Assertion Consumer Service (eID §7.1 steps 4-8 / §3.1.1).
///
/// Receives the artifact via HTTP-Artifact binding (eID §7.4). Requires the
/// browser's flow cookie *before* touching the artifact, then resolves it over
/// the mTLS back-channel (eID §7.5, §9.4), validates the ArtifactResponse
/// (§7.6.1), Response (§7.6.2), and Assertion (§7.6.3, §7.6.3.5). On success,
/// delegates to the embedding application via `AuthState::on_authenticated` so
/// it can create its own session and set the appropriate cookie.
pub async fn handle_acs<S>(
    _: SamlAcsPath,
    State(state): State<S>,
    State(auth_state): State<AuthServiceState>,
    jar: CookieJar,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response
where
    S: AuthState,
    AuthServiceState: FromRef<S>,
{
    debug!("[ACS] Handler entered, query params: {}", params.len());

    // Cheapest gate first: a callback with no flow cookie can never be accepted,
    // so refuse it before resolving. Resolving costs an mTLS SOAP round-trip to
    // the RD, which an unauthenticated caller must not be able to trigger.
    let (bound_authn_id, jar) = crate::handlers::flow::take_bound_authn_id(
        jar,
        &auth_state.auth_config().dv.acs_url,
        &headers,
    );
    let Some(bound_authn_id) = bound_authn_id else {
        warn!(
            "[ACS] SSO flow cookie missing, malformed, or not bound to this User-Agent: \
             rejecting before resolving the artifact (possible login CSRF / forced login)"
        );
        let rejection = Rejection::Unanswered(AuthFailure::Error);
        return fail(&state, rejection, jar, &headers).await;
    };

    let claims = match resolve_artifact_to_claims(&auth_state, &params, &bound_authn_id).await {
        Ok(c) => c,
        Err(rejection) => return fail(&state, rejection, jar, &headers).await,
    };

    if !confirm_pending_request(&state, &bound_authn_id, &claims).await {
        let rejection = Rejection::Answered(AuthFailure::Error);
        return fail(&state, rejection, jar, &headers).await;
    }

    // SECURITY: never log decrypted SubjectID values; they are PII (BSN /
    // pseudonym per eID §7.6.3.4). Log only non-PII metadata for tracing.
    info!(
        "[ACS] Authentication successful. acting_subject_present={}, \
         legal_subject_present={}, loa={:?}, authenticating_authority={:?}, \
         service_uuid_present={}",
        claims.acting_subject_id.is_some(),
        claims.legal_subject_id.is_some(),
        claims.authn_context_class_ref.as_deref(),
        claims.authenticating_authority.as_deref(),
        claims.service_uuid.is_some(),
    );

    // Hand off to the embedding application to create its session. An
    // assertion without an acting SubjectID carries no usable identity, so it
    // is treated as an authentication failure (TVS L10) rather than handed on,
    // guaranteeing the application's `on_authenticated` a SubjectID.
    let Some(subject_id) = claims.acting_subject_id else {
        warn!("[ACS] No acting SubjectID in validated assertion: treating as auth failure");
        let rejection = Rejection::Answered(AuthFailure::Error);
        return fail(&state, rejection, jar, &headers).await;
    };
    debug!("[ACS] Handing off to AuthState::on_authenticated");
    state
        .on_authenticated(subject_id, claims.name_id, jar, &headers)
        .await
}

/// Require the validated assertion to answer the AuthnRequest this DV issued to
/// this browser: `bound_authn_id` is the ID the (already verified and cleared)
/// flow cookie carries. `false` means reject with `AuthFailure::Error` (TVS L10).
///
/// eID §7.6.3.5 rule 4 / §9.7: the Assertion must answer an AuthnRequest this DV
/// actually issued, and the matched ID is consumed in the same atomic step so a
/// replay can never be accepted (the store is the application's, so this holds
/// even when /login and the ACS callback hit different instances). Fails closed:
/// an absent, unknown, expired, or already-consumed InResponseTo is rejected.
///
/// Login-CSRF / forced-login defense: matching against the cookie's ID refuses
/// an assertion for a flow this browser did not start, even one still
/// outstanding in the store.
async fn confirm_pending_request<S: AuthState>(
    state: &S,
    bound_authn_id: &MessageId,
    claims: &Claims,
) -> bool {
    let Some(in_response_to) = claims.in_response_to.as_ref() else {
        warn!("[ACS] Assertion has no InResponseTo: cannot correlate to a pending AuthnRequest");
        return false;
    };

    if in_response_to != bound_authn_id {
        warn!(
            "[ACS] Assertion InResponseTo is not the AuthnRequest the SSO flow cookie is \
             bound to: rejecting (possible login CSRF / forced login)"
        );
        return false;
    }

    if !state.consume_if_pending(in_response_to.clone()).await {
        warn!(
            "[ACS] InResponseTo did not match an outstanding AuthnRequest \
             (unknown, expired, or replayed): rejecting"
        );
        return false;
    }
    true
}

/// Render the failure page for a rejected callback. The embedding application
/// draws the page and, for an answered flow, ends its local session (TVS L10).
/// Cookie changes staged on `jar` (the one-shot flow-cookie clearing) ride along.
async fn fail<S: AuthState>(
    state: &S,
    rejection: Rejection,
    jar: CookieJar,
    headers: &HeaderMap,
) -> Response {
    let (failure, end_session) = match rejection {
        Rejection::Unanswered(failure) => (failure, false),
        Rejection::Answered(failure) => (failure, true),
    };
    let mut response = state
        .on_authentication_failed(failure, jar, headers, end_session)
        .await;
    harden_headers(response.headers_mut());
    response
}

/// Defense-in-depth headers for the failure page, served on the URL that
/// carried the artifact: never cache it, and never leak it via `Referer`.
fn harden_headers(headers: &mut HeaderMap) {
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
}

/// Pull the one-time-use artifact out of the ACS query string (eID §7.4).
///
/// The artifact is an opaque reference, so a short prefix is safe to log for
/// correlation and is not itself sensitive PII.
fn artifact_from_params(params: &HashMap<String, String>) -> Result<Artifact, AuthFailure> {
    let Some(artifact) = params.get("SAMLart") else {
        warn!("[ACS] Missing SAMLart query parameter");
        return Err(AuthFailure::Error);
    };
    // Parsed, not just read: the value is signed into an ArtifactResolve and
    // sent to the RD, so an unbounded or non-base64 query parameter is refused
    // here rather than forwarded.
    let artifact = Artifact::parse(artifact).map_err(|e| {
        warn!("[ACS] Malformed SAMLart query parameter: {e}");
        AuthFailure::Error
    })?;

    info!("[ACS] Artifact received: {}...", artifact.log_prefix());

    Ok(artifact)
}

/// Parse the SOAP ArtifactResponse envelope exactly once; the whole
/// ArtifactResponse -> Response -> Assertion chain is then navigated on this one
/// tree, so inner elements keep the namespaces they inherit.
fn parse_soap_envelope(soap: &str) -> Result<Document<'_>, AuthFailure> {
    parse(soap).map_err(|e| {
        error!("[ACS] Failed to parse SOAP ArtifactResponse: {e}");
        AuthFailure::Error
    })
}

/// Resolve the artifact into validated [`Claims`], or a [`Rejection`] the
/// caller turns into a user-facing page. `bound_authn_id` is the AuthnRequest
/// the flow cookie binds this browser to.
async fn resolve_artifact_to_claims(
    auth_state: &AuthServiceState,
    params: &HashMap<String, String>,
    bound_authn_id: &MessageId,
) -> Result<Claims, Rejection> {
    let resolved = resolve_artifact(auth_state, params)
        .await
        .map_err(Rejection::Unanswered)?;

    // 3-5. Parse the SOAP ArtifactResponse exactly once and validate the
    //      ArtifactResponse -> Response -> Assertion chain by navigating that
    //      single tree. Inner elements (Response, Assertion) inherit their
    //      namespaces from the ArtifactResponse and are never re-parsed as
    //      standalone fragments; signature verification uses the self-contained
    //      source bytes of the RD-signed ArtifactResponse element.
    let doc = parse_soap_envelope(&resolved.soap).map_err(Rejection::Unanswered)?;
    let chain = ResponseChain {
        doc: &doc,
        auth_state,
        rd: &resolved.rd,
    };
    chain.claims(&resolved.resolve_id, bound_authn_id)
}

/// The RD's SOAP ArtifactResponse, the `@ID` of the ArtifactResolve it must
/// answer, and the RD descriptor it was resolved against.
struct ResolvedArtifact {
    soap: String,
    resolve_id: MessageId,
    rd: Arc<IdpMetadata>,
}

/// Steps 1-2: turn the `SAMLart` query parameter into the RD's ArtifactResponse.
async fn resolve_artifact(
    auth_state: &AuthServiceState,
    params: &HashMap<String, String>,
) -> Result<ResolvedArtifact, AuthFailure> {
    let artifact = artifact_from_params(params)?;

    let cfg = auth_state.auth_config();
    let dv_keys = auth_state.dv_keys();
    // Without the RD descriptor we have neither the ARS endpoint to resolve the
    // artifact against nor the RD signing keys to verify the response (eID §9.2),
    // so the flow cannot proceed. Transient: the RD metadata is not loaded yet.
    let Some(rd) = auth_state.rd_metadata() else {
        warn!("[ACS] RD metadata not loaded: cannot resolve or validate the artifact");
        return Err(AuthFailure::Unavailable);
    };

    debug!(
        "[ACS] Using DV entity_id={}, ARS url={}, signing_keys={}, encryption_keys={}",
        cfg.dv.entity_id,
        rd.ars_url,
        dv_keys.signing.len(),
        dv_keys.encryption.len()
    );

    // 1. Create signed ArtifactResolve (eID §7.5)
    let signing_key = dv_keys.primary_signing().map_err(|e| {
        error!("[ACS] Cannot sign the ArtifactResolve: {e}");
        AuthFailure::from(&e)
    })?;
    let resolve = build_artifact_resolve(&artifact, cfg, &rd, signing_key)?;

    // 2. Wrap in SOAP and send to ARS over mTLS (eID §9.4)
    let soap = send_artifact_resolve(&resolve.xml, &rd, cfg).await?;
    Ok(ResolvedArtifact {
        soap,
        resolve_id: resolve.id,
        rd,
    })
}

/// The parsed SOAP document plus the trust context threaded through the
/// ArtifactResponse -> Response -> Assertion validation chain (eID §7.6).
struct ResponseChain<'a, 'input> {
    doc: &'a Document<'input>,
    auth_state: &'a AuthServiceState,
    rd: &'a IdpMetadata,
}

impl ResponseChain<'_, '_> {
    /// Validate the full chain and extract the [`Claims`]. `resolve_id` is the
    /// `@ID` of the ArtifactResolve this response must answer, `bound_authn_id`
    /// the AuthnRequest the browser's flow cookie is bound to.
    fn claims(
        &self,
        resolve_id: &MessageId,
        bound_authn_id: &MessageId,
    ) -> Result<Claims, Rejection> {
        let root = self.doc.document_element();
        let Some(art_node) = unwrap_soap(self.doc, root) else {
            warn!(
                "[ACS] Failed to unwrap SOAP envelope (root_element={:?})",
                self.doc.local_name(root)
            );
            return Err(Rejection::Unanswered(AuthFailure::Error));
        };

        // 3. Validate ArtifactResponse (eID §7.6.1) using RD signing certs from metadata
        let response_node = self
            .response_node(art_node, resolve_id)
            .map_err(Rejection::Unanswered)?;
        // The Response is now RD-signed; from here on a failure is the RD's
        // answer to this browser's own flow, provided it names that flow.
        check_answers_bound_request(self.doc, response_node, bound_authn_id)
            .map_err(Rejection::Unanswered)?;

        // 4. Validate Response (eID §7.6.2): handle cancellation / IdP errors
        let assertion_node = self
            .assertion_node(response_node)
            .map_err(Rejection::Answered)?;

        // 5. Validate Assertion (eID §7.6.3, §7.6.3.5). The Assertion is
        //    authenticated by the enveloping RD signature on the ArtifactResponse
        //    (verified in step 3); per §9.1 only signatures outside an
        //    Assertion/Advice are validated. Binds the Assertion Issuer to the
        //    RD EntityID (`minvws/nl-rdo-max`).
        let claims = self
            .assertion_claims(assertion_node)
            .map_err(Rejection::Answered)?;

        self.check_matching_in_response_to(response_node, &claims)
            .map_err(Rejection::Answered)?;
        Ok(claims)
    }

    // Cross-check only: both values come from the same Response, so this does
    // NOT by itself satisfy eID §7.6.3.5 rule 4. Rule 4 (the assertion answers an
    // AuthnRequest this DV actually issued) is enforced in
    // `confirm_pending_request` below, which matches `claims.in_response_to`
    // against the pending-request store and consumes it.
    //
    // What this adds on top: eID §7.6.2 gives the Response an @InResponseTo of
    // cardinality 1, and it names the same AuthnRequest as the assertion's
    // SubjectConfirmationData. Since only the assertion's value is checked
    // against the store, requiring the two to agree rejects a Response whose
    // envelope and assertion name different requests, i.e. an assertion spliced
    // into a Response for another flow.
    fn check_matching_in_response_to(
        &self,
        response_node: NodeId,
        claims: &Claims,
    ) -> Result<(), AuthFailure> {
        let response_in_response_to = self.doc.get_attribute(response_node, "InResponseTo");
        if response_in_response_to != claims.in_response_to.as_ref().map(MessageId::as_str) {
            warn!(
                "[ACS] Response @InResponseTo does not match the assertion's InResponseTo: rejecting"
            );
            return Err(AuthFailure::Error);
        }
        Ok(())
    }
}

/// Require the (RD-signed) Response to answer the AuthnRequest the browser's flow
/// cookie is bound to. Decides the [`Rejection`] variant of what follows: a
/// Response to some other request, e.g. an attacker's own flow handed to this
/// browser, is not this browser's failed login and must not end its session.
/// `confirm_pending_request` later re-checks the assertion's copy of the ID
/// against the store and consumes it.
fn check_answers_bound_request(
    doc: &Document,
    response_node: NodeId,
    bound_authn_id: &MessageId,
) -> Result<(), AuthFailure> {
    if doc.get_attribute(response_node, "InResponseTo") != Some(bound_authn_id.as_str()) {
        warn!(
            "[ACS] Response @InResponseTo is not the AuthnRequest the SSO flow cookie is \
             bound to: rejecting (possible login CSRF / forced login)"
        );
        return Err(AuthFailure::Error);
    }
    Ok(())
}

fn build_artifact_resolve(
    artifact: &Artifact,
    cfg: &AuthConfig,
    rd: &IdpMetadata,
    signing_key: &crate::keys::KeyPair,
) -> Result<CreatedMessage, AuthFailure> {
    debug!("[ACS] Step 1: building signed ArtifactResolve");
    match create_artifact_resolve(artifact, &cfg.dv.entity_id, &rd.ars_url, signing_key) {
        Ok(m) => {
            debug!(
                "[ACS] ArtifactResolve built: id={}, xml_len={}",
                m.id,
                m.xml.len()
            );
            Ok(m)
        }
        Err(e) => {
            error!("[ACS] Failed to create ArtifactResolve: {e}");
            Err(AuthFailure::Error)
        }
    }
}

async fn send_artifact_resolve(
    resolve_xml: &str,
    rd: &IdpMetadata,
    cfg: &AuthConfig,
) -> Result<String, AuthFailure> {
    debug!("[ACS] Step 2: sending ArtifactResolve over mTLS SOAP back-channel");
    let soap_xml = wrap_in_soap_envelope(resolve_xml).map_err(|e| {
        error!("[ACS] Failed to build SOAP envelope: {e}");
        AuthFailure::Error
    })?;
    match send_soap_request(&rd.ars_url, &soap_xml, &cfg.tls).await {
        Ok(r) => {
            debug!(
                "[ACS] SOAP back-channel returned response (len={})",
                r.len()
            );
            Ok(r)
        }
        Err(e) => {
            error!("[ACS] SOAP back-channel failed: {e}");
            Err(AuthFailure::Error)
        }
    }
}

impl ResponseChain<'_, '_> {
    fn response_node(
        &self,
        art_node: NodeId,
        expected_id: &MessageId,
    ) -> Result<NodeId, AuthFailure> {
        debug!(
            "[ACS] Step 3: validating ArtifactResponse against {} RD signing key(s), \
             expected InResponseTo={}",
            self.rd.signing_keys.len(),
            expected_id
        );
        let mut errors = Vec::new();
        let response = validate_artifact_response_at(
            self.doc,
            art_node,
            &ValidateArtifactResponseOpts {
                trusted_keys: &self.rd.signing_keys,
                expected_in_response_to: Some(expected_id),
                // eID §7.6.1: bind the ArtifactResponse Issuer to the RD EntityID.
                expected_issuer: Some(&self.rd.entity_id),
            },
            &mut errors,
        );
        if !errors.is_empty() {
            error!("[ACS] ArtifactResponse validation failed: {errors:?}");
            return Err(AuthFailure::Error);
        }
        debug!("[ACS] Step 3: ArtifactResponse OK");
        response.ok_or_else(|| {
            warn!("[ACS] No Response in ArtifactResponse");
            AuthFailure::Error
        })
    }

    fn assertion_node(&self, response_node: NodeId) -> Result<NodeId, AuthFailure> {
        debug!("[ACS] Step 4: validating inner Response status");
        let mut errors = Vec::new();
        let assertion = validate_response_at(
            self.doc,
            response_node,
            // eID §7.6.2: bind the Response to this DV's ACS and the RD as issuer,
            // mirroring the assertion-level Recipient/Issuer checks (§7.6.3.5 r1-2).
            &ValidateResponseOpts {
                expected_destination: Some(&self.auth_state.auth_config().dv.acs_url),
                expected_issuer: Some(&self.rd.entity_id),
            },
            &mut errors,
        );

        if !errors.is_empty() {
            let errors_str = errors.join("; ");
            let is_authn_failed = errors_str.contains("AuthnFailed");
            let is_cancelled = errors_str.contains("Authentication cancelled");

            // TVS "Checklist Testen" v2.1 T3: the user cancelled.
            if is_authn_failed || is_cancelled {
                warn!("[ACS] Authentication cancelled by user");
                return Err(AuthFailure::Cancelled);
            }

            // TVS "Checklist Testen" v2.1 L10: RD/DigiD error status.
            warn!("[ACS] Authentication failed: {errors_str}");
            return Err(AuthFailure::Error);
        }
        debug!("[ACS] Step 4: Response status Success");

        assertion.ok_or_else(|| {
            warn!("[ACS] No Assertion element extracted from successful Response");
            AuthFailure::Error
        })
    }

    fn assertion_claims(&self, assertion_node: NodeId) -> Result<Claims, AuthFailure> {
        debug!("[ACS] Step 5: validating Assertion");

        let cfg = self.auth_state.auth_config();
        let priv_keys = DecryptionKey::from_key_set(self.auth_state.dv_keys());

        let mut errors = Vec::new();
        let claims = validate_assertion_at(
            self.doc,
            assertion_node,
            &ValidateAssertionOpts {
                dv_entity_id: &cfg.dv.entity_id,
                expected_recipient: Some(&cfg.dv.acs_url),
                // eID §9.1: the Assertion is authenticated by the enveloping RD
                // signature on the ArtifactResponse (verified in step 3); here we
                // only bind the Assertion Issuer to the RD EntityID (`minvws/nl-rdo-max`).
                expected_issuer: Some(&self.rd.entity_id),
                private_keys: &priv_keys,
                minimum_loa: Some(MINIMUM_LOA),
                // eID §7.6.3.4: bind to the registered service.
                expected_service_uuid: Some(&cfg.dv.service_uuid),
            },
            &mut errors,
        );

        claims.ok_or_else(|| {
            error!("[ACS] Assertion validation failed: {errors:?}");
            AuthFailure::Error
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::AuthConfig,
        handlers::test_support::MockAuthState,
        keys::{CertificatePem, KeyPair, KeySet, PrivateKeyPem, key_pair_paths},
        saml::{
            constants::{
                EID_ACTING_SUBJECT_ID, EID_SERVICE_UUID, NAMEID_PERSISTENT, NAMEID_TRANSIENT,
                NS_SAML, NS_SAMLP, STATUS_SUCCESS, SUBJECT_CONFIRMATION_BEARER,
            },
            crypto::sign,
        },
        types::{EndpointUrl, EntityId, ServiceUuid},
    };
    use chrono::{Duration, Utc};
    use secrecy::ExposeSecret;
    use std::path::PathBuf;

    /// The DV this test suite configures, and the RD of [`IdpMetadata::for_tests`].
    const DV: &str = "urn:test:dv";
    const RD: &str = "urn:test:rd";
    const ACS: &str = "https://dv.example.com/saml/sp/acs";
    const SERVICE_UUID: &str = "f847dc11-ac24-47b2-84a8-a057440ce56d";
    /// eIDAS substantial: above [`MINIMUM_LOA`], so it passes the LoA check.
    const LOA: &str = "http://eidas.europa.eu/LoA/substantial";
    /// The `@ID` of the ArtifactResolve the RD's ArtifactResponse answers.
    const RESOLVE_ID: &str = "_resolve1";
    /// The AuthnRequest this browser's flow cookie is bound to.
    const BOUND_ID: &str = "_bound1";
    /// The BSN the RD encrypts into the assertion's ActingSubjectID.
    const BSN: &str = "900070341";

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
    }

    /// A keypair from the committed fixtures, read synchronously (the library's
    /// loaders are async).
    fn load_key(name: &str) -> KeyPair {
        let paths = key_pair_paths(&fixtures_dir(), name);
        let read = |path: &std::path::Path| {
            std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
        };
        let cert = CertificatePem::parse(read(&paths.cert)).expect("fixture cert parses");
        KeyPair::from_pem(cert, PrivateKeyPem::new(read(&paths.key)))
    }

    fn rd_metadata() -> IdpMetadata {
        IdpMetadata::for_tests()
    }

    /// A configured DV that trusts the `rd-signing-1` fixture as the RD's
    /// signing key, so a hand-built ArtifactResponse validates against it.
    async fn state_with_rd() -> AuthServiceState {
        let mut cfg = AuthConfig::default().with_certs_dir(fixtures_dir());
        cfg.dv.entity_id = EntityId::from_static(DV);
        cfg.dv.service_uuid = ServiceUuid::from_static(SERVICE_UUID);
        cfg.dv.acs_url = EndpointUrl::from_base_url(ACS, "ACS").expect("test ACS URL");
        let keys = crate::keys::load_key_set(&cfg.dv.signing, &cfg.dv.encryption)
            .await
            .expect("load fixtures");
        let rd = IdpMetadata {
            signing_keys: vec![load_key("rd-signing-1")],
            ..IdpMetadata::for_tests()
        };
        AuthServiceState::new(cfg, keys, None, Some(rd))
    }

    /// A jar carrying the flow cookie `/login` would have set, so a test gets
    /// past the gate to the artifact-resolving part of the handler.
    fn jar_with_flow_cookie(
        auth: &AuthServiceState,
        authn_id: &str,
        headers: &HeaderMap,
    ) -> CookieJar {
        let acs_url = &auth.auth_config().dv.acs_url;
        let id = MessageId::parse(authn_id).expect("test message id");
        CookieJar::new().add(crate::handlers::flow::flow_cookie(acs_url, &id, headers))
    }

    /// Whether the embedder was told to end its session, as `MockAuthState`
    /// records it. Also asserts the failure page is rendered in place (no
    /// redirect) with the artifact-hardening headers.
    fn failure_ends_session(resp: &Response) -> bool {
        assert!(
            resp.headers().get(axum::http::header::LOCATION).is_none(),
            "the failure page is rendered directly on the ACS response"
        );
        assert_eq!(
            resp.headers().get(axum::http::header::CACHE_CONTROL),
            Some(&axum::http::HeaderValue::from_static("no-store")),
            "failure responses must not be cached"
        );
        assert_eq!(
            resp.headers().get(axum::http::header::REFERRER_POLICY),
            Some(&axum::http::HeaderValue::from_static("no-referrer")),
            "failure responses must not leak the artifact via Referer"
        );
        resp.headers()
            .get("x-test-end-session")
            .and_then(|v| v.to_str().ok())
            .expect("MockAuthState records end_session")
            == "true"
    }

    fn artifact_params() -> HashMap<String, String> {
        HashMap::from([(
            "SAMLart".to_string(),
            "AAQAAsomeOpaqueArtifact==".to_string(),
        )])
    }

    #[tokio::test]
    async fn missing_saml_artifact_renders_error_without_ending_session() {
        // No `SAMLart` query parameter, with a flow cookie: the callback fails
        // closed with Error. Nothing RD-signed answered this browser's flow (a
        // cross-site GET carries the Lax cookie), so the session is kept.
        let mock = MockAuthState::empty();
        let headers = HeaderMap::new();
        let resp = handle_acs(
            SamlAcsPath,
            State(mock.clone()),
            State(mock.auth.clone()),
            jar_with_flow_cookie(&mock.auth, "_pending", &headers),
            headers.clone(),
            Query(HashMap::new()),
        )
        .await;
        // MockAuthState renders Error as 401.
        assert_eq!(resp.status(), axum::http::StatusCode::UNAUTHORIZED);
        assert!(!failure_ends_session(&resp));
    }

    #[tokio::test]
    async fn artifact_without_rd_metadata_renders_unavailable() {
        // An artifact is present but no RD descriptor is loaded, so the flow
        // cannot be resolved or validated: the Unavailable page, session kept.
        let mock = MockAuthState::empty();
        let headers = HeaderMap::new();
        let resp = handle_acs(
            SamlAcsPath,
            State(mock.clone()),
            State(mock.auth.clone()),
            jar_with_flow_cookie(&mock.auth, "_pending", &headers),
            headers.clone(),
            Query(artifact_params()),
        )
        .await;
        assert_eq!(resp.status(), axum::http::StatusCode::SERVICE_UNAVAILABLE);
        assert!(!failure_ends_session(&resp));
    }

    #[tokio::test]
    async fn callback_without_flow_cookie_is_rejected_before_resolving() {
        // The page proves the ordering: this mock has no RD metadata, so
        // resolving first would have rendered Unavailable rather than Error.
        let mock = MockAuthState::empty();
        let resp = handle_acs(
            SamlAcsPath,
            State(mock.clone()),
            State(mock.auth.clone()),
            axum_extra::extract::CookieJar::new(),
            HeaderMap::new(),
            Query(artifact_params()),
        )
        .await;
        assert_eq!(
            resp.status(),
            axum::http::StatusCode::UNAUTHORIZED,
            "the flow-cookie gate must reject before the artifact is resolved"
        );
        // no flow to end: a cross-site link must not log anyone out
        assert!(!failure_ends_session(&resp));
    }

    #[tokio::test]
    async fn answered_flow_ends_the_session() {
        let mock = MockAuthState::empty();
        let headers = HeaderMap::new();

        let resp = fail(
            &mock,
            Rejection::Answered(AuthFailure::Cancelled),
            CookieJar::new(),
            &headers,
        )
        .await;
        // MockAuthState renders Cancelled as 403.
        assert_eq!(resp.status(), axum::http::StatusCode::FORBIDDEN);
        assert!(failure_ends_session(&resp));
    }

    #[tokio::test]
    async fn unanswered_flow_does_not_end_the_session() {
        let mock = MockAuthState::empty();
        let headers = HeaderMap::new();
        let resp = fail(
            &mock,
            Rejection::Unanswered(AuthFailure::Cancelled),
            CookieJar::new(),
            &headers,
        )
        .await;
        assert_eq!(resp.status(), axum::http::StatusCode::FORBIDDEN);
        assert!(!failure_ends_session(&resp));
    }

    #[test]
    fn response_must_answer_the_bound_authn_request() {
        let bound = MessageId::parse("_mine").unwrap();
        let check = |xml: &str| {
            let doc = parse(xml).expect("test Response parses");
            check_answers_bound_request(&doc, doc.document_element(), &bound)
        };
        const NS: &str = r#"xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol""#;

        assert!(check(&format!(r#"<samlp:Response {NS} InResponseTo="_mine"/>"#)).is_ok());
        // A Response to another flow (forced login) is not this browser's failure.
        assert_eq!(
            check(&format!(r#"<samlp:Response {NS} InResponseTo="_theirs"/>"#)),
            Err(AuthFailure::Error)
        );
        assert_eq!(
            check(&format!("<samlp:Response {NS}/>")),
            Err(AuthFailure::Error)
        );
    }

    #[test]
    fn build_artifact_resolve_produces_a_signed_message() {
        let cfg = AuthConfig {
            dv: crate::config::DvConfig {
                entity_id: EntityId::from_static(DV),
                ..Default::default()
            },
            ..AuthConfig::default()
        };
        let rd = rd_metadata();
        let key = load_key("dv-signing-1");

        let artifact = Artifact::parse("AAQAAartifact").expect("test artifact");
        let msg = build_artifact_resolve(&artifact, &cfg, &rd, &key)
            .expect("ArtifactResolve must build and sign");
        assert!(msg.id.as_str().starts_with('_'), "message id: {}", msg.id);
        assert!(msg.xml.contains("ArtifactResolve"), "{}", msg.xml);
        // The artifact and the destination ARS endpoint are carried in the XML.
        assert!(msg.xml.contains("AAQAAartifact"));
        assert!(msg.xml.contains("https://rd.example.com/ars"));
    }

    #[tokio::test]
    async fn malformed_saml_artifact_renders_error_without_ending_session() {
        // Refused where it is read, before it can be signed into an
        // ArtifactResolve and sent to the RD.
        let mock = MockAuthState::new(state_with_rd().await);
        let headers = HeaderMap::new();
        let resp = handle_acs(
            SamlAcsPath,
            State(mock.clone()),
            State(mock.auth.clone()),
            jar_with_flow_cookie(&mock.auth, BOUND_ID, &headers),
            headers.clone(),
            Query(HashMap::from([(
                "SAMLart".to_string(),
                "not base64!".to_string(),
            )])),
        )
        .await;
        assert_eq!(resp.status(), axum::http::StatusCode::UNAUTHORIZED);
        assert!(!failure_ends_session(&resp));
    }

    #[tokio::test]
    async fn a_failing_back_channel_renders_error_without_ending_session() {
        // The artifact is well-formed and gets signed into an ArtifactResolve,
        // but the ARS is unreachable: nothing RD-signed ever answered this
        // browser's flow, so the session survives the transport failure.
        let mut auth_cfg = AuthConfig::default().with_certs_dir(fixtures_dir());
        auth_cfg.dv.entity_id = EntityId::from_static(DV);
        let keys = crate::keys::load_key_set(&auth_cfg.dv.signing, &auth_cfg.dv.encryption)
            .await
            .expect("load fixtures");
        let rd = IdpMetadata {
            // A closed port on loopback: connection refused, no DNS, no waiting.
            ars_url: EndpointUrl::from_metadata("https://127.0.0.1:1/ars", "ARS")
                .expect("test ARS URL"),
            ..IdpMetadata::for_tests()
        };
        let mock = MockAuthState::new(AuthServiceState::new(auth_cfg, keys, None, Some(rd)));

        let headers = HeaderMap::new();
        let resp = handle_acs(
            SamlAcsPath,
            State(mock.clone()),
            State(mock.auth.clone()),
            jar_with_flow_cookie(&mock.auth, BOUND_ID, &headers),
            headers.clone(),
            Query(artifact_params()),
        )
        .await;
        assert_eq!(resp.status(), axum::http::StatusCode::UNAUTHORIZED);
        assert!(!failure_ends_session(&resp));
    }

    // -----------------------------------------------------------------------
    // The validation chain, driven over a real RD-signed ArtifactResponse
    // (built and signed here with the `rd-signing-1` fixture) so the
    // ArtifactResponse -> Response -> Assertion navigation the handler performs
    // is exercised rather than stubbed. Only the mTLS back-channel that would
    // deliver these bytes is left out.
    // -----------------------------------------------------------------------

    /// A SAML timestamp at `offset` from now.
    fn ts(offset: Duration) -> String {
        (Utc::now() + offset).to_rfc3339()
    }

    /// How the RD answered: the Response Status, and with it whether the
    /// Response carries an Assertion at all (eID §7.6.2).
    enum Outcome {
        Success,
        /// The user pressed cancel at DigiD (TVS T3).
        Cancelled,
        /// Any other RD/DigiD error status (TVS L10).
        Failed,
    }

    /// The parts of the RD's ArtifactResponse per test varies; [`Default`] is the
    /// message a successful login against this DV produces.
    struct Wire {
        /// The ArtifactResolve `@ID` the ArtifactResponse answers.
        resolve_id: &'static str,
        /// The Response `@InResponseTo`: the AuthnRequest the RD is answering.
        response_in_response_to: &'static str,
        /// The assertion's own copy of it, in SubjectConfirmationData.
        assertion_in_response_to: &'static str,
        outcome: Outcome,
        /// The key the ArtifactResponse is signed with.
        signing_key: &'static str,
        /// The assertion's `<saml:Audience>`: the DV it was minted for.
        audience: &'static str,
    }

    impl Default for Wire {
        fn default() -> Self {
            Self {
                resolve_id: RESOLVE_ID,
                response_in_response_to: BOUND_ID,
                assertion_in_response_to: BOUND_ID,
                outcome: Outcome::Success,
                signing_key: "rd-signing-1",
                audience: DV,
            }
        }
    }

    /// The Response `<Status>` for an outcome. The two failure statuses differ
    /// only in their second-level code, which is what the handler reads to tell
    /// a cancellation (T3) from any other failure (L10).
    fn status_xml(outcome: &Outcome) -> String {
        let responder = "urn:oasis:names:tc:SAML:2.0:status:Responder";
        match outcome {
            Outcome::Success => {
                format!(
                    r#"<samlp:Status><samlp:StatusCode Value="{STATUS_SUCCESS}"/></samlp:Status>"#
                )
            }
            Outcome::Cancelled => format!(
                r#"<samlp:Status><samlp:StatusCode Value="{responder}"><samlp:StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:AuthnFailed"/></samlp:StatusCode><samlp:StatusMessage>Authentication cancelled</samlp:StatusMessage></samlp:Status>"#
            ),
            Outcome::Failed => format!(
                r#"<samlp:Status><samlp:StatusCode Value="{responder}"><samlp:StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:RequestDenied"/></samlp:StatusCode><samlp:StatusMessage>Er is een fout opgetreden</samlp:StatusMessage></samlp:Status>"#
            ),
        }
    }

    /// An `<saml:EncryptedID>` carrying the acting subject's BSN, wrapped to the
    /// DV's own encryption key the way the RD does it (eID §7.6.3.4, §9.3:
    /// AES-256-CBC data, RSA-OAEP key wrap, addressed to us by `@Recipient`).
    fn encrypted_acting_subject(dv_keys: &KeySet) -> String {
        use bergshamra_enc::{EncContext, encrypt::encrypt};
        use bergshamra_keys::{KeysManager, loader};

        let recipient = dv_keys.encryption.first().expect("a DV encryption key");
        let key_name = recipient.key_name.as_str();
        let name_id = format!(
            r#"<saml:NameID xmlns:saml="{NS_SAML}" 
                            Format="{NAMEID_PERSISTENT}" 
                            NameQualifier="urn:nl-eid-gdi:1.0:id:legacy-BSN">
                {BSN}
            </saml:NameID>"#
        );
        let template = format!(
            r#"
            <saml:EncryptedID xmlns:saml="{NS_SAML}" 
                                 xmlns:xenc="http://www.w3.org/2001/04/xmlenc#" 
                                 xmlns:ds="http://www.w3.org/2000/09/xmldsig#">
              <xenc:EncryptedData Type="http://www.w3.org/2001/04/xmlenc#Element">
                <xenc:EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes256-cbc"/>
                <ds:KeyInfo>
                  <xenc:EncryptedKey Recipient="{DV}">
                    <xenc:EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#rsa-oaep-mgf1p"/>
                    <ds:KeyInfo>
                      <ds:KeyName>{key_name}</ds:KeyName>
                    </ds:KeyInfo>
                    <xenc:CipherData>
                      <xenc:CipherValue></xenc:CipherValue>
                    </xenc:CipherData>
                  </xenc:EncryptedKey>
                </ds:KeyInfo>
                <xenc:CipherData>
                  <xenc:CipherValue></xenc:CipherValue>
                </xenc:CipherData>
              </xenc:EncryptedData>
            </saml:EncryptedID>"#
        );

        let cert = loader::load_x509_cert_pem(recipient.cert_pem.as_str().as_bytes())
            .expect("load DV encryption cert")
            .with_name(key_name);
        let mut mgr = KeysManager::new();
        mgr.add_key(cert);
        encrypt(&EncContext::new(mgr), &template, name_id.as_bytes()).expect("encrypt the NameID")
    }

    /// The Assertion a successful login carries: every §7.6.3 check passes for
    /// the DV [`state_with_rd`] configures.
    fn assertion_xml(wire: &Wire, dv_keys: &KeySet) -> String {
        let issued = ts(Duration::zero());
        let scd_expiry = ts(Duration::minutes(2));
        let not_before = ts(-Duration::minutes(5));
        let not_on_or_after = ts(Duration::minutes(5));
        let in_response_to = wire.assertion_in_response_to;
        let audience = wire.audience;
        let encrypted_id = encrypted_acting_subject(dv_keys);
        format!(
            r#"
            <saml:Assertion ID="_assertion1" Version="2.0" IssueInstant="{issued}">
              <saml:Issuer>{RD}</saml:Issuer>
              <saml:Subject>
                <saml:NameID Format="{NAMEID_TRANSIENT}">
                  transient-subject
                </saml:NameID>
                <saml:SubjectConfirmation Method="{SUBJECT_CONFIRMATION_BEARER}">
                  <saml:SubjectConfirmationData NotOnOrAfter="{scd_expiry}" 
                                                Recipient="{ACS}" 
                                                InResponseTo="{in_response_to}"/>
                </saml:SubjectConfirmation>
              </saml:Subject>
              <saml:Conditions NotBefore="{not_before}" 
                               NotOnOrAfter="{not_on_or_after}">
                <saml:AudienceRestriction>
                  <saml:Audience>{audience}</saml:Audience>
                </saml:AudienceRestriction>
              </saml:Conditions>
              <saml:AuthnStatement AuthnInstant="{issued}">
                <saml:AuthnContext>
                  <saml:AuthnContextClassRef>{LOA}</saml:AuthnContextClassRef>
                </saml:AuthnContext>
              </saml:AuthnStatement>
              <saml:AttributeStatement>
                <saml:Attribute Name="{EID_SERVICE_UUID}">
                  <saml:AttributeValue>{SERVICE_UUID}</saml:AttributeValue>
                </saml:Attribute>
                <saml:Attribute Name="{EID_ACTING_SUBJECT_ID}">
                  <saml:AttributeValue>{encrypted_id}</saml:AttributeValue>
                </saml:Attribute>
              </saml:AttributeStatement>
            </saml:Assertion>"#
        )
    }

    /// The RD's SOAP ArtifactResponse for `wire`, signed over the whole
    /// envelope element as the RD signs it (eID §7.6.1).
    fn soap_artifact_response(wire: &Wire, dv_keys: &KeySet) -> String {
        let issued = ts(Duration::zero());
        let status = status_xml(&wire.outcome);
        // eID §7.6.2: an Assertion is present only on Success.
        let assertion = match wire.outcome {
            Outcome::Success => assertion_xml(wire, dv_keys),
            Outcome::Cancelled | Outcome::Failed => String::new(),
        };
        let in_response_to = wire.response_in_response_to;
        let response = format!(
            r#"
            <samlp:Response ID="_response1" 
                               Version="2.0"
                               IssueInstant="{issued}" 
                               Destination="{ACS}" 
                               InResponseTo="{in_response_to}">
              <saml:Issuer>{RD}</saml:Issuer>
              {status}
              {assertion}
            </samlp:Response>"#
        );

        let rd_key = load_key(wire.signing_key);
        let cert = &rd_key.cert_base64;
        // The enveloped-signature template the signer fills in, as the RD emits it.
        let signature = format!(
            r##"
            <dsig:Signature xmlns:dsig="http://www.w3.org/2000/09/xmldsig#">
              <dsig:SignedInfo>
                <dsig:CanonicalizationMethod Algorithm="http://www.w3.org/2001/10/xml-exc-c14n#"/>
                <dsig:SignatureMethod Algorithm="http://www.w3.org/2001/04/xmldsig-more#rsa-sha256"/>
                <dsig:Reference URI="#_artifactresponse1">
                  <dsig:Transforms>
                    <dsig:Transform Algorithm="http://www.w3.org/2000/09/xmldsig#enveloped-signature"/>
                    <dsig:Transform Algorithm="http://www.w3.org/2001/10/xml-exc-c14n#"/>
                  </dsig:Transforms>
                  <dsig:DigestMethod Algorithm="http://www.w3.org/2001/04/xmlenc#sha256"/>
                  <dsig:DigestValue></dsig:DigestValue>
                </dsig:Reference>
              </dsig:SignedInfo>
              <dsig:SignatureValue></dsig:SignatureValue>
              <dsig:KeyInfo>
                <dsig:X509Data>
                  <dsig:X509Certificate>{cert}</dsig:X509Certificate>
                </dsig:X509Data>
              </dsig:KeyInfo>
            </dsig:Signature>"##
        );
        let resolve_id = wire.resolve_id;
        let artifact_response = format!(
            r#"
            <samlp:ArtifactResponse xmlns:samlp="{NS_SAMLP}" 
                                    xmlns:saml="{NS_SAML}" 
                                    ID="_artifactresponse1" 
                                    Version="2.0" 
                                    IssueInstant="{issued}" 
                                    InResponseTo="{resolve_id}">
              <saml:Issuer>{RD}</saml:Issuer>
              {signature}
              <samlp:Status>
                <samlp:StatusCode Value="{STATUS_SUCCESS}"/>
              </samlp:Status>
              {response}
            </samlp:ArtifactResponse>"#
        );
        let signed = sign(&artifact_response, &rd_key.key_pem).expect("sign as the RD");
        wrap_in_soap_envelope(&signed).expect("SOAP envelope")
    }

    /// Run the handler's validation chain over `soap`, exactly as
    /// `resolve_artifact_to_claims` does once the back-channel has answered.
    fn chain_claims(auth: &AuthServiceState, soap: &str) -> Result<Claims, Rejection> {
        let doc = parse_soap_envelope(soap).map_err(Rejection::Unanswered)?;
        let rd = auth.rd_metadata().expect("test RD metadata");
        let chain = ResponseChain {
            doc: &doc,
            auth_state: auth,
            rd: rd.as_ref(),
        };
        chain.claims(
            &MessageId::parse(RESOLVE_ID).expect("test resolve id"),
            &MessageId::parse(BOUND_ID).expect("test bound id"),
        )
    }

    /// Build the RD's answer for `wire` and run the chain over it.
    async fn run_wire(wire: Wire) -> Result<Claims, Rejection> {
        let auth = state_with_rd().await;
        let soap = soap_artifact_response(&wire, auth.dv_keys());
        chain_claims(&auth, &soap)
    }

    #[tokio::test]
    async fn a_valid_artifact_response_yields_the_assertions_claims() {
        let claims = run_wire(Wire::default()).await.expect("the chain accepts");

        let acting = claims.acting_subject_id.expect("ActingSubjectID decrypted");
        assert_eq!(acting.value.expose_secret(), BSN);
        assert_eq!(claims.name_id.as_str(), "transient-subject");
        assert_eq!(claims.service_uuid.as_deref(), Some(SERVICE_UUID));
        assert_eq!(claims.authn_context_class_ref.as_deref(), Some(LOA));
        assert_eq!(
            claims.in_response_to.as_ref().map(MessageId::as_str),
            Some(BOUND_ID)
        );
    }

    #[tokio::test]
    async fn a_cancelled_login_is_an_answered_failure() {
        // The RD answered this browser's own flow, so the session ends (TVS T3).
        assert_eq!(
            run_wire(Wire {
                outcome: Outcome::Cancelled,
                ..Wire::default()
            })
            .await
            .err(),
            Some(Rejection::Answered(AuthFailure::Cancelled))
        );
    }

    #[tokio::test]
    async fn an_rd_error_status_is_an_answered_failure() {
        // Same flow, but not a cancellation: the generic error page (TVS L10).
        assert_eq!(
            run_wire(Wire {
                outcome: Outcome::Failed,
                ..Wire::default()
            })
            .await
            .err(),
            Some(Rejection::Answered(AuthFailure::Error))
        );
    }

    #[tokio::test]
    async fn a_response_to_another_flow_never_ends_this_session() {
        // A forced-login attempt: an RD-signed Response for someone else's
        // AuthnRequest, handed to this browser. Rejected *before* the failure
        // counts as answered, so a link cannot log the user out.
        assert_eq!(
            run_wire(Wire {
                response_in_response_to: "_theirs",
                ..Wire::default()
            })
            .await
            .err(),
            Some(Rejection::Unanswered(AuthFailure::Error))
        );
    }

    #[tokio::test]
    async fn an_assertion_spliced_into_another_flows_response_is_rejected() {
        // The Response names this browser's AuthnRequest but the assertion
        // inside it answers a different one (eID §7.6.2 gives both cardinality
        // 1 for the same request), so the two must agree.
        assert_eq!(
            run_wire(Wire {
                assertion_in_response_to: "_theirs",
                ..Wire::default()
            })
            .await
            .err(),
            Some(Rejection::Answered(AuthFailure::Error))
        );
    }

    #[tokio::test]
    async fn an_artifact_response_answering_another_resolve_is_rejected() {
        // eID §7.6.1: the ArtifactResponse must answer the ArtifactResolve we
        // just sent, so a replayed one for an earlier resolve gets no further.
        assert_eq!(
            run_wire(Wire {
                resolve_id: "_someotherresolve",
                ..Wire::default()
            })
            .await
            .err(),
            Some(Rejection::Unanswered(AuthFailure::Error))
        );
    }

    #[tokio::test]
    async fn an_assertion_minted_for_another_dv_is_an_answered_failure() {
        // eID §7.6.3.5 rule 5: the assertion must name this DV in its
        // AudienceRestriction. The Response was RD-signed and answers this
        // browser's own flow, so the failed login does end the session (L10).
        assert_eq!(
            run_wire(Wire {
                audience: "urn:test:another-dv",
                ..Wire::default()
            })
            .await
            .err(),
            Some(Rejection::Answered(AuthFailure::Error))
        );
    }

    #[tokio::test]
    async fn a_response_signed_by_anyone_but_the_rd_is_rejected() {
        // Signed with a well-formed key that is simply not in the RD metadata
        // (eID §9.2: verification keys come from verified metadata only).
        assert_eq!(
            run_wire(Wire {
                signing_key: "dv-signing-1",
                ..Wire::default()
            })
            .await
            .err(),
            Some(Rejection::Unanswered(AuthFailure::Error))
        );
    }

    #[tokio::test]
    async fn a_back_channel_body_that_is_not_a_soap_artifact_response_is_rejected() {
        let auth = state_with_rd().await;
        // Well-formed XML, but no SOAP envelope to unwrap.
        assert_eq!(
            chain_claims(&auth, "<html><body>proxy error</body></html>").err(),
            Some(Rejection::Unanswered(AuthFailure::Error))
        );
        // Not XML at all: rejected at the single parse.
        assert_eq!(
            chain_claims(&auth, "502 Bad Gateway").err(),
            Some(Rejection::Unanswered(AuthFailure::Error))
        );
    }

    // -----------------------------------------------------------------------
    // eID §7.6.3.5 rule 4 / §9.7: match-and-consume against the application's
    // pending-request store.
    // -----------------------------------------------------------------------

    /// Claims that carry only what `confirm_pending_request` reads.
    fn claims_answering(in_response_to: Option<&str>) -> Claims {
        Claims {
            name_id: crate::types::NameId::parse("transient-subject").expect("test NameID"),
            authn_context_class_ref: None,
            authenticating_authority: None,
            acting_subject_id: None,
            legal_subject_id: None,
            service_uuid: None,
            in_response_to: in_response_to.map(|id| MessageId::parse(id).expect("test message id")),
        }
    }

    #[tokio::test]
    async fn an_outstanding_authn_request_is_confirmed_once_and_then_consumed() {
        let mock = MockAuthState::empty().with_pending(BOUND_ID);
        let bound = MessageId::parse(BOUND_ID).expect("test message id");
        let claims = claims_answering(Some(BOUND_ID));

        assert!(confirm_pending_request(&mock, &bound, &claims).await);
        // Consumed in the same step, so replaying the assertion finds nothing.
        assert!(
            !confirm_pending_request(&mock, &bound, &claims).await,
            "a replayed assertion must not be accepted a second time"
        );
    }

    #[tokio::test]
    async fn an_assertion_is_confirmed_only_against_this_browsers_request() {
        let mock = MockAuthState::empty().with_pending(BOUND_ID);
        let bound = MessageId::parse(BOUND_ID).expect("test message id");

        // Nothing to correlate: fails closed.
        assert!(!confirm_pending_request(&mock, &bound, &claims_answering(None)).await);
        // An assertion for a flow this browser did not start, even one the
        // store still holds: login CSRF / forced login.
        let mock = mock.with_pending("_theirs");
        assert!(!confirm_pending_request(&mock, &bound, &claims_answering(Some("_theirs"))).await);
        // And an ID the store never held, even though it is the bound one.
        let unknown = MessageId::parse("_unknown").expect("test message id");
        assert!(
            !confirm_pending_request(&mock, &unknown, &claims_answering(Some("_unknown"))).await
        );
    }
}
