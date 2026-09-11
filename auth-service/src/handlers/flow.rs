//! Browser-binding of the SSO flow to defeat login-CSRF / forced login.
//!
//! `/login` sets a short-lived cookie holding the AuthnRequest ID plus a hash
//! of the browser's `User-Agent`; `GET /saml/sp/acs` requires that cookie before
//! it resolves the artifact, and its ID to equal the validated assertion's
//! `InResponseTo` before a session is created. Because the cookie cannot be set
//! on another browser cross-origin, an attacker cannot make a victim's browser
//! complete a flow the attacker started (the assertion's `InResponseTo` would
//! still be outstanding, but the victim's browser carries no matching cookie).
//!
//! This lives entirely in the auth-service crate and needs no
//! embedding-application API: both `handle_login` and `handle_acs` already
//! receive the [`CookieJar`] and request [`HeaderMap`].
//!
//! Note: only one SSO flow per browser is in flight at a time (a second `/login`
//! overwrites the cookie); completing the older flow then fails closed and the
//! user simply re-authenticates. That is acceptable for an SSO entry point.

use crate::types::{EndpointUrl, MessageId};
use axum::http::{HeaderMap, header::USER_AGENT};
use axum_extra::extract::{
    CookieJar,
    cookie::{Cookie, SameSite},
};
use sha2::{Digest, Sha256};

/// Cookie name in https deployments. The `__Host-` prefix forbids a `Domain`
/// attribute and requires `Secure` + `Path=/`, so a sibling subdomain cannot
/// plant it (defeats cookie fixation of the binding value).
const FLOW_COOKIE_HOST: &str = "__Host-eks-saml-flow";
/// Cookie name for plain-http local development: the `__Host-` prefix requires
/// `Secure`, which a browser refuses to honor over http.
const FLOW_COOKIE_DEV: &str = "eks-saml-flow";

/// Lifetime of the binding cookie. Derived from the pending-request window
/// ([`crate::PENDING_REQUEST_TTL`]) so an abandoned flow's cookie does not linger.
const FLOW_COOKIE_TTL_MINUTES: i64 = (crate::PENDING_REQUEST_TTL.as_secs() / 60) as i64;

fn cookie_name(secure: bool) -> &'static str {
    if secure {
        FLOW_COOKIE_HOST
    } else {
        FLOW_COOKIE_DEV
    }
}

/// Short (64-bit) hex hash of the request `User-Agent`. Pins the flow to the
/// browser's UA without storing the raw header; a missing UA hashes the empty
/// string (still consistent between `/login` and the ACS callback).
fn ua_hash(headers: &HeaderMap) -> String {
    let ua = headers
        .get(USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let digest = Sha256::digest(ua.as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// The bound cookie value: the AuthnRequest ID and the User-Agent hash. The ID
/// is a [`MessageId`], i.e. an XML `NCName`, so it cannot carry the `;` or `,`
/// that would break out of the cookie value.
fn bound_value(authn_id: &MessageId, headers: &HeaderMap) -> String {
    format!("{authn_id}.{}", ua_hash(headers))
}

/// Build the `Set-Cookie` that binds an SSO flow to this browser, set by
/// `handle_login`. `acs_url` selects the cookie name and `Secure` flag.
pub(crate) fn flow_cookie(
    acs_url: &EndpointUrl,
    authn_id: &MessageId,
    headers: &HeaderMap,
) -> Cookie<'static> {
    let secure = acs_url.is_https();
    Cookie::build((cookie_name(secure), bound_value(authn_id, headers)))
        .http_only(true)
        .secure(secure)
        // Lax (not Strict): the RD returns the artifact via a top-level cross-site
        // GET redirect to the ACS, and Lax sends the cookie on exactly that.
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(cookie::time::Duration::minutes(FLOW_COOKIE_TTL_MINUTES))
        .build()
}

/// One-shot marker: the failed callback ended a flow this browser started, so
/// the error landing may end the local session (TVS L10).
const FAILED_FLOW_COOKIE_HOST: &str = "__Host-eks-saml-failed";
const FAILED_FLOW_COOKIE_DEV: &str = "eks-saml-failed";

fn failed_flow_cookie_name(secure: bool) -> &'static str {
    if secure {
        FAILED_FLOW_COOKIE_HOST
    } else {
        FAILED_FLOW_COOKIE_DEV
    }
}

/// Only has to survive the redirect to the error landing.
pub(crate) fn failed_flow_cookie(acs_url: &EndpointUrl) -> Cookie<'static> {
    let secure = acs_url.is_https();
    Cookie::build((failed_flow_cookie_name(secure), "1"))
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(cookie::time::Duration::minutes(1))
        .build()
}

/// Whether the marker is present, plus the jar with it removed. Checks both
/// names, so the error landing needs no deployment knowledge.
pub(crate) fn take_failed_flow(jar: CookieJar) -> (bool, CookieJar) {
    let mut present = false;
    let mut jar = jar;
    for secure in [true, false] {
        let name = failed_flow_cookie_name(secure);
        if jar.get(name).is_some() {
            present = true;
            let removal = Cookie::build((name, "")).path("/").secure(secure).build();
            jar = jar.remove(removal);
        }
    }
    (present, jar)
}

/// The AuthnRequest ID this browser's flow cookie is bound to, plus the jar with
/// the one-shot cookie removed. `None` when the cookie is absent, malformed, or
/// bound to a different `User-Agent`.
///
/// Only what the browser claims: the caller still matches it against the
/// assertion's `InResponseTo` and consumes it in the pending-request store.
/// Returning it rather than verifying an expected value is what lets the ACS run
/// this gate before resolving the artifact.
pub(crate) fn take_bound_authn_id(
    jar: CookieJar,
    acs_url: &EndpointUrl,
    headers: &HeaderMap,
) -> (Option<MessageId>, CookieJar) {
    let secure = acs_url.is_https();
    let name = cookie_name(secure);
    let bound = jar
        .get(name)
        .and_then(|c| parse_bound_value(c.value(), headers));
    // The removal cookie must carry the same Path (and Secure for the __Host-
    // prefix) the browser stored it with, or the browser keeps the original.
    let removal = Cookie::build((name, "")).path("/").secure(secure).build();
    (bound, jar.remove(removal))
}

