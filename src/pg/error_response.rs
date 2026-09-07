use askama::Template;
use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use tracing::{error, warn};

use crate::{
    AppError, Context, HtmlTemplate, Locale, LocaleValues, auth::csrf_guard::CsrfRejection,
    filters, trans,
};

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

#[derive(Template, Clone)]
#[template(path = "pg/common/pages/error.html")]
pub struct ErrorTemplate {
    pub status_code: StatusCode,
    pub title: String,
    message: String,
    limit: Option<LimitMessage>,
}

impl ErrorTemplate {
    /// Swap in the localised texts where they exist (the 429 pages); other
    /// errors keep their English text.
    fn localise(mut self, locale: Locale) -> Self {
        if let Some(limit) = self.limit {
            self.title = limit.title(locale);
            self.message = limit.message(locale);
        }
        self
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

        let error_template = ErrorTemplate {
            status_code,
            title: error.title().to_string(),
            message,
            limit,
        };

        let mut response = status_code.into_response();
        response.extensions_mut().insert(error_template);
        response
    }
}

#[derive(Template)]
#[template(path = "pg/common/pages/request_error.html")]
struct RequestErrorTemplate {
    title: String,
    message: String,
}

/// Styled CSRF rejection page, rendered directly with the session locale
/// (the rejection short-circuits above `render_error_pages`).
pub(crate) fn csrf_rejection_response(rejection: CsrfRejection, locale: Locale) -> Response {
    let (status_code, title, message) = match rejection {
        CsrfRejection::InvalidToken => (
            StatusCode::BAD_REQUEST,
            trans!("common.request_error.csrf_title", locale),
            trans!("common.request_error.csrf_message", locale),
        ),
        CsrfRejection::BodyTooLarge => (
            StatusCode::PAYLOAD_TOO_LARGE,
            trans!("common.request_error.too_large_title", locale),
            trans!("common.request_error.too_large_message", locale),
        ),
        CsrfRejection::UnreadableBody => (
            StatusCode::BAD_REQUEST,
            trans!("common.request_error.unreadable_title", locale),
            trans!("common.request_error.unreadable_message", locale),
        ),
    };
    warn!(?rejection, "mutating request rejected by CSRF enforcement");

    let mut response = HtmlTemplate(
        RequestErrorTemplate { title, message },
        LocaleValues { locale },
    )
    .into_response();
    *response.status_mut() = status_code;
    response
}

/// Middleware to render error pages based on ErrorTemplate in response extensions
pub async fn render_error_pages(context: Context, request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;

    match response.extensions_mut().remove::<ErrorTemplate>() {
        None => response,
        Some(error_template) => {
            let error_template = error_template.localise(context.session.locale);
            (
                error_template.status_code,
                HtmlTemplate(error_template, context),
            )
                .into_response()
        }
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
    use crate::{AppError, AppState, Context, Form, HtmlTemplate, Locale, test_utils};
    use axum::{
        Router,
        body::Body,
        extract::{
            FromRequest, Multipart, Path, Request,
            rejection::{JsonRejection, MissingJsonContentType},
        },
        http::StatusCode,
        middleware,
        response::IntoResponse,
        routing::get,
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn not_found_renders_template_with_message() {
        let state = AppState::new_for_tests().await;
        let store = crate::PgStore::new_for_test();
        let app = Router::new()
            .route(
                "/",
                get(|| async { AppError::NotFound("missing".to_string()) }),
            )
            .layer(middleware::from_fn_with_state(state, render_error_pages));

        let mut request = Request::builder().uri("/").body(Body::empty()).unwrap();
        let session = crate::Session::new_test_with_locale(Locale::En);
        request.extensions_mut().insert(session);
        request.extensions_mut().insert(store);
        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = test_utils::response_body_string(response).await;
        assert!(body.contains("Error code 404"));
        assert!(body.contains("missing"));
    }

    #[cfg(feature = "database")]
    #[tokio::test]
    async fn database_error_maps_to_internal_server_error() {
        let state = AppState::new_for_tests().await;
        let store = crate::PgStore::new_for_test();
        let app = Router::new()
            .route(
                "/",
                get(|| async { AppError::DatabaseError(sqlx::Error::RowNotFound) }),
            )
            .layer(middleware::from_fn_with_state(state, render_error_pages));
        let mut request = Request::builder().uri("/").body(Body::empty()).unwrap();
        let session = crate::Session::new_test_with_locale(Locale::En);
        request.extensions_mut().insert(session);
        request.extensions_mut().insert(store);
        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[cfg(feature = "acme")]
    #[tokio::test]
    async fn acme_error_maps_to_internal_server_error() {
        let state = AppState::new_for_tests().await;
        let store = crate::PgStore::new_for_test();
        let app = Router::new()
            .route(
                "/",
                get(|| async { AppError::AcmeError(instant_acme::Error::Str("boom")) }),
            )
            .layer(middleware::from_fn_with_state(state, render_error_pages));
        let mut request = Request::builder().uri("/").body(Body::empty()).unwrap();
        let session = crate::Session::new_test_with_locale(Locale::En);

        request.extensions_mut().insert(session);
        request.extensions_mut().insert(store);
        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    /// Both rate-limit errors become a 429 whose page is translated with the
    /// session locale.
    #[tokio::test]
    async fn rate_limit_errors_map_to_too_many_requests() {
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
            let response = error.into_response();

            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

            let error_template = response
                .extensions()
                .get::<ErrorTemplate>()
                .expect("error template")
                .clone()
                .localise(locale);
            let html = test_utils::response_body_string(
                (
                    error_template.status_code,
                    HtmlTemplate(error_template, Context::new_test_without_db()),
                )
                    .into_response(),
            )
            .await;

            assert!(html.contains("Error code 429"), "{html}");
            assert!(html.contains(expected), "{html}");
        }
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

            let error_response = ErrorResponse::from(error);
            let response = error_response.into_response();
            let error_template = response.extensions().get::<ErrorTemplate>().unwrap();
            let content = error_template.title.clone();
            let context = Context::new_test_without_db();
            let html_response = (
                error_template.status_code,
                HtmlTemplate(error_template, context),
            )
                .into_response();

            assert_eq!(html_response.status(), response.status());

            let body = test_utils::response_body_string(html_response).await;

            assert!(body.contains(&content));
        }
    }
}
