//! Typed paths for the common (session-level) routes.

use axum::http::Uri;
use axum_extra::routing::TypedPath;

use crate::AppError;

#[derive(TypedPath)]
#[typed_path("/login", rejection(AppError))]
pub struct LoginStartPath;

/// Query flag that makes the login start page explain the session expired.
const SESSION_EXPIRED_QUERY: &str = "expired=true";

impl LoginStartPath {
    /// The login start page with the "your session expired" notice, where the
    /// expiry-warning script sends the browser once the session is gone.
    pub fn expired_url() -> String {
        format!("{}?{SESSION_EXPIRED_QUERY}", Self::PATH)
    }

    /// Whether `uri` carries the expired-session flag of [`Self::expired_url`].
    pub fn asks_for_expired_notice(uri: &Uri) -> bool {
        uri.query()
            .is_some_and(|query| query.split('&').any(|pair| pair == SESSION_EXPIRED_QUERY))
    }
}

#[derive(TypedPath)]
#[typed_path("/", rejection(AppError))]
pub struct PgIndexPath;

#[derive(TypedPath)]
#[typed_path("/language", rejection(AppError))]
pub struct SwitchLanguagePath;

#[derive(TypedPath)]
#[typed_path("/switch-election", rejection(AppError))]
pub struct SwitchElectionPath;

#[derive(TypedPath)]
#[typed_path("/select-election", rejection(AppError))]
pub struct SelectElectionPath;

#[derive(TypedPath)]
#[typed_path("/hide-download-warning", rejection(AppError))]
pub struct HideDownloadWarningPath;

#[derive(TypedPath)]
#[typed_path("/logout", rejection(AppError))]
pub struct LogoutPath;

/// The session's remaining lifetime: `GET` peeks at it without counting as
/// activity, `POST` extends it. Both answer JSON for the expiry-warning script.
#[derive(TypedPath)]
#[typed_path("/session", rejection(AppError))]
pub struct SessionExpiryPath;

#[derive(TypedPath)]
#[typed_path("/logged-out", rejection(AppError))]
pub struct LoggedOutPath;
