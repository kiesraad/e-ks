//! The response an [`AppError`] becomes: its status code, with the content of
//! the error page attached as an [`ErrorPage`] extension.
//!
//! The page is not rendered here. Each web section has an error-page layer
//! (`render_error_pages` on the app routes, `render_csb_error_pages` on the
//! CSB routes) that takes the page off the response with
//! [`ErrorPage::take_from`] and renders it in its own layout, so the mapping
//! from error to status code and texts lives in one place.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use tracing::{error, warn};

use crate::{AppError, Locale, trans};

/// Variants of error responses that can be sent to the client
#[derive(Serialize)]
enum ErrorResponseVariant {
    Unauthorised,
    BadRequest,
    TooManyRequests,
    InternalServerError,
    ServiceUnavailable,
    NotFound,
}

impl ErrorResponseVariant {
    fn status_code(&self) -> StatusCode {
        match self {
            ErrorResponseVariant::NotFound => StatusCode::NOT_FOUND,
            ErrorResponseVariant::BadRequest => StatusCode::BAD_REQUEST,
            ErrorResponseVariant::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            ErrorResponseVariant::Unauthorised => StatusCode::UNAUTHORIZED,
            ErrorResponseVariant::InternalServerError => StatusCode::INTERNAL_SERVER_ERROR,
            ErrorResponseVariant::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    fn title(&self) -> &'static str {
        match self {
            ErrorResponseVariant::Unauthorised => "Unauthorised",
            ErrorResponseVariant::BadRequest => "Bad request",
            ErrorResponseVariant::TooManyRequests => "Too many requests",
            ErrorResponseVariant::InternalServerError => "Internal server error",
            ErrorResponseVariant::ServiceUnavailable => "Service unavailable",
            ErrorResponseVariant::NotFound => "Not found",
        }
    }
}

/// The rate-limit (429) page texts. The locale is not known where an
/// [`AppError`] becomes a response, so these are translated at render time.
#[derive(Clone, Copy, Serialize)]
enum LimitMessage {
    Downloads,
    Events,
    Cap,
}

impl LimitMessage {
    fn from_error(err: &AppError) -> Option<Self> {
        match err {
            AppError::TooManyDownloads { .. } => Some(Self::Downloads),
            AppError::TooManyEvents { .. } => Some(Self::Events),
            AppError::EventLimitReached { .. } => Some(Self::Cap),
            _ => None,
        }
    }

    fn title(self, locale: Locale) -> String {
        trans!("common.rate_limit.title", locale)
    }

    fn message(self, locale: Locale) -> String {
        match self {
            Self::Downloads => trans!("common.rate_limit.downloads_message", locale),
            Self::Events => trans!("common.rate_limit.events_message", locale),
            Self::Cap => trans!("common.rate_limit.cap_message", locale),
        }
    }
}

/// Struct representing an error response to be sent to the client
#[derive(Serialize)]
pub struct ErrorResponse {
    error: ErrorResponseVariant,
    message: String,
    /// Set for rate-limit errors, whose texts are translated at render time.
    limit: Option<LimitMessage>,
}

/// The content of an error page. Carried on the response as an extension
/// until a section's error-page layer renders it in that section's layout.
#[derive(Clone)]
pub struct ErrorPage {
    pub status_code: StatusCode,
    pub title: String,
    pub message: String,
    /// Set for rate-limit errors, whose texts are translated at render time.
    limit: Option<LimitMessage>,
}

impl ErrorPage {
    /// Swap in the localised texts where they exist (the 429 pages); other
    /// errors keep their English text.
    fn localise(mut self, locale: Locale) -> Self {
        if let Some(limit) = self.limit {
            self.title = limit.title(locale);
            self.message = limit.message(locale);
        }
        self
    }

    /// Remove the error page an [`ErrorResponse`] attached to `response`, if
    /// any, with its texts localised for `locale`.
    pub fn take_from(response: &mut Response, locale: Locale) -> Option<Self> {
        let page = response.extensions_mut().remove::<Self>()?;
        Some(page.localise(locale))
    }
}

/// Convert ErrorResponse into an HTTP response
impl IntoResponse for ErrorResponse {
    fn into_response(self) -> Response {
        let ErrorResponse {
            error,
            message,
            limit,
        } = self;
        let status_code = error.status_code();

        let page = ErrorPage {
            status_code,
            title: error.title().to_string(),
            message,
            limit,
        };

        let mut response = status_code.into_response();
        response.extensions_mut().insert(page);
        response
    }
}

/// Convert AppError into an HTTP response, via the ErrorResponse struct
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        ErrorResponse::from_app_error(&self).into_response()
    }
}