/// Split a cookie value back into its AuthnRequest ID, requiring the trailing
/// User-Agent hash to match. Split on the *last* `.`: an `NCName` may contain
/// dots, the hex hash cannot.
fn parse_bound_value(value: &str, headers: &HeaderMap) -> Option<MessageId> {
    let (authn_id, ua) = value.rsplit_once('.')?;
    if ua != ua_hash(headers) {
        return None;
    }
    MessageId::parse(authn_id).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers_with_ua(ua: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(USER_AGENT, ua.parse().unwrap());
        h
    }

    fn acs(url: &str) -> EndpointUrl {
        EndpointUrl::from_base_url(url, "ACS").expect("test ACS URL")
    }

    #[test]
    fn failed_flow_marker_is_taken_once() {
        let jar = CookieJar::new().add(failed_flow_cookie(&acs("https://dv.example/")));
        let (present, jar) = take_failed_flow(jar);
        assert!(present);
        assert!(jar.get(FAILED_FLOW_COOKIE_HOST).is_none());

        let (present, _) = take_failed_flow(CookieJar::new());
        assert!(!present);
    }

    fn id(value: &str) -> MessageId {
        MessageId::parse(value).expect("test message id")
    }

    fn jar_with(name: &str, value: &str) -> CookieJar {
        CookieJar::new().add(Cookie::build((name.to_string(), value.to_string())).build())
    }

    #[test]
    fn https_uses_host_prefixed_secure_cookie() {
        let h = headers_with_ua("agent/1");
        let c = flow_cookie(&acs("https://dv.example.com/saml/sp/acs"), &id("_abc"), &h);
        assert_eq!(c.name(), FLOW_COOKIE_HOST);
        assert_eq!(c.secure(), Some(true));
        assert_eq!(c.same_site(), Some(SameSite::Lax));
        assert_eq!(c.http_only(), Some(true));
        assert!(c.value().starts_with("_abc."));
    }

    #[test]
    fn http_dev_uses_plain_non_secure_cookie() {
        let h = headers_with_ua("agent/1");
        let c = flow_cookie(&acs("http://localhost:3000/saml/sp/acs"), &id("_abc"), &h);
        assert_eq!(c.name(), FLOW_COOKIE_DEV);
        assert_eq!(c.secure(), Some(false));
    }

    #[test]
    fn bound_id_is_recovered_for_the_same_browser_and_ua() {
        let h = headers_with_ua("agent/1");
        let acs = acs("https://dv.example.com/saml/sp/acs");
        // An ID containing a dot: the split must not cut it short.
        let value = flow_cookie(&acs, &id("_ab.c"), &h).value().to_string();
        let jar = jar_with(FLOW_COOKIE_HOST, &value);
        let (bound, jar) = take_bound_authn_id(jar, &acs, &h);
        assert_eq!(bound, Some(id("_ab.c")));
        // One-shot.
        assert!(jar.get(FLOW_COOKIE_HOST).is_none());
    }

    #[test]
    fn missing_cookie_yields_no_bound_id() {
        let h = headers_with_ua("agent/1");
        let acs = acs("https://dv.example.com/saml/sp/acs");
        let (bound, _) = take_bound_authn_id(CookieJar::new(), &acs, &h);
        assert!(
            bound.is_none(),
            "absent flow cookie must be rejected (login CSRF)"
        );
    }

    #[test]
    fn bound_id_is_the_cookie_value_not_the_assertions() {
        // Models forced login: the victim's browser carries a cookie for a
        // different flow, so the ID handed back cannot match the attacker's
        // assertion InResponseTo.
        let h = headers_with_ua("agent/1");
        let acs = acs("https://dv.example.com/saml/sp/acs");
        let value = flow_cookie(&acs, &id("_victim-request"), &h)
            .value()
            .to_string();
        let jar = jar_with(FLOW_COOKIE_HOST, &value);
        let (bound, _) = take_bound_authn_id(jar, &acs, &h);
        assert_eq!(bound, Some(id("_victim-request")));
        assert_ne!(bound, Some(id("_attacker")));
    }

    #[test]
    fn changed_user_agent_yields_no_bound_id() {
        let acs = acs("https://dv.example.com/saml/sp/acs");
        let value = flow_cookie(&acs, &id("_abc"), &headers_with_ua("agent/1"))
            .value()
            .to_string();
        let jar = jar_with(FLOW_COOKIE_HOST, &value);
        let (bound, _) = take_bound_authn_id(jar, &acs, &headers_with_ua("agent/2"));
        assert!(
            bound.is_none(),
            "a different User-Agent must not satisfy the binding"
        );
    }

    #[test]
    fn malformed_cookie_values_yield_no_bound_id() {
        let h = headers_with_ua("agent/1");
        let acs = acs("https://dv.example.com/saml/sp/acs");
        let hash = ua_hash(&h);
        for value in [
            String::new(),
            // No separator at all.
            "_abc".to_string(),
            // Empty ID, and an ID that is not an NCName.
            format!(".{hash}"),
            format!("1abc.{hash}"),
        ] {
            let (bound, _) = take_bound_authn_id(jar_with(FLOW_COOKIE_HOST, &value), &acs, &h);
            assert!(bound.is_none(), "must reject cookie value {value:?}");
        }
    }
}
