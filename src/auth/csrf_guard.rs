//! CSRF token verification for mutating requests, invoked by the session
//! middleware so no session-guarded route can skip it.

use axum::{
    body::{Body, Bytes, to_bytes},
    extract::Request,
    http::{HeaderMap, Method, Uri, header::CONTENT_TYPE},
};

use crate::Session;

/// Form field carrying the CSRF token.
pub const CSRF_FORM_FIELD: &str = "csrf_token";

/// Header carrying the token for JS `fetch` requests (see the layout's
/// `csrf-token` meta tag).
pub const CSRF_HEADER: &str = "x-csrf-token";

/// Urlencoded buffering cap, matching axum's default body limit.
const URLENCODED_BODY_CAP: usize = 2 * 1024 * 1024;

/// Methods that never mutate state and skip CSRF verification.
pub(crate) fn is_safe_method(method: &Method) -> bool {
    matches!(method, &Method::GET | &Method::HEAD | &Method::OPTIONS)
}

/// Why a mutating request failed CSRF enforcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CsrfRejection {
    /// No token, or a token that does not match the session's.
    InvalidToken,
    /// The body exceeds the buffering cap for its content type.
    BodyTooLarge,
    /// The body could not be read (client abort, transport error).
    UnreadableBody,
}

/// Verify a mutating request's token against the session's;
/// a buffered form body is handed back intact on the returned request.
pub(crate) async fn enforce_csrf(
    request: Request,
    session: &Session,
) -> Result<Request, CsrfRejection> {
    if is_safe_method(request.method()) {
        return Ok(request);
    }

    if let Some(token) = header_token(request.headers()) {
        return if session.csrf_matches(token) {
            Ok(request)
        } else {
            Err(CsrfRejection::InvalidToken)
        };
    }

    // Multipart bodies (file uploads) are never buffered or parsed here: their
    // token travels in the form's `action` query string. The token is useless
    // without the session cookie, so its exposure in request logs is accepted.
    if is_multipart(request.headers()) {
        return match query_token(request.uri()) {
            Some(token) if session.csrf_matches(&token) => Ok(request),
            _ => Err(CsrfRejection::InvalidToken),
        };
    }

    // Anything but an urlencoded form body cannot carry a token at this point.
    if !is_urlencoded(request.headers()) {
        return Err(CsrfRejection::InvalidToken);
    }

    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, URLENCODED_BODY_CAP).await.map_err(|err| {
        if is_length_limit_error(&err) {
            CsrfRejection::BodyTooLarge
        } else {
            CsrfRejection::UnreadableBody
        }
    })?;

    match urlencoded_token(&bytes) {
        Some(token) if session.csrf_matches(&token) => {
            Ok(Request::from_parts(parts, Body::from(bytes)))
        }
        _ => Err(CsrfRejection::InvalidToken),
    }
}

fn content_type(headers: &HeaderMap) -> Option<&str> {
    headers.get(CONTENT_TYPE)?.to_str().ok()
}

fn is_urlencoded(headers: &HeaderMap) -> bool {
    content_type(headers).is_some_and(|ct| ct.starts_with("application/x-www-form-urlencoded"))
}

fn is_multipart(headers: &HeaderMap) -> bool {
    content_type(headers).is_some_and(|ct| ct.starts_with("multipart/form-data"))
}

fn header_token(headers: &HeaderMap) -> Option<&str> {
    headers.get(CSRF_HEADER)?.to_str().ok()
}

/// True when the token travelled in the header, which only a script can set:
/// the request came from a background `fetch`, not a form navigation.
pub(crate) fn is_header_token_request(headers: &HeaderMap) -> bool {
    headers.contains_key(CSRF_HEADER)
}

fn query_token(uri: &Uri) -> Option<String> {
    url::form_urlencoded::parse(uri.query()?.as_bytes())
        .find(|(name, _)| name == CSRF_FORM_FIELD)
        .map(|(_, value)| value.into_owned())
}

/// True when `to_bytes` failed on the size cap rather than a transport error.
fn is_length_limit_error(err: &axum::Error) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(err);
    while let Some(current) = source {
        if current.is::<http_body_util::LengthLimitError>() {
            return true;
        }
        source = current.source();
    }
    false
}