/// Convert AppError into ErrorResponse, the AppError contains more information
/// that should not be exposed to the client, but should be logged at this point.
impl From<AppError> for ErrorResponse {
    fn from(err: AppError) -> Self {
        ErrorResponse::from_app_error(&err)
    }
}

impl ErrorResponse {
    fn from_app_error(err: &AppError) -> Self {
        // Infrastructure failures (database unreachable, broken schema) become a
        // 503 so clients and proxies can retry, regardless of which variant
        // carried the failure. Everything else maps per-variant in `build`.
        let response = if err.is_infrastructure_failure() {
            Self::service_unavailable()
        } else {
            Self::build(err)
        };
        log_app_error(err, &response);
        response
    }

    /// The temporary-outage response shared by every infrastructure failure.
    fn service_unavailable() -> Self {
        ErrorResponse {
            error: ErrorResponseVariant::ServiceUnavailable,
            message: "The service is temporarily unavailable. Please try again shortly."
                .to_string(),
            limit: None,
        }
    }

    fn build(err: &AppError) -> Self {
        use ErrorResponseVariant::*;

        let internal = || {
            (
                InternalServerError,
                "An internal server error occurred.".to_string(),
            )
        };

        let (error, message) = match err {
            AppError::NotFound(msg) => (NotFound, msg.to_string()),
            AppError::GenericNotFound => (NotFound, "Page not found".to_string()),
            AppError::Unauthorised => (
                Unauthorised,
                "You are not authorised to perform this action.".to_string(),
            ),
            AppError::MultipartFormError(_)
            | AppError::MultipartError(_)
            | AppError::FormRejection(_)
            | AppError::PathRejection(_)
            | AppError::JsonRejection(_)
            | AppError::QueryRejection(_)
            | AppError::UserError(_)
            | AppError::TooManyCandidates { .. }
            | AppError::AmbiguousHash => (BadRequest, err.to_string()),
            AppError::TooManyDownloads { .. }
            | AppError::TooManyEvents { .. }
            | AppError::EventLimitReached { .. } => LimitMessage::from_error(err)
                .map_or_else(internal, |limit| {
                    (TooManyRequests, limit.message(Locale::En))
                }),
            AppError::EmlError(err) => (BadRequest, format!("EML error: {err}")),
            AppError::IncompleteData(err) => (
                BadRequest,
                format!("Missing data when generating PDF: {err}"),
            ),
            #[cfg(feature = "database")]
            AppError::DatabaseError(_) => internal(),
            #[cfg(feature = "acme")]
            AppError::AcmeError(_) => internal(),
            AppError::InternalServerError
            | AppError::NoStorageConfigured
            | AppError::IntegrityViolation
            | AppError::MissingEnvVar(_)
            | AppError::ConfigLoadError(_)
            | AppError::PdfError(_)
            | AppError::DocxError(_)
            | AppError::MarkdownError(_)
            | AppError::TemplateError(_)
            | AppError::UpstreamError(_)
            | AppError::ServerError(_)
            | AppError::EventDecodeError(_)
            | AppError::BrpError(_)
            | AppError::AuthError(_) => internal(),
        };

        ErrorResponse {
            error,
            message,
            limit: LimitMessage::from_error(err),
        }
    }
}

/// Emit a single tracing event for an error response.
fn log_app_error(err: &AppError, response: &ErrorResponse) {
    match response.error {
        ErrorResponseVariant::InternalServerError | ErrorResponseVariant::ServiceUnavailable => {
            error!(error = ?err, "5xx error");
        }
        ErrorResponseVariant::BadRequest
        | ErrorResponseVariant::TooManyRequests
        | ErrorResponseVariant::Unauthorised
        | ErrorResponseVariant::NotFound => log_client_error(err),
    }
}

/// Warn about a 4xx error without echoing request input into the log.
fn log_client_error(err: &AppError) {
    if message_is_safe_to_log(err) {
        warn!(error = ?err, "4xx error");
    } else {
        // Debug of inner extractor errors can echo request input, so
        // log only the variant name (the prefix of the Debug output).
        let dbg = format!("{err:?}");
        let kind = dbg.split_once('(').map_or(dbg.as_str(), |(n, _)| n);
        warn!(kind, "4xx error");
    }
}

