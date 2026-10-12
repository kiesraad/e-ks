//! Access guards for the CSB section.
//!
//! [`csb_ip_allow_list_middleware`] refuses peers off `Config::csb_ip_allow_list`
//! and [`report_alert_hours_activity`], called by the session middleware,
//! reports committee activity within `Config::csb_alert_hours`, once per user
//! and peer address per `CSB_ALERT_HOURS_REPEAT_INTERVAL`. Both are no-ops
//! while their setting is unset, and both emit a warning with an `event` marker
//! for monitoring to alert on.
//!
//! The IP gate is layered over the dedicated CSB listener only: a committee
//! session correcting paper documents uses the political-group routes as
//! well, so gating by path would leave part of the committee's traffic open.
//! `Config` refuses an allow list without `CSB_BIND_ADDRESS` for the same
//! reason.

use std::{
    net::{IpAddr, SocketAddr},
    time::Instant,
};

use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use tracing::warn;

use crate::{AppState, Session, SessionUser, constants::DEFAULT_TIMEZONE};

/// The connection's peer address, recorded by the server through
/// `into_make_service_with_connect_info`.
fn peer_ip(request: &Request) -> Option<IpAddr> {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip())
}

/// The peer address for a log field; `unknown` when none was recorded.
fn peer_ip_label(request: &Request) -> String {
    peer_ip(request).map_or_else(|| "unknown".to_string(), |ip| ip.to_string())
}

/// Refuses requests whose peer address is not on the allow list with `403`.
/// A request without a recorded peer address is refused as well, so the gate
/// fails closed. Alerting matches on `event = "csb.ip_denied"`.
pub async fn csb_ip_allow_list_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let Some(allow_list) = state.config.csb_ip_allow_list.as_ref() else {
        return next.run(request).await;
    };

    if peer_ip(&request).is_some_and(|ip| allow_list.allows(ip)) {
        return next.run(request).await;
    }

    warn!(
        event = "csb.ip_denied",
        ip = %peer_ip_label(&request),
        method = %request.method(),
        path = %request.uri().path(),
        "request from an IP outside the CSB allow list refused"
    );
    StatusCode::FORBIDDEN.into_response()
}

