//! Per-request tracing layer for HTTP traffic.
//!
//! Builds an `http_request` span carrying the HTTP method, matched route
//! template, and the request path (never the query string, which can contain
//! tokens). Emits one event per response whose level reflects the status
//! class: 5xx → ERROR, 4xx → WARN, otherwise INFO.
//!
//! The CDN's request id, visitor country code and bot verdict headers are
//! recorded when present. Their values are bounded and checked first: this
//! layer runs before the `x-eks-key` gate, so a direct request can carry any
//! header it likes.

use std::time::Duration;

use axum::{
    body::Body,
    extract::MatchedPath,
    http::{HeaderMap, Request, Response, header},
};
use tower_http::{
    classify::{ServerErrorsAsFailures, ServerErrorsFailureClass, SharedClassifier},
    trace::TraceLayer,
};
use tracing::{Level, Span, field};

type MakeSpan = fn(&Request<Body>) -> Span;
type OnResponse = fn(&Response<Body>, Duration, &Span);
type OnFailure = fn(ServerErrorsFailureClass, Duration, &Span);

pub fn layer()
-> TraceLayer<SharedClassifier<ServerErrorsAsFailures>, MakeSpan, (), OnResponse, (), (), OnFailure>
{
    TraceLayer::new_for_http()
        .make_span_with(make_span as MakeSpan)
        .on_request(())
        .on_response(on_response as OnResponse)
        .on_body_chunk(())
        .on_eos(())
        .on_failure(on_failure as OnFailure)
}

const CDN_REQUEST_ID: &str = "cdn-requestid";
const CDN_REQUEST_COUNTRY_CODE: &str = "cdn-requestcountrycode";
/// Only sent when the CDN classifies the request as a bot.
const CDN_BOT: &str = "cdn-bot";

fn make_span(request: &Request<Body>) -> Span {
    let method = request.method().as_str();
    let path = request.uri().path();

    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str)
        .unwrap_or(path);

    let span = tracing::info_span!(
        "http_request",
        method = %method,
        route = %route,
        path = %path,
        request_id = field::Empty,
        country_code = field::Empty,
        bot = field::Empty,
        status = field::Empty,
        duration_ms = field::Empty,
        response_size = field::Empty,
    );

    let headers = request.headers();
    if let Some(id) = cdn_request_id(headers) {
        span.record("request_id", id);
    }
    if let Some(code) = cdn_country_code(headers) {
        span.record("country_code", code);
    }
    if let Some(bot) = cdn_bot(headers) {
        span.record("bot", bot);
    }

    span
}

/// The `CDN-RequestId` value when it is a plausible id: at most 64 ASCII
/// letters, digits, `-` or `_`.
fn cdn_request_id(headers: &HeaderMap) -> Option<&str> {
    header_str(headers, CDN_REQUEST_ID).filter(|value| {
        value.len() <= 64
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    })
}

/// The `CDN-RequestCountryCode` value when it is two ASCII uppercase letters.
fn cdn_country_code(headers: &HeaderMap) -> Option<&str> {
    header_str(headers, CDN_REQUEST_COUNTRY_CODE)
        .filter(|value| value.len() == 2 && value.bytes().all(|b| b.is_ascii_uppercase()))
}

/// The `CDN-Bot` value when it is at most 128 printable ASCII characters.
fn cdn_bot(headers: &HeaderMap) -> Option<&str> {
    header_str(headers, CDN_BOT).filter(|value| {
        value.len() <= 128 && value.bytes().all(|b| b.is_ascii_graphic() || b == b' ')
    })
}

/// A non-empty header value as a string.
fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
}

fn on_response(response: &Response<Body>, latency: Duration, span: &Span) {
    let status = response.status().as_u16();
    let duration_ms = latency.as_secs_f64() * 1000.0;
    let response_size = response
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());

    span.record("status", status);
    span.record("duration_ms", duration_ms);
    if let Some(size) = response_size {
        span.record("response_size", size);
    }

    emit_response_event(span, status);
}

/// Emits the per-response event at the level matching the status class.
#[expect(
    clippy::cognitive_complexity,
    reason = "`event!` requires a const level, and its expansion inflates the metric."
)]
fn emit_response_event(span: &Span, status: u16) {
    match status {
        500.. => tracing::event!(parent: span, Level::ERROR, "http request"),
        400.. => tracing::event!(parent: span, Level::WARN, "http request"),
        _ => tracing::event!(parent: span, Level::INFO, "http request"),
    }
}

fn on_failure(error: ServerErrorsFailureClass, latency: Duration, span: &Span) {
    let duration_ms = latency.as_secs_f64() * 1000.0;
    tracing::event!(
        parent: span,
        Level::ERROR,
        duration_ms = duration_ms,
        error = %error,
        "http request failed",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(name: &'static str, value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(name, HeaderValue::from_str(value).unwrap());
        headers
    }

    #[test]
    fn request_id_accepts_cdn_style_ids() {
        for id in [
            "3f2a9c1e4b7d",
            "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
            "x_1",
        ] {
            assert_eq!(cdn_request_id(&headers(CDN_REQUEST_ID, id)), Some(id));
        }
    }

    #[test]
    fn request_id_rejects_long_or_odd_values() {
        let too_long = "a".repeat(65);
        for id in [too_long.as_str(), "", "id with space", "id\"}", "ünïcode"] {
            assert_eq!(cdn_request_id(&headers(CDN_REQUEST_ID, id)), None, "{id:?}");
        }
        assert_eq!(cdn_request_id(&HeaderMap::new()), None);
    }

    #[test]
    fn country_code_is_two_uppercase_letters() {
        assert_eq!(
            cdn_country_code(&headers(CDN_REQUEST_COUNTRY_CODE, "NL")),
            Some("NL")
        );
        for code in ["nl", "NLD", "N", "1A", ""] {
            assert_eq!(
                cdn_country_code(&headers(CDN_REQUEST_COUNTRY_CODE, code)),
                None,
                "{code:?}"
            );
        }
    }

    #[test]
    fn bot_is_bounded_printable_ascii() {
        let verdict = "verified=true action=allow";
        assert_eq!(cdn_bot(&headers(CDN_BOT, verdict)), Some(verdict));

        let too_long = "b".repeat(129);
        for value in [too_long.as_str(), "", "tab\there"] {
            assert_eq!(cdn_bot(&headers(CDN_BOT, value)), None, "{value:?}");
        }
    }

    #[test]
    fn header_names_match_case_insensitively() {
        let mut map = HeaderMap::new();
        map.insert("CDN-RequestId", HeaderValue::from_static("abc123"));
        assert_eq!(cdn_request_id(&map), Some("abc123"));
    }
}