/// Return `true` when the user-facing `ErrorResponse.message` for this
/// variant is a constant or developer-authored string and can be safely
/// included in the log event. For variants where the message is built from
/// the inner extractor/validation error (which can echo request input),
/// this returns `false`.
fn message_is_safe_to_log(err: &AppError) -> bool {
    matches!(
        err,
        AppError::Unauthorised
            | AppError::GenericNotFound
            | AppError::UserError(_)
            | AppError::NotFound(_)
            | AppError::IncompleteData(_)
            | AppError::TooManyDownloads { .. }
            | AppError::TooManyEvents { .. }
            | AppError::EventLimitReached { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppError, Form};
    use axum::{
        body::Body,
        extract::{
            FromRequest, Multipart, Path, Request,
            rejection::{JsonRejection, MissingJsonContentType},
        },
        http::StatusCode,
        response::IntoResponse,
    };

    /// Both rate-limit errors become a 429 whose page is translated with the
    /// locale of the layer that takes it.
    #[test]
    fn rate_limit_errors_map_to_too_many_requests() {
        let cases = [
            (
                AppError::TooManyDownloads {
                    max: 20,
                    window: chrono::TimeDelta::hours(1),
                },
                Locale::En,
                "Too many downloads",
            ),
            (
                AppError::EventLimitReached { max: 20_000 },
                Locale::En,
                "You can still view everything you entered",
            ),
            (
                AppError::TooManyEvents {
                    max: 20,
                    window: chrono::TimeDelta::hours(1),
                },
                Locale::Nl,
                "Te veel wijzigingen",
            ),
            (
                AppError::EventLimitReached { max: 20_000 },
                Locale::Nl,
                "Te veel verzoeken",
            ),
        ];

        for (error, locale, expected) in cases {
            let mut response = error.into_response();

            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

            let page = ErrorPage::take_from(&mut response, locale).expect("error page");
            assert_eq!(page.status_code, StatusCode::TOO_MANY_REQUESTS);
            let texts = format!("{} {}", page.title, page.message);
            assert!(texts.contains(expected), "{texts}");
        }
    }

    /// Taking the page removes it, so it is rendered by one layer only, and a
    /// response that carries no page yields none.
    #[test]
    fn take_from_removes_the_page() {
        let mut response = AppError::GenericNotFound.into_response();
        assert!(ErrorPage::take_from(&mut response, Locale::En).is_some());
        assert!(ErrorPage::take_from(&mut response, Locale::En).is_none());

        let mut response = "fine".into_response();
        assert!(ErrorPage::take_from(&mut response, Locale::En).is_none());
    }

    fn get_multipart_error_request() -> Request<Body> {
        let body = "--boundary\r\n\
                Content-Disposition: form-data; name=\"fiel";

        Request::builder()
            .method("POST")
            .uri("/upload")
            .header("Content-Type", "multipart/form-data; boundary=boundary")
            .body(Body::from(body))
            .unwrap()
    }

    fn get_multipart_rejection_request() -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/upload")
            .body(Body::from("not multipart"))
            .unwrap()
    }

    #[tokio::test]
    async fn app_error_variants_convert_to_error_response() {
        let form_rejection = Form::<bool>::from_request(
            Request::builder()
                .uri("/save")
                .body(Body::from("incorrect"))
                .unwrap(),
            &(),
        )
        .await
        .unwrap_err();
        let json_rejection: JsonRejection = MissingJsonContentType::default().into();
        let multipart_rejection = Multipart::from_request(get_multipart_rejection_request(), &())
            .await
            .unwrap_err();
        let mut multipart_form_result = Multipart::from_request(get_multipart_error_request(), &())
            .await
            .unwrap();
        let multipart_error = multipart_form_result.next_field().await.unwrap_err();
        let path_rejection = Path::<i32>::from_request(
            Request::builder()
                .uri("/not-a-number")
                .body(Body::empty())
                .unwrap(),
            &(),
        )
        .await
        .unwrap_err();

        let errors = vec![
            AppError::Unauthorised,
            AppError::InternalServerError,
            AppError::GenericNotFound,
            AppError::NotFound("missing".to_string()),
            AppError::from(askama::Error::Fmt),
            AppError::from(multipart_rejection),
            AppError::from(multipart_error),
            form_rejection,
            AppError::from(json_rejection),
            AppError::from(path_rejection),
            AppError::MissingEnvVar("STORAGE_URL"),
            AppError::ConfigLoadError("bad".to_string()),
            AppError::ServerError(std::io::Error::other("oh nooo")),
            #[cfg(feature = "database")]
            AppError::from(sqlx::Error::RowNotFound),
        ];

        for error in errors {
            let message = error.to_string();

            assert!(!message.is_empty());

            let mut response = ErrorResponse::from(error).into_response();
            let page = ErrorPage::take_from(&mut response, Locale::En).expect("error page");

            // The page and the response agree on the status, and every
            // variant carries texts to show.
            assert_eq!(page.status_code, response.status());
            assert!(!page.title.is_empty());
            assert!(!page.message.is_empty());
        }
    }
}