fn urlencoded_token(bytes: &Bytes) -> Option<String> {
    url::form_urlencoded::parse(bytes)
        .find(|(name, _)| name == CSRF_FORM_FIELD)
        .map(|(_, value)| value.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};

    fn urlencoded_request(body: String) -> Request {
        Request::builder()
            .method("POST")
            .uri("/submit")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap()
    }

    fn multipart_request(uri: &str, boundary: &str, body: String) -> Request {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header(
                CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap()
    }

    fn multipart_body(boundary: &str, file_data: &str) -> String {
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file_data\"; filename=\"a.csv\"\r\nContent-Type: text/csv\r\n\r\n{file_data}\r\n--{boundary}--\r\n"
        )
    }

    /// A POST carrying the session's token in an urlencoded body passes, and
    /// the body is handed back intact for the downstream extractor.
    #[tokio::test]
    async fn accepts_valid_urlencoded_token() {
        let session = Session::new_test();
        let body = format!("a=b&csrf_token={}", session.csrf_token());

        let request = urlencoded_request(body.clone());
        let request = enforce_csrf(request, &session).await.expect("accepted");

        let bytes = to_bytes(request.into_body(), usize::MAX).await.unwrap();
        assert_eq!(bytes, body.as_bytes());
    }

    /// A wrong token is rejected.
    #[tokio::test]
    async fn rejects_invalid_token() {
        let session = Session::new_test();

        let result = enforce_csrf(urlencoded_request("csrf_token=wrong".into()), &session).await;

        assert_eq!(result.err(), Some(CsrfRejection::InvalidToken));
    }

    /// A POST without any token is rejected.
    #[tokio::test]
    async fn rejects_missing_token() {
        let session = Session::new_test();

        let result = enforce_csrf(urlencoded_request("a=b".into()), &session).await;

        assert_eq!(result.err(), Some(CsrfRejection::InvalidToken));
    }

    /// Safe methods pass through without a token.
    #[tokio::test]
    async fn safe_methods_pass_without_token() {
        let session = Session::new_test();
        let request = Request::builder().uri("/page").body(Body::empty()).unwrap();

        assert!(enforce_csrf(request, &session).await.is_ok());
    }

    /// Multipart POSTs (file uploads) carry the token in the query string and
    /// their body reaches the handler untouched, without being buffered here.
    #[tokio::test]
    async fn accepts_multipart_with_valid_query_token() {
        let session = Session::new_test();
        let boundary = "test-boundary";
        let body = multipart_body(boundary, "data");
        let uri = format!("/submit?csrf_token={}", session.csrf_token());

        let request = multipart_request(&uri, boundary, body.clone());
        let request = enforce_csrf(request, &session).await.expect("accepted");

        let bytes = to_bytes(request.into_body(), usize::MAX).await.unwrap();
        assert_eq!(bytes, body.as_bytes());
    }

    /// A multipart POST with a wrong query token is rejected.
    #[tokio::test]
    async fn rejects_multipart_with_invalid_query_token() {
        let session = Session::new_test();
        let boundary = "test-boundary";
        let body = multipart_body(boundary, "data");

        let result = enforce_csrf(
            multipart_request("/submit?csrf_token=wrong", boundary, body),
            &session,
        )
        .await;

        assert_eq!(result.err(), Some(CsrfRejection::InvalidToken));
    }

    /// A token inside the multipart body is not consulted: without a query
    /// token the request is rejected (the body is never parsed).
    #[tokio::test]
    async fn rejects_multipart_without_query_token() {
        let session = Session::new_test();
        let boundary = "test-boundary";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"csrf_token\"\r\n\r\n{}\r\n--{boundary}--\r\n",
            session.csrf_token()
        );

        let result = enforce_csrf(multipart_request("/submit", boundary, body), &session).await;

        assert_eq!(result.err(), Some(CsrfRejection::InvalidToken));
    }

    /// An undeclared (streamed) body that exceeds the cap is also 413, not a
    /// generic read error.
    #[tokio::test]
    async fn rejects_streamed_oversize_urlencoded_body() {
        let session = Session::new_test();
        let mut request = urlencoded_request("a".repeat(URLENCODED_BODY_CAP + 1));
        request.headers_mut().remove(CONTENT_LENGTH);

        let result = enforce_csrf(request, &session).await;

        assert_eq!(result.err(), Some(CsrfRejection::BodyTooLarge));
    }

    /// JSON requests authenticate via the `X-CSRF-Token` header and keep their
    /// body untouched.
    #[tokio::test]
    async fn accepts_valid_header_token_for_json() {
        let session = Session::new_test();
        let request = Request::builder()
            .method("POST")
            .uri("/reorder")
            .header(CONTENT_TYPE, "application/json")
            .header(CSRF_HEADER, session.csrf_token().0.clone())
            .body(Body::from(r#"{"person_ids":[]}"#))
            .unwrap();

        let request = enforce_csrf(request, &session).await.expect("accepted");

        let bytes = to_bytes(request.into_body(), usize::MAX).await.unwrap();
        assert_eq!(bytes, r#"{"person_ids":[]}"#.as_bytes());
    }

    /// A wrong header token is rejected.
    #[tokio::test]
    async fn rejects_invalid_header_token() {
        let session = Session::new_test();
        let request = Request::builder()
            .method("POST")
            .uri("/reorder")
            .header(CONTENT_TYPE, "application/json")
            .header(CSRF_HEADER, "wrong")
            .body(Body::empty())
            .unwrap();

        let result = enforce_csrf(request, &session).await;

        assert_eq!(result.err(), Some(CsrfRejection::InvalidToken));
    }

    /// A JSON POST without a header token cannot carry a token at all.
    #[tokio::test]
    async fn rejects_json_without_header_token() {
        let session = Session::new_test();
        let request = Request::builder()
            .method("POST")
            .uri("/reorder")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .unwrap();

        let result = enforce_csrf(request, &session).await;

        assert_eq!(result.err(), Some(CsrfRejection::InvalidToken));
    }
}