/// Reports committee activity within the alert hours without blocking it,
/// once per user and peer address per `CSB_ALERT_HOURS_REPEAT_INTERVAL`.
/// Alerting matches on `event = "csb.alert_hours_activity"`.
pub(super) fn report_alert_hours_activity(state: &AppState, session: &Session, request: &Request) {
    let Some(hours) = state.config.csb_alert_hours.as_ref() else {
        return;
    };
    let SessionUser::CentralElectoralCommittee { user, .. } = &session.user else {
        return;
    };

    let now = Utc::now().with_timezone(DEFAULT_TIMEZONE);
    if !hours.contains(now.time()) {
        return;
    }

    let key = (user.clone(), peer_ip(request));
    if !state.csb_alert_throttle.allows(key, Instant::now()) {
        return;
    }

    warn!(
        event = "csb.alert_hours_activity",
        user = ?user,
        ip = %peer_ip_label(request),
        method = %request.method(),
        path = %request.uri().path(),
        local_time = %now.format("%H:%M"),
        "CSB user active within the alert hours"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::Request, middleware, routing::get};
    use chrono::TimeDelta;
    use tower::ServiceExt;
    use tracing_test::traced_test;

    use crate::{
        AppState, Config, SESSION_COOKIE_NAME,
        core::{CsbAlertHours, CsbIpAllowList},
        session_middleware,
    };

    async fn ip_gate_app(allow_list: Option<&str>) -> Router {
        let mut config = Config::new_test();
        config.csb_ip_allow_list = allow_list.map(|raw| CsbIpAllowList::parse(raw).expect("list"));
        let state = AppState::new_for_tests_with_config(config).await;

        Router::new()
            .route("/probe", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                csb_ip_allow_list_middleware,
            ))
            .with_state(state)
    }

    fn probe_from(peer: Option<&str>) -> Request<Body> {
        let mut request = Request::builder()
            .uri("/probe")
            .body(Body::empty())
            .unwrap();
        if let Some(peer) = peer {
            let addr: SocketAddr = peer.parse().expect("socket address");
            request.extensions_mut().insert(ConnectInfo(addr));
        }
        request
    }

    #[tokio::test]
    async fn ip_gate_passes_every_peer_when_unset() {
        let app = ip_gate_app(None).await;

        for peer in [None, Some("203.0.113.8:4000")] {
            let response = app.clone().oneshot(probe_from(peer)).await.unwrap();

            assert_eq!(response.status(), StatusCode::OK, "{peer:?}");
        }
    }

    #[tokio::test]
    async fn ip_gate_accepts_a_listed_peer() {
        let app = ip_gate_app(Some("203.0.113.7, 10.0.0.0/8")).await;

        for peer in [
            "203.0.113.7:4000",
            "10.1.2.3:4000",
            "[::ffff:10.1.2.3]:4000",
        ] {
            let response = app.clone().oneshot(probe_from(Some(peer))).await.unwrap();

            assert_eq!(response.status(), StatusCode::OK, "{peer}");
        }
    }

    #[traced_test]
    #[tokio::test]
    async fn ip_gate_refuses_and_reports_an_unlisted_peer() {
        let app = ip_gate_app(Some("203.0.113.7")).await;

        let response = app
            .oneshot(probe_from(Some("203.0.113.8:4000")))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(logs_contain("csb.ip_denied"));
        assert!(logs_contain("203.0.113.8"));
    }

    /// Without a recorded peer address the gate fails closed.
    #[traced_test]
    #[tokio::test]
    async fn ip_gate_refuses_a_request_without_peer_address() {
        let app = ip_gate_app(Some("203.0.113.7")).await;

        let response = app.oneshot(probe_from(None)).await.unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(logs_contain("csb.ip_denied"));
        assert!(logs_contain("unknown"));
    }

    /// A two-hour window around now, or the two hours starting after it.
    fn hours_relative_to_now(containing_now: bool) -> CsbAlertHours {
        let now = Utc::now().with_timezone(DEFAULT_TIMEZONE).time();
        let offset = if containing_now {
            TimeDelta::hours(-1)
        } else {
            TimeDelta::hours(1)
        };
        // `overflowing_add_signed` wraps around midnight and drops the day.
        CsbAlertHours {
            start: now.overflowing_add_signed(offset).0,
            end: now.overflowing_add_signed(offset + TimeDelta::hours(2)).0,
        }
    }

    /// The session middleware (which runs the alert-hours check) with the
    /// cookie of a session for `session`.
    async fn alert_hours_app(hours: CsbAlertHours, session: Session) -> (Router, String) {
        let mut config = Config::new_test();
        config.csb_alert_hours = Some(hours);
        let state = AppState::new_for_tests_with_config(config).await;
        let token = session.token_string();
        state.sessions.insert(session).await;

        let app = Router::new()
            .route("/probe", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                session_middleware,
            ))
            .with_state(state);
        (app, format!("{SESSION_COOKIE_NAME}={token}"))
    }

    async fn probe_with_cookie(app: Router, cookie: &str) -> Response {
        let mut request = probe_from(Some("203.0.113.7:4000"));
        request
            .headers_mut()
            .insert(axum::http::header::COOKIE, cookie.parse().unwrap());
        app.oneshot(request).await.unwrap()
    }

    #[traced_test]
    #[tokio::test]
    async fn alert_hours_report_committee_activity_without_blocking() {
        let (app, cookie) =
            alert_hours_app(hours_relative_to_now(true), Session::new_test_committee()).await;

        let response = probe_with_cookie(app, &cookie).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert!(logs_contain("csb.alert_hours_activity"));
        assert!(logs_contain("203.0.113.7"));
    }

    /// A second request by the same user from the same address within the
    /// repeat interval is not reported again.
    #[traced_test]
    #[tokio::test]
    async fn alert_hours_report_a_user_and_address_once_per_interval() {
        let (app, cookie) =
            alert_hours_app(hours_relative_to_now(true), Session::new_test_committee()).await;

        probe_with_cookie(app.clone(), &cookie).await;
        probe_with_cookie(app, &cookie).await;

        logs_assert(|lines| {
            let reports = lines
                .iter()
                .filter(|line| line.contains("csb.alert_hours_activity"))
                .count();
            if reports == 1 {
                Ok(())
            } else {
                Err(format!("expected one report, got {reports}"))
            }
        });
    }

    #[traced_test]
    #[tokio::test]
    async fn alert_hours_stay_quiet_outside_the_window() {
        let (app, cookie) =
            alert_hours_app(hours_relative_to_now(false), Session::new_test_committee()).await;

        let response = probe_with_cookie(app, &cookie).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert!(!logs_contain("csb.alert_hours_activity"));
    }

    /// Political-group sessions are not committee activity.
    #[traced_test]
    #[tokio::test]
    async fn alert_hours_ignore_political_group_sessions() {
        let (app, cookie) = alert_hours_app(hours_relative_to_now(true), Session::new_test()).await;

        let response = probe_with_cookie(app, &cookie).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert!(!logs_contain("csb.alert_hours_activity"));
    }
}
