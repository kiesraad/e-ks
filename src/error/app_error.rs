use axum::extract::{
    multipart::{MultipartError, MultipartRejection},
    rejection::{JsonRejection, PathRejection, QueryRejection},
};
use axum_extra::extract::FormRejection;
use chrono::TimeDelta;
use std::{
    convert::Infallible,
    fmt::{Display, Formatter},
};

/// Type alias for application responses
pub type AppResponse<T> = Result<T, AppError>;

/// Application wide error enum
#[derive(Default, Debug)]
pub enum AppError {
    // Request level errors
    Unauthorised,
    InternalServerError,
    #[default]
    GenericNotFound,
    NotFound(String),
    UserError(String),
    #[cfg(feature = "database")]
    DatabaseError(sqlx::Error),
    TemplateError(askama::Error),
    FormRejection(FormRejection),

    // Axum error types
    MultipartFormError(MultipartError),
    MultipartError(MultipartRejection),
    JsonRejection(JsonRejection),
    PathRejection(PathRejection),
    QueryRejection(QueryRejection),

    // Application level errors
    MissingEnvVar(&'static str),
    ConfigLoadError(String),
    ServerError(std::io::Error),
    UpstreamError(reqwest::Error),

    /// Missing or invalid data when generating a PDF.
    IncompleteData(&'static str),
    PdfError(textris_pdf::render::RenderError),
    MarkdownError(textris_pdf::markdown::MarkdownParseError),
    EmlError(eml_nl::EMLError),

    AuthError(auth_service::error::AuthError),

    #[cfg(feature = "acme")]
    AcmeError(instant_acme::Error),

    NoStorageConfigured,
    IntegrityViolation,

    /// Attempted to add a candidate to a list that is already at the maximum
    /// allowed number of candidates. Carries the limit that was exceeded.
    TooManyCandidates {
        max: usize,
    },

    /// A hash prefix matched more than one event; the user must supply a longer prefix.
    AmbiguousHash,

    /// The download rate limit was reached; clears once the window passes.
    TooManyDownloads {
        max: usize,
        window: TimeDelta,
    },

    /// The events-per-window rate limit was reached; clears once the window
    /// passes.
    TooManyEvents {
        max: usize,
        window: TimeDelta,
    },

    /// The absolute cap on the number of events in one stream is reached, so
    /// no new event is accepted. Reads are unaffected: the stream stays
    /// viewable.
    EventLimitReached {
        max: usize,
    },

    /// A persisted event could not be decrypted or deserialized.
    /// Indicates tampering, a wrong key, or a corrupt/unsupported frame.
    EventDecodeError(String),

    /// The BRP could not be consulted. Never a statement about a person: what
    /// the BRP says about a candidate is a
    /// [`crate::structs::brp::BrpFinding`], not an error.
    BrpError(String),
}

impl Display for AppError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::ConfigLoadError(err) => write!(f, "Configuration load error: {err}"),
            #[cfg(feature = "database")]
            AppError::DatabaseError(err) => write!(f, "Database error: {err}"),
            AppError::FormRejection(err) => write!(f, "Form error: {err}"),
            AppError::GenericNotFound => write!(f, "Page not found"),
            AppError::IntegrityViolation => write!(f, "Data integrity violation"),
            AppError::TooManyCandidates { max } => write!(
                f,
                "Cannot add more than {max} candidates to a candidate list"
            ),
            AppError::AmbiguousHash => write!(f, "Ambiguous hash prefix"),
            AppError::TooManyDownloads { max, window } => write!(
                f,
                "Rate limit reached: at most {max} downloads per {} seconds",
                window.num_seconds()
            ),
            AppError::TooManyEvents { max, window } => write!(
                f,
                "Rate limit reached: at most {max} changes per {} seconds",
                window.num_seconds()
            ),
            AppError::EventLimitReached { max } => write!(
                f,
                "Maximum of {max} recorded changes for this account reached"
            ),
            AppError::InternalServerError => write!(f, "Internal server error"),
            AppError::JsonRejection(err) => write!(f, "JSON error: {err}"),
            AppError::MissingEnvVar(var) => write!(f, "Missing environment variable: {var}"),
            AppError::MultipartError(err) => write!(f, "Multipart error: {err}"),
            AppError::MultipartFormError(err) => write!(f, "Multipart form error: {err}"),
            AppError::NoStorageConfigured => write!(f, "No event storage configured"),
            AppError::PdfError(err) => write!(f, "PDF error: {err}"),
            AppError::MarkdownError(err) => write!(f, "Markdown template error: {err}"),
            AppError::NotFound(msg) => write!(f, "{msg}"),
            AppError::UserError(msg) => write!(f, "{msg}"),
            AppError::PathRejection(err) => write!(f, "Path error: {err}"),
            AppError::QueryRejection(err) => write!(f, "Query error: {err}"),
            AppError::ServerError(err) => write!(f, "Server error: {err}"),
            AppError::TemplateError(err) => write!(f, "Template error: {err}"),
            AppError::Unauthorised => write!(f, "Unauthorised"),
            AppError::UpstreamError(err) => write!(f, "Upstream error: {err}"),
            AppError::IncompleteData(err) => write!(f, "Missing data when generating PDF: {err}"),
            AppError::EventDecodeError(err) => write!(f, "Event decode error: {err}"),
            AppError::EmlError(err) => write!(f, "EML error: {err}"),
            AppError::AuthError(err) => write!(f, "Authentication error: {err}"),
            #[cfg(feature = "acme")]
            AppError::AcmeError(err) => write!(f, "ACME error: {err}"),
            AppError::BrpError(err) => write!(f, "BRP error: {err}"),
        }
    }
}

