//! Session model and token generation.

use chrono::{DateTime, Duration, Utc};
use rand::{RngExt, distr::Alphanumeric};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;

use crate::{
    AppError, CsbUser, ElectionConfig, Locale, Scope, SessionUser, StreamId, TokenValue,
    form::{csrf_token_matches, generate_csrf_token},
    utils::sha256_hex,
};

/// Idle timeout (in seconds) after which a session is considered expired: the
/// ceiling the DigiD "Checklist Testen" (TVS v2.1 T8) sets, at most 15 minutes
/// of inactivity. Every request through the session middleware counts as
/// activity, so a page load extends the session by this much.
const SESSION_IDLE_TIMEOUT_SECS: i64 = 15 * 60; // 15 minutes

/// Absolute cap on total session lifetime, regardless of activity (defense in
/// depth; TVS mandates only the idle ceiling). Covers one working day.
const SESSION_ABSOLUTE_TIMEOUT_SECS: i64 = 8 * 60 * 60; // 8 hours

/// How long before the session expires the browser warns the user and offers
/// to extend it (see `frontend/scripts/generic-ui/session-expiry.ts`).
const SESSION_EXPIRY_WARNING_LEAD_SECS: i64 = 1 * 60; // 1 minute

/// Idle timeout after which a session is considered expired.
pub fn session_idle_timeout() -> Duration {
    Duration::seconds(SESSION_IDLE_TIMEOUT_SECS)
}

/// Absolute lifetime cap, checked regardless of activity.
pub fn session_absolute_timeout() -> Duration {
    Duration::seconds(SESSION_ABSOLUTE_TIMEOUT_SECS)
}

/// Lead time of the expiry warning shown in the browser.
pub fn session_expiry_warning_lead() -> Duration {
    Duration::seconds(SESSION_EXPIRY_WARNING_LEAD_SECS)
}

/// When a session runs out, as the browser needs it: rendered into every
/// session page as data attributes and answered as JSON on `/session`, so the
/// script that warns before expiry never hard-codes a duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SessionExpiry {
    /// Seconds until the session expires, whichever of the idle timeout and
    /// the absolute cap comes first. Zero once expired.
    pub expires_in_secs: u64,
    /// Seconds before expiry at which the warning should appear.
    pub warning_lead_secs: u64,
    /// Whether activity still pushes the expiry back. `false` once the absolute
    /// cap is the binding limit: extending is then pointless, and the warning
    /// says the user has to log in again instead of offering to extend.
    pub extendable: bool,
}

impl SessionExpiry {
    /// A session that has already expired.
    pub fn expired() -> Self {
        Self {
            expires_in_secs: 0,
            warning_lead_secs: Self::warning_lead_secs(),
            extendable: false,
        }
    }

    fn warning_lead_secs() -> u64 {
        u64::try_from(session_expiry_warning_lead().num_seconds()).unwrap_or_default()
    }

    /// `true` while the remaining time is within the warning lead.
    pub fn is_due_for_warning(&self) -> bool {
        self.expires_in_secs <= self.warning_lead_secs
    }
}

/// SHA-256 (hex) of a raw token: the value stored at rest and used as the lookup
/// key, so the bearer token itself is never persisted.
pub(crate) fn hash_token(raw: &str) -> String {
    sha256_hex(raw.as_bytes())
}

/// Opaque session token kept secret until explicitly exposed.
#[derive(Clone)]
pub struct SessionToken(SecretString);

impl SessionToken {
    pub(crate) fn new(value: String) -> Self {
        Self(SecretString::from(value))
    }

    pub(crate) fn expose(&self) -> &str {
        self.0.expose_secret()
    }

    pub(crate) fn to_exposed_string(&self) -> String {
        self.expose().to_string()
    }
}

impl std::fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionToken([REDACTED])")
    }
}

/// Server-side session data.
///
/// Persisted either in memory or the database depending on `STORAGE_URL`.
/// Carries no BSN/`id_code`: the user's stream id is pre-derived at login.
/// The role-specific state lives in one [`SessionUser`] value, so a session
/// always belongs to exactly one complete identity; sessions only exist after
/// a successful login.
#[derive(Clone)]
pub struct Session {
    /// SHA-256 (hex) of the token: the storage key and only token material at rest.
    pub(crate) token_hash: String,
    /// Raw token, held only until the `Set-Cookie` is emitted; `None` once loaded
    /// from storage, so a reloaded session can't re-expose it.
    pub(crate) raw_token: Option<SessionToken>,
    /// Random CSRF token embedded in forms and verified on every mutating
    /// request
    pub(crate) csrf_token: TokenValue,
    /// Creation time, for the absolute-lifetime cap.
    pub created_at: DateTime<Utc>,
    /// Timestamp of the last activity for idle-timeout validation.
    pub last_activity: DateTime<Utc>,
    /// Truncated SHA-256 of the creating `User-Agent`; when set, the middleware
    /// rejects requests whose UA differs. `None` leaves the session unpinned.
    pub user_agent_hash: Option<String>,
    /// The identity behind the session, set at login (see [`SessionUser`]).
    pub user: SessionUser,
    /// Active locale for the session.
    pub locale: Locale,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("token", &"***")
            .field("created_at", &self.created_at)
            .field("last_activity", &self.last_activity)
            .field("user", &self.user)
            .field("locale", &self.locale)
            .finish()
    }
}

