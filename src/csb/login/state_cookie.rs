//! Short-lived cookie binding the login challenge to the browser that loaded
//! the login page: a finish whose challenge was minted for another browser
//! is rejected (login-CSRF defence, complementing the one-shot
//! pending-request check). The server-side 15-minute pending TTL bounds the
//! challenge lifetime.

use axum_extra::extract::cookie::{Cookie, SameSite};

/// Name of the challenge cookie. Like the session cookie, the `__Host-`
/// prefix (production only) forbids a `Domain` and requires `Secure` +
/// `Path=/`.
#[cfg(feature = "dev-features")]
pub(super) const STATE_COOKIE_NAME: &str = "EKS_CSB_CHALLENGE";
#[cfg(not(feature = "dev-features"))]
pub(super) const STATE_COOKIE_NAME: &str = "__Host-EKS_CSB_CHALLENGE";

fn apply_state_cookie_attributes(cookie: &mut Cookie<'static>) {
    cookie.set_http_only(true);
    #[cfg(feature = "dev-features")]
    cookie.set_secure(false);
    #[cfg(not(feature = "dev-features"))]
    cookie.set_secure(true);
    // The finish step is a same-site form post from the login page itself.
    cookie.set_same_site(SameSite::Strict);
    cookie.set_path("/");
}

/// Cookie carrying the base64url challenge for the duration of the ceremony.
pub(super) fn build_state_cookie(challenge: String) -> Cookie<'static> {
    let mut cookie = Cookie::new(STATE_COOKIE_NAME, challenge);
    apply_state_cookie_attributes(&mut cookie);
    cookie
}

/// Expired twin of the challenge cookie for clearing it; attributes must
/// match.
pub(super) fn build_state_removal_cookie() -> Cookie<'static> {
    let mut cookie = Cookie::from(STATE_COOKIE_NAME);
    apply_state_cookie_attributes(&mut cookie);
    cookie
}
