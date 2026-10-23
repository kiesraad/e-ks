//! Session cookie helpers and request extraction.

use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, header::USER_AGENT, request::Parts},
};
use axum_extra::extract::{
    CookieJar,
    cookie::{Cookie, SameSite},
};

use crate::{AppError, Session, SessionStore, utils::sha256_hex};

/// Name of the session cookie. The `__Host-` prefix (production only) forbids a
/// `Domain` and requires `Secure` + `Path=/`, blocking sibling-subdomain
/// shadowing. It mandates `Secure`, so dev over http keeps the bare name.
#[cfg(feature = "dev-features")]
pub const SESSION_COOKIE_NAME: &str = "EKS_SESSION_ID";
#[cfg(not(feature = "dev-features"))]
pub const SESSION_COOKIE_NAME: &str = "__Host-EKS_SESSION_ID";

/// Where the request that establishes a session comes from, which sets the
/// cookie's `SameSite`. A SAML login returns through a cross-site redirect
/// from the IdP, and the redirect that follows the ACS only carries a `Lax`
/// cookie; every other login is a same-site request and gets `Strict` at
/// once. The session middleware replaces a `Lax` cookie with its `Strict`
/// twin on the first request it serves (see [`Session::is_unused`]), so a
/// cross-site navigation never carries a session afterwards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoginOrigin {
    SameSite,
    CrossSite,
}

impl LoginOrigin {
    fn same_site(self) -> SameSite {
        match self {
            Self::SameSite => SameSite::Strict,
            Self::CrossSite => SameSite::Lax,
        }
    }
}

/// Cookie with the shared session cookie attributes applied.
fn build_cookie(name: &'static str, value: String, same_site: SameSite) -> Cookie<'static> {
    let mut cookie = Cookie::new(name, value);
    apply_session_cookie_attributes(&mut cookie, same_site);
    cookie
}

/// Expired twin of a cookie for clearing it. Attributes must match the set
/// cookie (esp. `Secure` + `Path=/` for the `__Host-` prefix) or it lingers.
fn build_expired_cookie(name: &'static str) -> Cookie<'static> {
    let mut cookie = Cookie::from(name);
    apply_session_cookie_attributes(&mut cookie, SameSite::Strict);
    cookie
}

/// Builds the session cookie. Only valid right after creation, while the raw
/// token is still in memory.
pub(crate) fn build_session_cookie(session: &Session, origin: LoginOrigin) -> Cookie<'static> {
    let token = session
        .reveal_token()
        .expect("build_session_cookie requires a freshly created session with its raw token");
    build_cookie(
        SESSION_COOKIE_NAME,
        token.to_exposed_string(),
        origin.same_site(),
    )
}

/// The `Strict` twin of a session cookie the browser presented, replacing
/// the `Lax` one a cross-site login handed over.
pub(crate) fn build_strict_session_cookie(token: &str) -> Cookie<'static> {
    build_cookie(SESSION_COOKIE_NAME, token.to_string(), SameSite::Strict)
}

/// Expired twin of the session cookie for clearing it.
pub(crate) fn build_removal_cookie() -> Cookie<'static> {
    build_expired_cookie(SESSION_COOKIE_NAME)
}

fn apply_session_cookie_attributes(cookie: &mut Cookie<'static>, same_site: SameSite) {
    cookie.set_http_only(true);
    #[cfg(feature = "dev-features")]
    cookie.set_secure(false);
    #[cfg(not(feature = "dev-features"))]
    cookie.set_secure(true);
    cookie.set_same_site(same_site);
    cookie.set_path("/");
}

/// Establishes a freshly created session, shared by every login flow: drops
/// any session the browser already holds (session-fixation defence), sweeps
/// expired sessions, stores the new session, and returns the jar with its
/// cookie set. Role changes go through here too: an existing session is
/// replaced, never escalated in place.
pub(crate) async fn establish_session(
    sessions: &SessionStore,
    jar: CookieJar,
    session: Session,
    origin: LoginOrigin,
) -> Result<CookieJar, AppError> {
    if let Some(old_token) = jar.get(SESSION_COOKIE_NAME).map(|c| c.value().to_string()) {
        sessions.remove(&old_token).await?;
    }
    sessions.cleanup_expired().await;
    let cookie = build_session_cookie(&session, origin);
    sessions.insert(session).await;
    Ok(jar.add(cookie))
}

/// Truncated (64-bit) hex SHA-256 of the request `User-Agent` for session
/// pinning; a missing UA hashes the empty string.
pub(crate) fn user_agent_hash(headers: &HeaderMap) -> String {
    let ua = headers
        .get(USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let mut hex = sha256_hex(ua.as_bytes());
    hex.truncate(16); // first 8 bytes of the digest
    hex
}

/// Extracts the current session from request extensions.
impl<S> FromRequestParts<S> for Session
where
    S: Send + Sync,
{
    type Rejection = AppError;

    /// Retrieves the session that was injected by the session middleware.
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Session>()
            .cloned()
            .ok_or(AppError::InternalServerError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_cookie_same_site_follows_the_login_origin() {
        let session = Session::new_test();
        let strict = build_session_cookie(&session, LoginOrigin::SameSite);
        let lax = build_session_cookie(&session, LoginOrigin::CrossSite);
        assert_eq!(strict.same_site(), Some(SameSite::Strict));
        assert_eq!(lax.same_site(), Some(SameSite::Lax));
        assert_eq!(strict.value(), lax.value());

        let hardened = build_strict_session_cookie(lax.value());
        assert_eq!(hardened.same_site(), Some(SameSite::Strict));
        assert_eq!(hardened.value(), lax.value());
        assert_eq!(hardened.path(), lax.path());
        assert_eq!(hardened.http_only(), Some(true));
    }
}
