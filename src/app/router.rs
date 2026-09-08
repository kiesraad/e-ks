//! Builds the application Axum router and wires feature routes.
//! Used by the server startup to assemble all routes.

use auth_service::SamlLogoutPath;
use axum::{
    Router,
    http::{HeaderName, HeaderValue, header},
    middleware,
    routing::get,
};
use axum_extra::routing::TypedPath;
use tower_http::{csrf::CsrfLayer, set_header::SetResponseHeaderLayer};

use crate::{
    AppState, audit_log, candidate_lists, candidates, common, csb, csb_store_middleware,
    db_gate_middleware, eks_key_middleware, finalise, health_router, http_trace, lb_health_router,
    list_designation, list_submitters, name_authorisations, persons, political_groups,
    render_error_pages, session_middleware, store_middleware, substitute_list_submitters,
    utils::bag,
};

pub fn create(state: AppState) -> Router<AppState> {
    let app_router = app_feature_router();

    #[cfg(feature = "dev-features")]
    let dev_router = Router::new().route(
        crate::app::middleware::dev_login::DEV_LOGIN_PATH,
        get(crate::app::middleware::dev_login::dev_login),
    );

    let app_router = app_router
        .fallback(get(common::not_found))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            render_error_pages,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            store_middleware,
        ));

    let csb_router = csb_router(&state);

    // These routes need a session but NOT store middleware: select-election runs
    // before a stream_id is chosen, and /language must stay reachable for CSB
    // (committee) sessions that store_middleware redirects off app routes.
    // `bag::router()` joins them: scanning the embedded BAG database is too
    // expensive to expose anonymously, and it is only called from a logged-in
    // form. The session middleware also verifies the CSRF token of every mutating
    // request (see `auth::csrf_guard`), so no handler can forget the check.
    let app_router = app_router
        .merge(common::session_only_router())
        .merge(bag::router())
        .merge(csb_router)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            session_middleware,
        ));

    #[cfg(feature = "dev-features")]
    let router = Router::new().merge(dev_router).merge(app_router);

    #[cfg(not(feature = "dev-features"))]
    let router = app_router;

    let router = router.merge(public_router());

    let router = router
        .layer(middleware::from_fn_with_state(
            state.clone(),
            db_gate_middleware,
        ))
        .layer(http_trace::layer());

    // The health probe is polled continuously; merging it
    // after the trace layer keeps those requests out of the request log
    let router = router.merge(health_router());

    #[cfg(feature = "livereload")]
    let router = router.merge(crate::utils::livereload::livereload_router());

    let router = mount_static_assets(router).merge(common::always_public_router());

    let router = router
        .layer(csrf_layer())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            eks_key_middleware,
        ));

    // The load balancer only checks that this process is up, and holds no
    // `x-eks-key`: its probe is merged last so it sits outside that gate.
    let router = router.merge(lb_health_router());

    // The CA's http-01 validators hold no `x-eks-key` either: merged outside
    // that gate the same way.
    #[cfg(feature = "acme")]
    let router = router.merge(crate::acme::acme_challenge_router());

    apply_security_headers(router)
}

/// Global fetch-metadata CSRF protection, backstopping the session
/// middleware's token check. SAML single logout is exempt (cross-site IdP
/// POSTs, signature-validated); the typed path is shared with the route
/// registration in `auth_service::router`, so route and bypass cannot drift.
fn csrf_layer() -> CsrfLayer {
    CsrfLayer::new().with_insecure_bypass(|_, uri| uri.path() == SamlLogoutPath::PATH)
}

/// Routes mounted outside the session middleware (no session required): the
/// SAML auth-service endpoints, the PG login and logged-out pages, and the
/// CSB GitHub login.
fn public_router() -> Router<AppState> {
    auth_service::router()
        .merge(common::public_router())
        .merge(csb::login::public_router())
}

