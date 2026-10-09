//! The CSB security-key (WebAuthn) login endpoints. The mechanism they
//! drive, the challenge lifecycle and the assertion verification, lives in
//! [`crate::auth::csb_webauthn`]; this module holds only the HTTP side.
//!
//! - `GET /csb/login` issues a challenge and renders the page that runs
//!   `navigator.credentials.get` with every configured credential id, so the
//!   key identifies the user and no username is typed.
//! - `POST /csb/login/finish` verifies the assertion, drops any pre-existing
//!   session (fixation defence), creates a
//!   [`crate::SessionUser::CentralElectoralCommittee`] session and records
//!   the login on the shared CSB main stream for the audit log.
//! - Every failure ends on the login page with one generic message, so
//!   nothing reveals which check failed.

mod pages;
mod paths;

pub use pages::public_router;
pub use paths::{CsbLoginFinishPath, CsbLoginPath};

use crate::{AppError, Config, CsbWebauthnConfig};

/// The security-key config, or 404 when this deployment has no CSB login.
fn require_webauthn(config: &Config) -> Result<&CsbWebauthnConfig, AppError> {
    config
        .csb_webauthn
        .as_ref()
        .ok_or(AppError::GenericNotFound)
}
