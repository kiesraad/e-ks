//! Management of the passkeys committee members log in with.
//!
//! Any committee session (GitHub, passkey or dev login) can register a
//! passkey here, so the first passkey rides on a GitHub login and no admin
//! tooling or enrollment code is needed. A GitHub or dev session creates a
//! new passkey account; a passkey session adds to its own account only, so
//! nobody can plant a credential on a colleague's account. Any member may
//! revoke any passkey or account: a lost device must not depend on its
//! owner. Every change is recorded on the CSB main stream.
mod pages;
pub(in crate::csb) mod paths;

pub use pages::router;
pub use paths::CsbPasskeysPath;