/// The application's feature routes (everything that sits behind the session
/// and store middleware). Kept separate from [`create`] so the wiring of
/// global layers stays readable.
/// CSB routes need the session plus their own (CSB) store middleware, which
/// also gates them to committee-scoped sessions. They must NOT get the app
/// `store_middleware`, so they are merged into the session layer separately.
///
/// Errors are rendered in the CSB layout by `render_csb_error_pages`, the CSB
/// counterpart of `render_error_pages`. `csb::common::router()` claims every
/// other path under `/csb` for the CSB not-found page: the app router's
/// fallback would otherwise catch those, and its store middleware redirects
/// committee sessions away instead of answering with a page.
fn csb_router(state: &AppState) -> Router<AppState> {
    csb::index::router()
        .merge(csb::audit_log::router())
        .merge(csb::common::router())
        .merge(csb::examination::router())
        .merge(csb::recovery::router())
        .merge(csb::import::router())
        .merge(csb::monitoring::router())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            csb::render_csb_error_pages,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            csb_store_middleware,
        ))
}

fn app_feature_router() -> Router<AppState> {
    Router::new()
        .merge(audit_log::router())
        .merge(candidates::router())
        .merge(candidate_lists::router())
        .merge(common::router())
        .merge(list_designation::router())
        .merge(list_submitters::router())
        .merge(name_authorisations::router())
        .merge(persons::router())
        .merge(political_groups::router())
        .merge(finalise::router())
        .merge(substitute_list_submitters::router())
}

/// Deny every powerful browser feature; the app uses none. The names are the
/// union of the directives listed on MDN
/// (<https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Permissions-Policy>)
/// and in the W3C feature registry
/// (<https://github.com/w3c/webappsec-permissions-policy/blob/main/features.md>).
/// Unknown directives are ignored, so naming retired and proposed features
/// costs nothing.
const PERMISSIONS_POLICY: &str = concat!(
    "accelerometer=(), ambient-light-sensor=(), attribution-reporting=(), autoplay=(), ",
    "battery=(), bluetooth=(), browsing-topics=(), camera=(), captured-surface-control=(), ",
    "clipboard-read=(), clipboard-write=(), compute-pressure=(), deferred-fetch=(), ",
    "deferred-fetch-minimal=(), digital-credentials-get=(), display-capture=(), ",
    "document-domain=(), encrypted-media=(), fullscreen=(), gamepad=(), geolocation=(), ",
    "gyroscope=(), hid=(), identity-credentials-get=(), idle-detection=(), interest-cohort=(), ",
    "join-ad-interest-group=(), keyboard-map=(), local-fonts=(), magnetometer=(), microphone=(), ",
    "midi=(), otp-credentials=(), payment=(), picture-in-picture=(), private-aggregation=(), ",
    "private-state-token-issuance=(), private-state-token-redemption=(), ",
    "publickey-credentials-create=(), publickey-credentials-get=(), run-ad-auction=(), ",
    "screen-wake-lock=(), serial=(), speaker-selection=(), storage-access=(), sync-xhr=(), ",
    "unload=(), usb=(), web-share=(), window-management=(), xr-spatial-tracking=()",
);

/// Fetch directives `default-src 'none'` already covers are spelled out anyway,
/// so a change to a browser's fallback chain cannot widen the policy. Trusted
/// Types is enforced because the bundle assigns to no injection sink.
///
/// `form-action 'self'` holds for every page but one: the SAML auto-submit
/// page POSTs to the IdP endpoint from the verified RD metadata, so it sets
/// its own policy (`auth_service::bindings::http_post::autosubmit_csp`) that
/// widens `form-action` by exactly that URL.
const CONTENT_SECURITY_POLICY: &str = concat!(
    "default-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'; ",
    "script-src 'self'; style-src 'self'; img-src 'self'; font-src 'self'; connect-src 'self'; ",
    "object-src 'none'; media-src 'none'; frame-src 'none'; child-src 'none'; ",
    "worker-src 'none'; manifest-src 'none'; ",
    "require-trusted-types-for 'script'; trusted-types 'none'",
);

