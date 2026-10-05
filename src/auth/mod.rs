//! Authentication and session helpers.

/// The authenticated identity behind a CSB (committee) session.
pub mod csb_user;

/// BSN-based identifier derivation using HKDF-SHA256.
pub mod derive_id;

/// Session model and token utilities.
pub mod session;

/// The identity behind a session: one variant per role.
pub mod session_user;

/// Session storage with pluggable in-memory or Postgres backends.
pub mod session_store;

/// Postgres-backed session persistence (feature-gated).
#[cfg(feature = "database")]
mod session_db;

/// Pending AuthnRequest ID storage with pluggable in-memory or Postgres backends.
pub mod pending_request_store;

/// Postgres-backed pending-request persistence (feature-gated).
#[cfg(feature = "database")]
mod pending_request_db;

/// CSRF token verification for mutating requests, driven by the session
/// middleware.
pub mod csrf_guard;

/// Session cookie helpers and request extraction.
pub mod session_extractor;

/// Passkey (WebAuthn) login for committee members: identities, credential
/// store, ceremony state and the configured relying party.
pub mod passkey;
