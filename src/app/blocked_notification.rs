//! `GET /blocked-notification`: called by the script on the CDN's (Bunny
//! Shield) block page. Answers `204` to everyone and logs
//! `event = "blocked_notification"` for logged-in users only, throttled per
//! user so the log cannot be flooded.
//!
//! Mounted outside the session middleware: anonymous callers get a cheap
//! `204` rather than a login redirect, and a notification is not activity.
//! A `GET` because the block page holds no CSRF token; the handler changes
//! nothing but an in-memory counter.
//!
//! Bunny documents no template variables for its pages, so the page sends
//! only what the browser knows. The CDN headers Bunny adds to every proxied
//! request (request id, country, bot classification, JA4) are logged too.

use std::{collections::HashMap, fmt, sync::Arc};

use axum::{
    Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header::USER_AGENT},
};
use axum_extra::{
    extract::{CookieJar, cookie::Cookie},
    routing::{RouterExt, TypedPath},
};
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::Deserialize;
use tracing::warn;

use crate::{
    AppError, AppState, CsbUser, RateLimit, SESSION_COOKIE_NAME, Session, SessionUser, StreamId,
};

#[derive(TypedPath)]
#[typed_path("/blocked-notification", rejection(AppError))]
pub struct BlockedNotificationPath;

pub fn router() -> Router<AppState> {
    Router::new().typed_get(blocked_notification)
}

/// Which Bunny Shield response page sent the notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    Block,
    Challenge,
    RateLimit,
}

impl fmt::Display for BlockKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Block => "block",
            Self::Challenge => "challenge",
            Self::RateLimit => "rate_limit",
        })
    }
}

/// Longest value of a client-supplied text field that reaches the log.
const MAX_LOG_TEXT_CHARS: usize = 256;

/// Client-supplied text made safe to log: no control characters (so no forged
/// log lines), at most [`MAX_LOG_TEXT_CHARS`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(from = "String")]
pub struct LogText(String);

impl From<String> for LogText {
    fn from(raw: String) -> Self {
        Self(
            raw.chars()
                .filter(|c| !c.is_control())
                .take(MAX_LOG_TEXT_CHARS)
                .collect(),
        )
    }
}

impl fmt::Display for LogText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What the block page's script reports; only `kind` is required.
#[derive(Debug, Deserialize)]
pub struct BlockedNotification {
    pub kind: BlockKind,
    /// Path of the blocked request, never its query string (CSRF tokens).
    pub path: Option<LogText>,
    /// Path of the referring page.
    pub referrer: Option<LogText>,
    /// Status of the blocked response, where the browser exposes it.
    pub status: Option<u16>,
    /// The CDN's id of the blocked request, when the page can get at it.
    pub request_id: Option<LogText>,
}

/// The user a notification is counted against.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ThrottleKey {
    Stream(StreamId),
    Committee(CsbUser),
}

impl From<&SessionUser> for ThrottleKey {
    fn from(user: &SessionUser) -> Self {
        match user {
            SessionUser::PoliticalGroup { stream_id, .. } => Self::Stream(*stream_id),
            SessionUser::CentralElectoralCommittee { user, .. } => Self::Committee(user.clone()),
        }
    }
}

/// Notifications counted since `start`.
#[derive(Debug, Clone, Copy)]
struct Window {
    start: DateTime<Utc>,
    count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    /// Over the limit; `first` marks the one refusal per window worth logging.
    Throttled {
        first: bool,
    },
}

/// Keys tracked before expired windows are swept; a safety valve, since only
/// logged-in users get a key.
const SWEEP_AT_KEYS: usize = 10_000;

/// Per-process count of notifications per user over a fixed window.
#[derive(Debug, Clone, Default)]
pub struct BlockedNotificationThrottle {
    windows: Arc<Mutex<HashMap<ThrottleKey, Window>>>,
}

impl BlockedNotificationThrottle {
    /// Count one notification for `key`; the window opens with the first one.
    pub fn record(&self, key: ThrottleKey, limit: RateLimit, now: DateTime<Utc>) -> Verdict {
        let window_start = limit.window_start(now);
        let mut windows = self.windows.lock();

        if windows.len() >= SWEEP_AT_KEYS && !windows.contains_key(&key) {
            windows.retain(|_, window| window.start > window_start);
        }

        let window = windows.entry(key).or_insert(Window {
            start: now,
            count: 0,
        });
        if window.start <= window_start {
            *window = Window {
                start: now,
                count: 0,
            };
        }

        let reached = limit.is_reached(window.count);
        window.count = window.count.saturating_add(1);

        if reached {
            Verdict::Throttled {
                first: window.count == limit.max.saturating_add(1),
            }
        } else {
            Verdict::Allowed
        }
    }