/// Static security response headers shared by every response. HSTS is added
/// separately on the TLS listener (see `core::server`), only over https.
fn apply_security_headers(mut router: Router<AppState>) -> Router<AppState> {
    let headers: [(HeaderName, &'static str); 9] = [
        (header::X_FRAME_OPTIONS, "DENY"),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        // Not `no-referrer`: the language switch and the download-warning
        // dismissal navigate back to the referring page.
        (header::REFERRER_POLICY, "same-origin"),
        (
            HeaderName::from_static("cross-origin-opener-policy"),
            "same-origin",
        ),
        (
            HeaderName::from_static("cross-origin-resource-policy"),
            "same-origin",
        ),
        // Every subresource is same-origin, so requiring an opt-in from
        // cross-origin ones costs nothing.
        (
            HeaderName::from_static("cross-origin-embedder-policy"),
            "require-corp",
        ),
        (
            HeaderName::from_static("x-permitted-cross-domain-policies"),
            "none",
        ),
        (HeaderName::from_static("origin-agent-cluster"), "?1"),
        (
            HeaderName::from_static("permissions-policy"),
            PERMISSIONS_POLICY,
        ),
    ];

    for (name, value) in headers {
        // `overriding`, not `if_not_present`: no handler may weaken these.
        router = router.layer(SetResponseHeaderLayer::overriding(
            name,
            HeaderValue::from_static(value),
        ));
    }

    router.layer(SetResponseHeaderLayer::if_not_present(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CONTENT_SECURITY_POLICY),
    ))
}

/// `Cache-Control: no-store` for every dynamic response: pages carry personal
/// data and must reach neither a shared cache nor a browser's disk cache.
/// `if_not_present`, so the download handlers keep their own value.
fn apply_no_store(router: Router<AppState>) -> Router<AppState> {
    router.layer(SetResponseHeaderLayer::if_not_present(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    ))
}

