//! Typed paths for the CSB security-key login. Both steps are same-origin
//! (a page and a form post), so one strict Content-Security-Policy covers the
//! whole flow.

use axum_extra::routing::TypedPath;

use crate::AppError;

/// The login page, which starts the ceremony.
#[derive(TypedPath)]
#[typed_path("/csb/login", rejection(AppError))]
pub struct CsbLoginPath;

/// Receives the signed assertion and establishes the session.
#[derive(TypedPath)]
#[typed_path("/csb/login/finish", rejection(AppError))]
pub struct CsbLoginFinishPath;