    #[cfg(test)]
    fn tracked_keys(&self) -> usize {
        self.windows.lock().len()
    }
}

/// Request headers Bunny adds to what it proxies to the origin.
const CDN_REQUEST_ID: &str = "cdn-requestid";
const CDN_COUNTRY_CODE: &str = "cdn-requestcountrycode";
const CDN_BOT: &str = "cdn-bot";
const CDN_JA4: &str = "cdn-ja4";

/// An optional [`LogText`] as a tracing field value.
fn text(value: Option<&LogText>) -> Option<String> {
    value.map(ToString::to_string)
}

/// A header's value as a loggable string, `None` when absent or not UTF-8.
fn header_text(headers: &HeaderMap, name: &str) -> Option<LogText> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(|value| LogText::from(value.to_string()))
}

async fn blocked_notification(
    _: BlockedNotificationPath,
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Query(notification): Query<BlockedNotification>,
) -> Result<StatusCode, AppError> {
    let token = jar.get(SESSION_COOKIE_NAME).map(Cookie::value);

    let session = match state.sessions.get_existing(token).await {
        Ok(Some(session)) => session,
        Ok(None) => return Ok(StatusCode::NO_CONTENT),
        // As in the session middleware: an outage is not a signed-out user.
        Err(err) if err.is_infrastructure_failure() => {
            state.db_health.mark_unavailable(&err);
            return Ok(StatusCode::SERVICE_UNAVAILABLE);
        }
        Err(err) => return Err(err),
    };

    let limit = state.config.rate_limits.blocked_notifications;
    let verdict =
        state
            .blocked_notifications
            .record(ThrottleKey::from(&session.user), limit, Utc::now());

    match verdict {
        Verdict::Allowed => log_notification(&session, &notification, &headers),
        Verdict::Throttled { first: true } => log_throttled(&session, limit),
        Verdict::Throttled { first: false } => {}
    }

    Ok(StatusCode::NO_CONTENT)
}

/// The session's identity for the log: `(stream_id, csb_user)`.
fn identity(session: &Session) -> (Option<String>, Option<String>) {
    match &session.user {
        SessionUser::PoliticalGroup { stream_id, .. } => (Some(stream_id.to_string()), None),
        SessionUser::CentralElectoralCommittee { user, .. } => (None, Some(format!("{user:?}"))),
    }
}

/// Alerting matches on `event = "blocked_notification"`.
fn log_notification(session: &Session, notification: &BlockedNotification, headers: &HeaderMap) {
    let (stream_id, csb_user) = identity(session);
    let path = text(notification.path.as_ref());
    let referrer = text(notification.referrer.as_ref());
    let request_id = text(notification.request_id.as_ref());
    let cdn_request_id = text(header_text(headers, CDN_REQUEST_ID).as_ref());
    let country = text(header_text(headers, CDN_COUNTRY_CODE).as_ref());
    let cdn_bot = text(header_text(headers, CDN_BOT).as_ref());
    let ja4 = text(header_text(headers, CDN_JA4).as_ref());
    let user_agent = text(header_text(headers, USER_AGENT.as_str()).as_ref());

    warn!(
        event = "blocked_notification",
        kind = %notification.kind,
        scope = ?session.scope(),
        stream_id,
        csb_user,
        election = session.user.election().map(|election| election.code()),
        election_domain = session.user.election().and_then(|election| election.domain_code()),
        session_created_at = %session.created_at,
        path,
        referrer,
        status = notification.status,
        request_id,
        cdn_request_id,
        country,
        cdn_bot,
        ja4,
        user_agent,
        "logged-in user blocked by the CDN"
    );
}