impl std::error::Error for AppError {}

impl AppError {
    /// Whether this error reflects the database (or another backing service)
    /// being unreachable or structurally broken, as opposed to a logic error
    /// for one request.
    pub fn is_infrastructure_failure(&self) -> bool {
        #[cfg(feature = "database")]
        if let AppError::DatabaseError(err) = self {
            return sqlx_error_is_infrastructure(err);
        }
        false
    }
}

/// Classify a `sqlx::Error` as an infrastructure failure (connection lost,
/// pool exhausted, missing table, server shutting down) rather than a
/// per-query logic error (`RowNotFound`, decode mismatch, etc.).
#[cfg(feature = "database")]
pub(crate) fn sqlx_error_is_infrastructure(err: &sqlx::Error) -> bool {
    use sqlx::Error;

    match err {
        // Transport/pool level: the database could not be reached at all.
        Error::Io(_)
        | Error::Tls(_)
        | Error::Protocol(_)
        | Error::PoolTimedOut
        | Error::PoolClosed
        | Error::WorkerCrashed => true,
        // Server-reported errors: classify by SQLSTATE. Class 08 = connection
        // exceptions, 53 = insufficient resources, 57 = operator intervention
        // (e.g. admin shutdown / crash), 42P01 = undefined table (broken or
        // missing schema). Everything else is treated as a logic error.
        Error::Database(db) => db.code().is_some_and(|code| {
            let code = code.as_ref();
            code.starts_with("08")
                || code.starts_with("53")
                || code.starts_with("57")
                || code == "42P01"
        }),
        _ => false,
    }
}

#[cfg(feature = "database")]
impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        AppError::DatabaseError(err)
    }
}

impl From<textris_pdf::render::RenderError> for AppError {
    fn from(err: textris_pdf::render::RenderError) -> Self {
        AppError::PdfError(err)
    }
}

impl From<textris_pdf::markdown::MarkdownParseError> for AppError {
    fn from(err: textris_pdf::markdown::MarkdownParseError) -> Self {
        AppError::MarkdownError(err)
    }
}

impl From<std::fmt::Error> for AppError {
    fn from(_: std::fmt::Error) -> Self {
        AppError::InternalServerError
    }
}

impl From<askama::Error> for AppError {
    fn from(err: askama::Error) -> Self {
        AppError::TemplateError(err)
    }
}