impl PartialEq for Session {
    fn eq(&self, other: &Self) -> bool {
        self.token_hash == other.token_hash
    }
}

impl Eq for Session {}

impl Session {
    /// Creates a new session for `user` with a cryptographically strong random
    /// token. Sessions only come into existence through a login flow, so the
    /// identity is required up front.
    fn new(user: SessionUser, locale: Locale) -> Self {
        let raw_token = generate_session_token();
        let now = Utc::now();
        Self {
            token_hash: hash_token(raw_token.expose()),
            raw_token: Some(raw_token),
            csrf_token: generate_csrf_token(),
            created_at: now,
            last_activity: now,
            user_agent_hash: None,
            user,
            locale,
        }
    }

    /// Creates a session for a political group that logged in.
    pub fn for_political_group(
        stream_id: StreamId,
        saml_name_id: String,
        election: Option<ElectionConfig>,
        locale: Locale,
    ) -> Self {
        Self::new(
            SessionUser::PoliticalGroup {
                stream_id,
                saml_name_id,
                election,
            },
            locale,
        )
    }

    /// Creates a session for a committee member that logged in.
    pub fn for_committee(user: CsbUser, election: ElectionConfig, locale: Locale) -> Self {
        Self::new(
            SessionUser::CentralElectoralCommittee {
                user,
                election,
                paper_correction_stream_id: None,
            },
            locale,
        )
    }

    /// Session-side view of the stream classifier, for logging and checks.
    pub fn scope(&self) -> Scope {
        self.user.scope()
    }

    /// The current election, or an internal error when none has been picked
    /// yet (CSB routes may rely on it being present).
    pub fn require_current_election(&self) -> Result<ElectionConfig, AppError> {
        self.user.election().ok_or(AppError::InternalServerError)
    }

    /// The committee identity of this session, or `Unauthorised` when the
    /// session was not established through a CSB login.
    pub fn require_csb_user(&self) -> Result<CsbUser, AppError> {
        match &self.user {
            SessionUser::CentralElectoralCommittee { user, .. } => Ok(user.clone()),
            SessionUser::PoliticalGroup { .. } => Err(AppError::Unauthorised),
        }
    }

    /// Enters (`Some`) or leaves (`None`) paper-corrections mode, or
    /// `Unauthorised` for a session that is not a committee session.
    pub fn set_paper_correction_stream_id(
        &mut self,
        stream_id: Option<StreamId>,
    ) -> Result<(), AppError> {
        match &mut self.user {
            SessionUser::CentralElectoralCommittee {
                paper_correction_stream_id,
                ..
            } => {
                *paper_correction_stream_id = stream_id;
                Ok(())
            }
            SessionUser::PoliticalGroup { .. } => Err(AppError::Unauthorised),
        }
    }

    /// Pins the session to the hash of the client's `User-Agent`.
    pub fn set_user_agent_hash(&mut self, user_agent_hash: String) {
        self.user_agent_hash = Some(user_agent_hash);
    }

    /// Returns the SHA-256 (hex) storage key for this session.
    pub(crate) fn token_hash(&self) -> &str {
        &self.token_hash
    }

    /// Raw token if still in memory (only between creation and cookie-minting).
    pub(crate) fn reveal_token(&self) -> Option<&SessionToken> {
        self.raw_token.as_ref()
    }

    /// True once past the idle timeout or the absolute lifetime cap.
    pub fn is_expired(&self) -> bool {
        self.expiry_at(Utc::now()).expires_in_secs == 0
    }

    /// The remaining lifetime as seen from `now`, for the browser-side warning.
    pub fn expiry(&self) -> SessionExpiry {
        self.expiry_at(Utc::now())
    }

