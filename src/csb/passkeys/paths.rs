//! Typed paths for the passkey management pages.

use axum_extra::routing::TypedPath;
use serde::Deserialize;

use crate::{AppError, PasskeyAccount, PasskeyAccountId, PasskeyId, StoredPasskey};

#[derive(TypedPath)]
#[typed_path("/csb/passkeys", rejection(AppError))]
pub struct CsbPasskeysPath;

/// JSON POST: starts a registration ceremony for the session's user.
#[derive(TypedPath)]
#[typed_path("/csb/passkeys/register/start", rejection(AppError))]
pub struct CsbPasskeyRegisterStartPath;

/// JSON POST: finishes the registration ceremony and stores the passkey.
#[derive(TypedPath)]
#[typed_path("/csb/passkeys/register/finish", rejection(AppError))]
pub struct CsbPasskeyRegisterFinishPath;

#[derive(TypedPath, Deserialize)]
#[typed_path("/csb/passkeys/{id}/delete", rejection(AppError))]
pub struct CsbDeletePasskeyPath {
    pub id: PasskeyId,
}

#[derive(TypedPath, Deserialize)]
#[typed_path("/csb/passkeys/accounts/{id}/delete", rejection(AppError))]
pub struct CsbDeletePasskeyAccountPath {
    pub id: PasskeyAccountId,
}

impl StoredPasskey {
    pub fn delete_path(&self) -> CsbDeletePasskeyPath {
        CsbDeletePasskeyPath { id: self.id }
    }
}

impl PasskeyAccount {
    pub fn delete_path(&self) -> CsbDeletePasskeyAccountPath {
        CsbDeletePasskeyAccountPath { id: self.id }
    }
}