/// Alerting matches on `event = "blocked_notification.throttled"`.
fn log_throttled(session: &Session, limit: RateLimit) {
    let (stream_id, csb_user) = identity(session);

    warn!(
        event = "blocked_notification.throttled",
        scope = ?session.scope(),
        stream_id,
        csb_user,
        max = limit.max,
        window_secs = limit.window.num_seconds(),
        "blocked-notification limit reached; dropping further notifications this window"
    );
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::Body,
        http::{Request, header},
    };
    use chrono::TimeDelta;
    use tower::ServiceExt;
    use tracing_test::traced_test;

    use super::*;
    use crate::{Config, ElectionConfig, RateLimits};

    fn limit(max: usize) -> RateLimit {
        RateLimit {
            max,
            window: TimeDelta::minutes(10),
        }
    }

    /// `max` pass, the next refusal is loud, the rest of the window silent.
    #[test]
    fn throttle_allows_up_to_max_then_refuses_once_loudly() {
        let throttle = BlockedNotificationThrottle::default();
        let key = ThrottleKey::Stream(StreamId::new());
        let now = Utc::now();

        assert_eq!(
            throttle.record(key.clone(), limit(2), now),
            Verdict::Allowed
        );
        assert_eq!(
            throttle.record(key.clone(), limit(2), now),
            Verdict::Allowed
        );
        assert_eq!(
            throttle.record(key.clone(), limit(2), now),
            Verdict::Throttled { first: true }
        );
        assert_eq!(
            throttle.record(key, limit(2), now),
            Verdict::Throttled { first: false }
        );
    }

    /// A new window opens once the old one has passed.
    #[test]
    fn throttle_resets_after_the_window() {
        let throttle = BlockedNotificationThrottle::default();
        let key = ThrottleKey::Stream(StreamId::new());
        let now = Utc::now();

        assert_eq!(
            throttle.record(key.clone(), limit(1), now),
            Verdict::Allowed
        );
        assert_eq!(
            throttle.record(key.clone(), limit(1), now),
            Verdict::Throttled { first: true }
        );

        let later = now + TimeDelta::minutes(10);
        assert_eq!(throttle.record(key, limit(1), later), Verdict::Allowed);
    }

    /// Users are counted apart, and a zero limit disables the throttle.
    #[test]
    fn throttle_is_per_user_and_zero_disables() {
        let throttle = BlockedNotificationThrottle::default();
        let now = Utc::now();
        let a = ThrottleKey::Stream(StreamId::new());
        let b = ThrottleKey::Committee(CsbUser::new_test());

        assert_eq!(throttle.record(a.clone(), limit(1), now), Verdict::Allowed);
        assert_eq!(throttle.record(b, limit(1), now), Verdict::Allowed);
        assert_eq!(
            throttle.record(a.clone(), limit(1), now),
            Verdict::Throttled { first: true }
        );

        for _ in 0..5 {
            assert_eq!(throttle.record(a.clone(), limit(0), now), Verdict::Allowed);
        }
    }

    /// Expired windows are swept once the map grows large.
    #[test]
    fn throttle_sweeps_expired_windows_when_large() {
        let throttle = BlockedNotificationThrottle::default();
        let then = Utc::now() - TimeDelta::hours(1);
        for _ in 0..SWEEP_AT_KEYS {
            throttle.record(ThrottleKey::Stream(StreamId::new()), limit(5), then);
        }
        assert_eq!(throttle.tracked_keys(), SWEEP_AT_KEYS);

        throttle.record(ThrottleKey::Stream(StreamId::new()), limit(5), Utc::now());

        assert_eq!(throttle.tracked_keys(), 1);
    }

    /// Control characters are dropped and the length is capped.
    #[test]
    fn log_text_is_sanitised() {
        let text = LogText::from("a\nb\r\n\tc\u{7f}d".to_string());
        assert_eq!(text.to_string(), "abcd");

        let long = LogText::from("x".repeat(MAX_LOG_TEXT_CHARS + 50));
        assert_eq!(long.to_string().chars().count(), MAX_LOG_TEXT_CHARS);
    }

    fn app(state: &AppState) -> Router {
        router().with_state(state.clone())
    }

    async fn state_with_limit(max: usize) -> AppState {
        let mut config = Config::new_test();
        config.rate_limits = RateLimits::default().with_blocked_notifications(limit(max));
        AppState::new_for_tests_with_config(config).await
    }

    fn request(uri: &str, cookie: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .uri(uri)
            .header(USER_AGENT, "test-browser")
            .header(CDN_REQUEST_ID, "e139c9b1d59675e24a8a25fadf9c324b")
            .header(CDN_COUNTRY_CODE, "NL");
        if let Some(cookie) = cookie {
            builder = builder.header(header::COOKIE, cookie);
        }
        builder.body(Body::empty()).unwrap()
    }

    async fn political_group_cookie(state: &AppState) -> (String, StreamId) {
        let mut session = Session::new_test();
        session.set_test_election(ElectionConfig::EK27);
        let stream_id = session.test_stream_id();
        let token = session.token_string();
        state.sessions.insert(session).await;
        (format!("{SESSION_COOKIE_NAME}={token}"), stream_id)
    }

    /// No session: `204`, nothing logged, no login redirect.
    #[traced_test]
    #[tokio::test]
    async fn anonymous_notification_is_dropped_silently() {
        let state = state_with_limit(5).await;

        let response = app(&state)
            .oneshot(request("/blocked-notification?kind=block&path=/x", None))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(!logs_contain("blocked_notification"));
    }

    /// A political group's notification is logged with stream id, page
    /// fields and CDN headers.
    #[traced_test]
    #[tokio::test]
    async fn logged_in_notification_is_logged_with_stream_id() {
        let state = state_with_limit(5).await;
        let (cookie, stream_id) = political_group_cookie(&state).await;

        let response = app(&state)
            .oneshot(request(
                "/blocked-notification?kind=rate_limit&path=/persons&referrer=/&status=429",
                Some(&cookie),
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(logs_contain("event=\"blocked_notification\""));
        assert!(logs_contain(&format!("stream_id=\"{stream_id}\"")));
        assert!(logs_contain("kind=rate_limit"));
        assert!(logs_contain("path=\"/persons\""));
        assert!(logs_contain("status=429"));
        assert!(logs_contain(
            "cdn_request_id=\"e139c9b1d59675e24a8a25fadf9c324b\""
        ));
        assert!(logs_contain("country=\"NL\""));
        assert!(logs_contain("election=\"EK27\""));
    }

    /// A committee session is logged by its committee identity.
    #[traced_test]
    #[tokio::test]
    async fn committee_notification_is_logged_with_csb_user() {
        let state = state_with_limit(5).await;
        let session = Session::new_test_committee();
        let token = session.token_string();
        state.sessions.insert(session).await;
        let cookie = format!("{SESSION_COOKIE_NAME}={token}");

        let response = app(&state)
            .oneshot(request(
                "/blocked-notification?kind=challenge",
                Some(&cookie),
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(logs_contain("event=\"blocked_notification\""));
        assert!(logs_contain("csb_user=\"Developer\""));
        assert!(logs_contain("scope=CentralElectoralCommittee"));
    }

    /// Over the limit: one throttle marker, then silence for the window.
    #[traced_test]
    #[tokio::test]
    async fn notifications_over_the_limit_are_throttled() {
        let state = state_with_limit(1).await;
        let (cookie, _) = political_group_cookie(&state).await;
        let app = app(&state);

        for _ in 0..3 {
            let response = app
                .clone()
                .oneshot(request("/blocked-notification?kind=block", Some(&cookie)))
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
        }

        logs_assert(|lines: &[&str]| {
            let logged = lines
                .iter()
                .filter(|line| line.contains("event=\"blocked_notification\""))
                .count();
            let throttled = lines
                .iter()
                .filter(|line| line.contains("event=\"blocked_notification.throttled\""))
                .count();
            if logged == 1 && throttled == 1 {
                Ok(())
            } else {
                Err(format!(
                    "expected 1 logged + 1 throttled, got {logged} + {throttled}"
                ))
            }
        });
    }

    /// A notification does not count as session activity.
    #[tokio::test]
    async fn notification_does_not_touch_the_session() {
        let state = state_with_limit(5).await;
        let session = Session::new_test();
        let token = session.token_string();
        let last_activity = session.last_activity;
        state.sessions.insert(session).await;

        app(&state)
            .oneshot(request(
                "/blocked-notification?kind=block",
                Some(&format!("{SESSION_COOKIE_NAME}={token}")),
            ))
            .await
            .expect("response");

        let reloaded = state
            .sessions
            .get(&token)
            .await
            .expect("load")
            .expect("session");
        assert_eq!(reloaded.last_activity, last_activity);
    }

    /// An unknown kind is a malformed request, not a logged notification.
    #[traced_test]
    #[tokio::test]
    async fn unknown_kind_is_rejected() {
        let state = state_with_limit(5).await;
        let (cookie, _) = political_group_cookie(&state).await;

        let response = app(&state)
            .oneshot(request("/blocked-notification?kind=banana", Some(&cookie)))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(!logs_contain("event=\"blocked_notification\""));
    }
}