impl From<MultipartError> for AppError {
    fn from(err: MultipartError) -> Self {
        AppError::MultipartFormError(err)
    }
}

impl From<MultipartRejection> for AppError {
    fn from(err: MultipartRejection) -> Self {
        AppError::MultipartError(err)
    }
}

impl From<FormRejection> for AppError {
    fn from(err: FormRejection) -> Self {
        AppError::FormRejection(err)
    }
}

impl From<JsonRejection> for AppError {
    fn from(err: JsonRejection) -> Self {
        AppError::JsonRejection(err)
    }
}

impl From<PathRejection> for AppError {
    fn from(err: PathRejection) -> Self {
        AppError::PathRejection(err)
    }
}

impl From<QueryRejection> for AppError {
    fn from(err: QueryRejection) -> Self {
        AppError::QueryRejection(err)
    }
}

impl From<Infallible> for AppError {
    fn from(_: Infallible) -> Self {
        AppError::InternalServerError
    }
}

impl From<reqwest::Error> for AppError {
    fn from(err: reqwest::Error) -> Self {
        Self::UpstreamError(err)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(_: serde_json::Error) -> Self {
        AppError::InternalServerError
    }
}

impl From<csv::Error> for AppError {
    fn from(_: csv::Error) -> Self {
        AppError::InternalServerError
    }
}

impl From<eml_nl::EMLError> for AppError {
    fn from(err: eml_nl::EMLError) -> Self {
        AppError::EmlError(err)
    }
}

impl From<auth_service::error::AuthError> for AppError {
    fn from(err: auth_service::error::AuthError) -> Self {
        AppError::AuthError(err)
    }
}

#[cfg(feature = "acme")]
impl From<instant_acme::Error> for AppError {
    fn from(err: instant_acme::Error) -> Self {
        AppError::AcmeError(err)
    }
}
#[cfg(test)]
mod tests {
    use crate::AppError;

    #[test]
    fn displays_not_found_message() {
        let err = AppError::NotFound("missing".to_string());
        assert_eq!(err.to_string(), "missing");
    }

    #[test]
    fn displays_missing_env_var() {
        let err = AppError::MissingEnvVar("STORAGE_URL");
        assert_eq!(err.to_string(), "Missing environment variable: STORAGE_URL");
    }

    #[test]
    fn displays_database_error() {
        #[cfg(feature = "database")]
        {
            let err = AppError::DatabaseError(sqlx::Error::RowNotFound);
            assert!(err.to_string().contains("Database error"));
        }
    }

    #[cfg(feature = "database")]
    #[test]
    fn classifies_connection_errors_as_infrastructure() {
        // Transport-level failures are infrastructure failures.
        let io = AppError::DatabaseError(sqlx::Error::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "refused",
        )));
        assert!(io.is_infrastructure_failure());

        let timed_out = AppError::DatabaseError(sqlx::Error::PoolTimedOut);
        assert!(timed_out.is_infrastructure_failure());

        let closed = AppError::DatabaseError(sqlx::Error::PoolClosed);
        assert!(closed.is_infrastructure_failure());
    }

    #[cfg(feature = "database")]
    #[test]
    fn does_not_classify_logic_errors_as_infrastructure() {
        // A missing row is a per-query outcome, not an outage.
        assert!(!AppError::DatabaseError(sqlx::Error::RowNotFound).is_infrastructure_failure());
        // Non-database errors are never infrastructure failures.
        assert!(!AppError::Unauthorised.is_infrastructure_failure());
        assert!(!AppError::GenericNotFound.is_infrastructure_failure());
    }

    #[cfg(feature = "acme")]
    #[test]
    fn converts_and_displays_acme_errors() {
        let err = AppError::from(instant_acme::Error::Str("no http-01 challenge"));
        assert!(err.to_string().starts_with("ACME error:"));
        assert!(err.to_string().contains("no http-01 challenge"));
        assert!(!err.is_infrastructure_failure());
    }
}