/// Mount the cache-busted `/static` asset routes: served from the embedded
/// bundle in release builds, proxied to the dev asset server otherwise.
///
/// [`apply_no_store`] runs first, so these cache-busted assets stay cacheable
/// while every page above them does not.
fn mount_static_assets(router: Router<AppState>) -> Router<AppState> {
    let router = apply_no_store(router);
    let code = crate::filters::cache_buster();
    let index_js = format!("/{code}-index.js");
    let index_css = format!("/{code}-index.css");

    #[cfg(feature = "memory-serve")]
    let router = {
        let memory_serve = memory_serve::load!()
            .index_file(None)
            .add_alias(index_js.leak(), "/index.js")
            .add_alias(index_css.leak(), "/index.css");

        router.nest("/static", memory_serve.into_router())
    };

    #[cfg(not(feature = "memory-serve"))]
    let router = router.nest(
        "/static",
        Router::new().fallback(crate::proxy_handler(
            "http://localhost:8888",
            vec![
                (index_js, "/index.js".to_string()),
                (index_css, "/index.css".to_string()),
            ],
        )),
    );

    router
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode, header},
    };
    use tower::ServiceExt;

    use crate::{AppState, test_utils::response_body_string};

    #[tokio::test]
    async fn index_route_renders_index() {
        let state = AppState::new_for_tests().await;
        let app: Router = create(state.clone()).with_state(state.clone());

        let mut request = Request::builder().uri("/").body(Body::empty()).unwrap();
        let mut session = crate::Session::new_test();
        session.set_test_election(crate::ElectionConfig::EK27);
        let token = session.token_string();
        state.sessions.insert(session).await;
        let store = crate::PgStore::new_for_test();
        request.headers_mut().insert(
            header::COOKIE,
            format!("{}={}", crate::SESSION_COOKIE_NAME, token)
                .parse()
                .unwrap(),
        );
        request.extensions_mut().insert(store);
        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Kiesraad - Kandidaatstelling"));
    }

    /// Insert a committee session and build a GET request for `uri` that
    /// carries its cookie.
    async fn committee_request(state: &AppState, uri: &str) -> Request<Body> {
        let session = crate::Session::new_test_committee();
        let token = session.token_string();
        state.sessions.insert(session).await;

        Request::builder()
            .uri(uri)
            .header(
                header::COOKIE,
                format!("{}={}", crate::SESSION_COOKIE_NAME, token),
            )
            .body(Body::empty())
            .unwrap()
    }

    /// An unknown path under `/csb` gets the CSB not-found page, not the app
    /// router's fallback (which would redirect the committee session).
    #[tokio::test]
    async fn unknown_csb_path_renders_csb_not_found_page() {
        let state = AppState::new_for_tests().await;
        let app: Router = create(state.clone()).with_state(state.clone());

        let request = committee_request(&state, "/csb/does-not-exist").await;
        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = response_body_string(response).await;
        assert!(body.contains("Pagina niet gevonden"), "{body}");
        assert!(body.contains("/csb/does-not-exist"), "{body}");
        assert!(body.contains("href=\"/csb\""), "{body}");
    }

    /// An error from a CSB handler or extractor is rendered as a page in the
    /// CSB layout rather than answered with a bare status code.
    #[tokio::test]
    async fn csb_handler_error_renders_csb_error_page() {
        let state = AppState::new_for_tests().await;
        let app: Router = create(state.clone()).with_state(state.clone());

        let uri = format!("/csb/examination/{}", crate::StreamId::new());
        let request = committee_request(&state, &uri).await;
        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = response_body_string(response).await;
        assert!(body.contains("Foutcode 404"), "{body}");
        assert!(body.contains("Stream not found"), "{body}");
        assert!(body.contains("href=\"/csb\""), "{body}");
    }

    /// The model download routes are static segments under the same prefix as
    /// `/csb/examination/{stream_id}`; they must resolve to their own handlers
    /// rather than being parsed as a (bogus) stream id.
    #[tokio::test]
    async fn model_download_routes_win_over_the_political_group_route() {
        let state = AppState::new_for_tests().await;

        for (uri, content_type) in [
            ("/csb/examination/i1.pdf", "application/pdf"),
            (
                "/csb/examination/i1.docx",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ),
            ("/csb/examination/i4.pdf", "application/pdf"),
        ] {
            let app: Router = create(state.clone()).with_state(state.clone());
            let request = committee_request(&state, uri).await;
            let response = app.oneshot(request).await.expect("response");

            assert_eq!(response.status(), StatusCode::OK, "{uri}");
            assert_eq!(
                response.headers().get(header::CONTENT_TYPE).expect("type"),
                content_type,
                "{uri}"
            );
        }
    }

    #[tokio::test]
    async fn db_gate_serves_maintenance_when_unhealthy() {
        let state = AppState::new_for_tests().await;
        // Simulate the prober (or a request handler) tripping the gate.
        state.db_health.mark_unavailable("test: database down");

        let app: Router = create(state.clone()).with_state(state.clone());

        let request = Request::builder().uri("/").body(Body::empty()).unwrap();
        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers().get("retry-after").unwrap(), "30");
        let body = response_body_string(response).await;
        assert!(body.contains("Tijdelijk niet beschikbaar"));
    }

    #[tokio::test]
    async fn db_gate_exempts_health_endpoint_when_unhealthy() {
        let state = AppState::new_for_tests().await;
        state.db_health.mark_unavailable("test: database down");

        let app: Router = create(state.clone()).with_state(state.clone());

        let request = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.expect("response");

        // The health probe must keep answering (memory backend is reachable),
        // not be swallowed by the maintenance gate.
        assert_eq!(response.status(), StatusCode::OK);
    }

    /// The security headers are applied outermost, so they must also cover
    /// short-circuit responses from outer middleware (the maintenance page)
    /// and routes merged after the app router (the load balancer probe).
    #[tokio::test]
    async fn security_headers_cover_short_circuits_and_late_merged_routes() {
        let state = AppState::new_for_tests().await;
        state.db_health.mark_unavailable("test: database down");
        let app: Router = create(state.clone()).with_state(state.clone());

        for uri in ["/", crate::app::middleware::health::LbHealthPath::PATH] {
            let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
            let response = app.clone().oneshot(request).await.expect("response");

            for name in [
                "content-security-policy",
                "x-frame-options",
                "x-content-type-options",
                "referrer-policy",
                "cross-origin-opener-policy",
                "cross-origin-resource-policy",
                "cross-origin-embedder-policy",
                "x-permitted-cross-domain-policies",
                "origin-agent-cluster",
                "permissions-policy",
            ] {
                assert!(
                    response.headers().contains_key(name),
                    "{uri} response must carry {name}"
                );
            }
        }
    }

    #[tokio::test]
    async fn autosubmit_response_keeps_its_own_csp() {
        use auth_service::{EndpointUrl, bindings::http_post::autosubmit_post_response};

        const IDP_URL: &str = "https://tvs-mock.example/kvs/rd/request_authentication";

        let state = AppState::new_for_tests().await;
        let router: Router<AppState> = apply_security_headers(Router::new().route(
            "/autosubmit-test",
            axum::routing::get(async || {
                autosubmit_post_response(
                    &EndpointUrl::from_metadata(IDP_URL, "SSO").unwrap(),
                    "<samlp:AuthnRequest/>",
                    "SAMLRequest",
                )
                .unwrap()
            }),
        ));
        let app: Router = router.with_state(state);

        let request = Request::builder()
            .uri("/autosubmit-test")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.expect("response");

        let csp = response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .expect("csp")
            .to_str()
            .unwrap();
        assert!(
            csp.contains(&format!("form-action 'self' {IDP_URL}")),
            "the per-response CSP must survive the global layer: {csp}"
        );
        // The rest of the security headers still come from the global layer.
        assert_eq!(
            response
                .headers()
                .get("cross-origin-opener-policy")
                .unwrap(),
            "same-origin"
        );
    }

    /// Pins the values of the headers that are easy to weaken by accident: the
    /// CSP must stay closed by default with no inline escape hatch, and the
    /// cross-origin trio must stay at its strictest setting.
    #[tokio::test]
    async fn security_header_values_stay_strict() {
        let state = AppState::new_for_tests().await;
        let app: Router = create(state.clone()).with_state(state.clone());

        let request = Request::builder()
            .uri("/login")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.expect("response");
        let headers = response.headers();

        let csp = headers
            .get(header::CONTENT_SECURITY_POLICY)
            .expect("csp")
            .to_str()
            .unwrap();
        for directive in [
            "default-src 'none'",
            "base-uri 'none'",
            "form-action 'self'",
            "frame-ancestors 'none'",
            "object-src 'none'",
            "require-trusted-types-for 'script'",
        ] {
            assert!(
                csp.contains(directive),
                "CSP must keep `{directive}`: {csp}"
            );
        }
        assert!(
            !csp.contains("unsafe-inline") && !csp.contains("unsafe-eval"),
            "CSP must not allow inline or eval: {csp}"
        );

        for (name, value) in [
            ("cross-origin-opener-policy", "same-origin"),
            ("cross-origin-resource-policy", "same-origin"),
            ("cross-origin-embedder-policy", "require-corp"),
            ("x-permitted-cross-domain-policies", "none"),
            ("origin-agent-cluster", "?1"),
            ("x-frame-options", "DENY"),
            ("x-content-type-options", "nosniff"),
        ] {
            assert_eq!(headers.get(name).unwrap(), value, "{name}");
        }

        // A missing feature name is a policy that silently allows it, so keep a
        // sample of the less obvious ones covered.
        let permissions = headers
            .get("permissions-policy")
            .expect("permissions policy")
            .to_str()
            .unwrap();
        for feature in [
            "browsing-topics=()",
            "camera=()",
            "clipboard-read=()",
            "document-domain=()",
            "geolocation=()",
            "microphone=()",
            "storage-access=()",
        ] {
            assert!(
                permissions.contains(feature),
                "Permissions-Policy must deny `{feature}`: {permissions}"
            );
        }
    }

    /// Pages hold personal data, so no cache may keep them; the cache-busted
    /// asset bundle is mounted outside that layer and stays cacheable.
    #[tokio::test]
    async fn dynamic_responses_are_not_stored_but_static_assets_stay_cacheable() {
        let state = AppState::new_for_tests().await;
        let app: Router = create(state.clone()).with_state(state.clone());

        for uri in ["/login", "/", "/missing"] {
            let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
            let response = app.clone().oneshot(request).await.expect("response");

            assert_eq!(
                response.headers().get(header::CACHE_CONTROL).unwrap(),
                "no-store",
                "{uri} must not be stored by any cache"
            );
        }

        let request = Request::builder()
            .uri("/static/index.js")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.expect("response");
        assert_ne!(
            response
                .headers()
                .get(header::CACHE_CONTROL)
                .map(|v| v.to_str().unwrap().to_string()),
            Some("no-store".to_string()),
            "cache-busted assets must stay cacheable"
        );
    }

    /// Signing out must also clear what the session left in the browser.
    #[tokio::test]
    async fn logged_out_page_clears_site_data() {
        let state = AppState::new_for_tests().await;
        let app: Router = create(state.clone()).with_state(state.clone());

        let request = Request::builder()
            .uri("/logged-out")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("clear-site-data").unwrap(),
            "\"cache\", \"storage\""
        );
    }

    /// The load balancer has no `x-eks-key`, so its probe must answer from
    /// behind a configured gate that rejects `/health`.
    #[tokio::test]
    async fn lb_health_answers_without_eks_key() {
        let mut config = crate::Config::new_test();
        config.eks_key = Some(secrecy::SecretString::from("s3cret"));
        let state = AppState::new_for_tests_with_config(config).await;
        let app: Router = create(state.clone()).with_state(state.clone());

        let request = Request::builder()
            .uri(crate::app::middleware::health::LbHealthPath::PATH)
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_body_string(response).await, "started");

        let request = Request::builder()
            .uri(crate::app::middleware::health::HealthPath::PATH)
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    /// A cross-site POST is rejected by the global CSRF layer before it
    /// reaches any session or handler logic.
    #[tokio::test]
    async fn cross_site_post_is_rejected_by_csrf_layer() {
        let state = AppState::new_for_tests().await;
        let app: Router = create(state.clone()).with_state(state.clone());

        let request = Request::builder()
            .method("POST")
            .uri("/switch-election")
            .header("sec-fetch-site", "cross-site")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    /// A same-origin POST passes the CSRF layer: without a session cookie it
    /// reaches the session middleware, which redirects to login.
    #[tokio::test]
    async fn same_origin_post_passes_csrf_layer() {
        let state = AppState::new_for_tests().await;
        let app: Router = create(state.clone()).with_state(state.clone());

        let request = Request::builder()
            .method("POST")
            .uri("/switch-election")
            .header("sec-fetch-site", "same-origin")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
    }

    /// The SAML single-logout endpoint receives cross-site POSTs from the IdP
    /// by design and must bypass the CSRF layer (it validates SAML signatures
    /// instead).
    #[tokio::test]
    async fn cross_site_post_to_saml_logout_bypasses_csrf_layer() {
        let state = AppState::new_for_tests().await;
        let app: Router = create(state.clone()).with_state(state.clone());

        let request = Request::builder()
            .method("POST")
            .uri(SamlLogoutPath::PATH)
            .header("sec-fetch-site", "cross-site")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.expect("response");

        assert_ne!(response.status(), StatusCode::FORBIDDEN);
    }

    /// The BAG address lookup is expensive to answer, so it must sit behind the
    /// session middleware rather than being reachable anonymously.
    #[tokio::test]
    async fn bag_lookup_requires_a_session() {
        let state = AppState::new_for_tests().await;
        let app: Router = create(state.clone()).with_state(state.clone());

        for uri in ["/lookup?pc=1234AB&n=1", "/suggest?wp=Amsterdam"] {
            let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
            let response = app.clone().oneshot(request).await.expect("response");

            assert_eq!(
                response.status(),
                StatusCode::SEE_OTHER,
                "{uri} must not answer without a session"
            );
            assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/login");
        }
    }

    /// robots.txt and security.txt must answer requests without a session
    /// cookie (they are served to anonymous visitors through the CDN).
    #[tokio::test]
    async fn robots_and_security_txt_need_no_session() {
        let state = AppState::new_for_tests().await;
        let app: Router = create(state.clone()).with_state(state.clone());

        for (uri, expected) in [
            ("/robots.txt", "Disallow: /"),
            ("/.well-known/security.txt", "Contact: mailto:"),
        ] {
            let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
            let response = app.clone().oneshot(request).await.expect("response");

            assert_eq!(response.status(), StatusCode::OK, "{uri} must be public");
            let body = response_body_string(response).await;
            assert!(body.contains(expected), "{uri} body: {body}");
        }
    }

    /// The challenge route must answer without an `x-eks-key` even when the
    /// gate is configured.
    #[cfg(feature = "acme")]
    #[tokio::test]
    async fn acme_challenge_answers_without_eks_key() {
        let mut config = crate::Config::new_test();
        config.eks_key = Some(secrecy::SecretString::from("s3cret"));
        let state = AppState::new_for_tests_with_config(config).await;
        state
            .acme_store
            .put_challenge("tok", "tok.thumbprint")
            .await
            .unwrap();
        let app: Router = create(state.clone()).with_state(state.clone());

        let request = Request::builder()
            .uri("/.well-known/acme-challenge/tok")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_body_string(response).await, "tok.thumbprint");

        let request = Request::builder()
            .uri("/.well-known/acme-challenge/unknown")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    /// The eks-key gate guards robots.txt and security.txt like every other
    /// route: it exists so only our CDN can reach the server directly.
    #[tokio::test]
    async fn robots_and_security_txt_stay_behind_eks_key_gate() {
        let mut config = crate::Config::new_test();
        config.eks_key = Some(secrecy::SecretString::from("s3cret"));
        let state = AppState::new_for_tests_with_config(config).await;
        let app: Router = create(state.clone()).with_state(state.clone());

        for uri in ["/robots.txt", "/.well-known/security.txt"] {
            let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
            let response = app.clone().oneshot(request).await.expect("response");

            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{uri}");
        }
    }

    #[tokio::test]
    async fn fallback_route_renders_not_found() {
        let state = AppState::new_for_tests().await;
        let app: Router = create(state.clone()).with_state(state.clone());

        let mut request = Request::builder()
            .uri("/missing")
            .body(Body::empty())
            .unwrap();
        let mut session = crate::Session::new_test();
        session.set_test_election(crate::ElectionConfig::EK27);
        let token = session.token_string();
        state.sessions.insert(session).await;
        let store = crate::PgStore::new_for_test();
        request.headers_mut().insert(
            header::COOKIE,
            format!("{}={}", crate::SESSION_COOKIE_NAME, token)
                .parse()
                .unwrap(),
        );
        request.extensions_mut().insert(store);
        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = response_body_string(response).await;
        assert!(body.contains("Pagina niet gevonden"));
    }
}
