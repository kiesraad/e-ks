//! Typed paths for the common (session-level) routes.

use axum_extra::routing::TypedPath;

use crate::{AppError, QueryParamState};

#[derive(TypedPath)]
#[typed_path("/login", rejection(AppError))]
pub struct LoginStartPath;

impl LoginStartPath {
    /// The login start page with the "your session expired" notice, where the
    /// expiry-warning script sends the browser once the session is gone.
    pub fn expired_url() -> impl TypedPath {
        Self.with_query_params(QueryParamState::session_expired())
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