    fn expiry_at(&self, now: DateTime<Utc>) -> SessionExpiry {
        let idle_deadline = self.last_activity + session_idle_timeout();
        let absolute_deadline = self.created_at + session_absolute_timeout();
        let deadline = idle_deadline.min(absolute_deadline);
        // A deadline in the past clamps to zero: the session is expired.
        let expires_in_secs = u64::try_from((deadline - now).num_seconds()).unwrap_or_default();
        SessionExpiry {
            expires_in_secs,
            warning_lead_secs: SessionExpiry::warning_lead_secs(),
            extendable: expires_in_secs > 0 && idle_deadline < absolute_deadline,
        }
    }

    /// Raw CSRF token to embed in rendered forms.
    pub fn csrf_token(&self) -> &TokenValue {
        &self.csrf_token
    }

    /// Constant-time check of a submitted CSRF token against the session's.
    pub fn csrf_matches(&self, submitted: &str) -> bool {
        csrf_token_matches(submitted, &self.csrf_token.0)
    }

    /// Replaces the CSRF token, so forms rendered before the switch (e.g. in
    /// another tab) can no longer submit against the new context.
    pub fn rotate_csrf_token(&mut self) {
        self.csrf_token = generate_csrf_token();
    }
}

#[cfg(test)]
impl Session {
    /// Test session: a political group with a fresh random stream and no
    /// election picked yet.
    pub fn new_test() -> Self {
        Self::new_test_with_locale(Locale::default())
    }

    pub fn new_test_with_locale(locale: Locale) -> Self {
        Self::for_political_group(StreamId::new(), String::new(), None, locale)
    }

    /// Test session: a political group bound to the given stream.
    pub fn new_test_for_stream(stream_id: StreamId) -> Self {
        Self::for_political_group(stream_id, String::new(), None, Locale::default())
    }

    /// Test session: a committee member on the EK27 election.
    pub fn new_test_committee() -> Self {
        Self::for_committee(CsbUser::new_test(), ElectionConfig::EK27, Locale::default())
    }

    /// Test helper: set the election on either identity variant.
    pub fn set_test_election(&mut self, election: ElectionConfig) {
        match &mut self.user {
            SessionUser::PoliticalGroup {
                election: current, ..
            } => *current = Some(election),
            SessionUser::CentralElectoralCommittee {
                election: current, ..
            } => *current = election,
        }
    }

    /// Test helper: the political group's stream, panicking for committee
    /// sessions (tests that need it construct political-group sessions).
    pub fn test_stream_id(&self) -> StreamId {
        match &self.user {
            SessionUser::PoliticalGroup { stream_id, .. } => *stream_id,
            SessionUser::CentralElectoralCommittee { .. } => {
                panic!("committee sessions have no stream of their own")
            }
        }
    }

    /// Test helper: the paper-corrections stream of a committee session.
    pub fn test_paper_correction_stream_id(&self) -> Option<StreamId> {
        match &self.user {
            SessionUser::CentralElectoralCommittee {
                paper_correction_stream_id,
                ..
            } => *paper_correction_stream_id,
            SessionUser::PoliticalGroup { .. } => None,
        }
    }

    /// Test helper: raw token of a fresh session (panics on a reloaded one).
    pub fn token_string(&self) -> String {
        self.reveal_token()
            .expect("fresh session retains its raw token")
            .to_exposed_string()
    }
}

