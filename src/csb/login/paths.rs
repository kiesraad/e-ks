//! Typed paths for the CSB login flows.

use axum_extra::routing::TypedPath;

use crate::AppError;

#[derive(TypedPath)]
#[typed_path("/csb/login", rejection(AppError))]
pub struct CsbLoginPath;

/// Starts the OAuth round-trip. A plain link, so the browser navigates to
/// GitHub without a form submission: `form-action` never enters the picture
/// and the app keeps one strict Content-Security-Policy for every page.
#[derive(TypedPath)]
#[typed_path("/csb/login/start", rejection(AppError))]
pub struct CsbLoginStartPath;

#[derive(TypedPath)]
#[typed_path("/csb/login/callback", rejection(AppError))]
pub struct CsbLoginCallbackPath;

/// Starts a passkey login ceremony: a JSON POST from the login page's script
/// with the account name, answered with the WebAuthn request options.
#[derive(TypedPath)]
#[typed_path("/csb/login/passkey/start", rejection(AppError))]
pub struct CsbPasskeyLoginStartPath;

/// Finishes a passkey login ceremony: a JSON POST with the browser's
/// assertion, answered with `204` once the session is established.
#[derive(TypedPath)]
#[typed_path("/csb/login/passkey/finish", rejection(AppError))]
pub struct CsbPasskeyLoginFinishPath;