/// Generates a random session token with ~250 bits of entropy.
fn generate_session_token() -> SessionToken {
    // 62-character alphabet => log2(62) ~= 5.95 bits per char.
    // 42 chars gives ~250 bits of entropy (42 * 5.95 ~= 250) - the answer, obviously.
    let token = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(42)
        .map(char::from)
        .collect();
    SessionToken::new(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ensures session tokens are 42-char base62 strings (~250-bit entropy).
    #[test]
    fn new_generates_base62_token() {
        let session = Session::new_test();
        let token = session.token_string();

        assert_eq!(token.len(), 42);
        assert!(token.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    /// Stored key is the token's hash, not the token itself.
    #[test]
    fn token_hash_is_sha256_of_raw_token() {
        let session = Session::new_test();
        let raw = session.token_string();

        assert_eq!(session.token_hash(), hash_token(&raw));
        assert_eq!(session.token_hash().len(), 64); // 32 bytes hex-encoded
        assert_ne!(session.token_hash(), raw);
    }

    /// The CSRF token is random and independent of the session token.
    #[test]
    fn csrf_token_is_random_and_independent_of_session_token() {
        let session = Session::new_test();
        let other = Session::new_test();

        assert!(!session.csrf_token().0.is_empty());
        assert_ne!(session.csrf_token().0, session.token_hash());
        assert_ne!(session.csrf_token().0, session.token_string());
        // Distinct sessions get distinct CSRF tokens.
        assert_ne!(session.csrf_token(), other.csrf_token());
    }

    /// The session verifies its own token and rejects other values.
    #[test]
    fn csrf_matches_verifies_submitted_token() {
        let session = Session::new_test();
        let token = session.csrf_token().to_string();

        assert!(session.csrf_matches(&token));
        assert!(!session.csrf_matches("wrong"));
    }

    /// Rotation replaces the token and invalidates the previous one.
    #[test]
    fn rotate_csrf_token_invalidates_previous_token() {
        let mut session = Session::new_test();
        let old = session.csrf_token().to_string();

        session.rotate_csrf_token();

        assert_ne!(session.csrf_token().0, old);
        assert!(!session.csrf_matches(&old));
        assert!(session.csrf_matches(&session.csrf_token().to_string()));
    }

    /// Confirms idle timeout invalidates stale sessions.
    #[test]
    fn session_expires_after_idle_timeout() {
        let mut session = Session::new_test();
        session.last_activity = Utc::now() - session_idle_timeout() - Duration::seconds(1);

        assert!(session.is_expired());
    }

    /// A refreshed-but-old session still expires at the absolute cap.
    #[test]
    fn session_expires_after_absolute_timeout() {
        let mut session = Session::new_test();
        session.last_activity = Utc::now(); // not idle
        session.created_at = Utc::now() - session_absolute_timeout() - Duration::seconds(1);

        assert!(session.is_expired());
    }

    /// The idle timeout matches the DigiD ceiling of 15 minutes inactivity,
    /// and the warning leads it by two minutes.
    #[test]
    fn timeouts_follow_the_digid_checklist() {
        assert_eq!(session_idle_timeout(), Duration::minutes(15));
        assert_eq!(session_expiry_warning_lead(), Duration::minutes(2));
        assert!(session_expiry_warning_lead() < session_idle_timeout());
    }

    /// A fresh session expires when its idle timeout runs out, and activity
    /// can still extend it.
    #[test]
    fn expiry_of_fresh_session_is_the_idle_timeout() {
        let session = Session::new_test();
        let now = session.last_activity;

        let expiry = session.expiry_at(now);

        assert_eq!(expiry.expires_in_secs, 15 * 60);
        assert_eq!(expiry.warning_lead_secs, 2 * 60);
        assert!(expiry.extendable);
        assert!(!expiry.is_due_for_warning());
    }

    /// Within the warning lead the expiry reports itself as due for a warning.
    #[test]
    fn expiry_is_due_for_warning_within_the_lead() {
        let mut session = Session::new_test();
        session.last_activity = Utc::now() - session_idle_timeout() + Duration::seconds(90);

        let expiry = session.expiry();

        assert!(expiry.expires_in_secs <= 90);
        assert!(expiry.expires_in_secs > 80);
        assert!(expiry.is_due_for_warning());
        assert!(expiry.extendable);
    }

    /// Once the absolute cap comes first, the remaining time follows the cap
    /// and extending is no longer offered.
    #[test]
    fn expiry_follows_the_absolute_cap_when_it_comes_first() {
        let mut session = Session::new_test();
        session.created_at = Utc::now() - session_absolute_timeout() + Duration::minutes(5);
        session.last_activity = Utc::now();

        let expiry = session.expiry();

        assert!(expiry.expires_in_secs <= 5 * 60);
        assert!(expiry.expires_in_secs > 5 * 60 - 10);
        assert!(!expiry.extendable);
    }

    /// An expired session reports zero remaining seconds, never a negative
    /// value wrapped around.
    #[test]
    fn expiry_of_expired_session_is_zero() {
        let mut session = Session::new_test();
        session.last_activity = Utc::now() - session_idle_timeout() - Duration::hours(1);

        assert_eq!(session.expiry(), SessionExpiry::expired());
    }

    /// Only committee sessions can enter paper-corrections mode.
    #[test]
    fn paper_corrections_mode_is_committee_only() {
        let mut committee = Session::new_test_committee();
        let stream_id = StreamId::new();
        committee
            .set_paper_correction_stream_id(Some(stream_id))
            .expect("committee session");
        assert!(matches!(
            committee.user,
            SessionUser::CentralElectoralCommittee {
                paper_correction_stream_id: Some(id),
                ..
            } if id == stream_id
        ));

        let mut political_group = Session::new_test();
        assert!(matches!(
            political_group.set_paper_correction_stream_id(Some(stream_id)),
            Err(AppError::Unauthorised)
        ));
    }

    /// A committee identity is only handed out for committee sessions.
    #[test]
    fn require_csb_user_rejects_political_group_sessions() {
        assert!(Session::new_test_committee().require_csb_user().is_ok());
        assert!(matches!(
            Session::new_test().require_csb_user(),
            Err(AppError::Unauthorised)
        ));
    }
}
