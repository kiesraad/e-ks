mod csrf;
mod file_form;
mod form_data;
mod id_newtype;
mod merge_errors;
mod string_validators;
mod switch_election;
mod validation_error;

pub use csrf::{TokenValue, csrf_token_matches, generate_csrf_token};
pub use file_form::FileForm;
pub use form_data::FormData;
pub use merge_errors::MergeErrors;
pub use string_validators::{
    is_teletex_char, validate_length, validate_multi_line_teletex_chars, validate_teletex_chars,
};
pub use switch_election::SwitchElectionForm;
pub use validation_error::ValidationError;

use axum::extract::{FromRequest, FromRequestParts};

use crate::AppError;

pub type FieldErrors = Vec<(String, ValidationError)>;

/// Wrapper around `axum_extra::extract::Form` that maps rejections to
/// [`AppError`]. Validation happens separately via the `Validate` derive.
#[derive(Debug, FromRequest)]
#[from_request(via(axum_extra::extract::Form), rejection(AppError))]
pub struct Form<T>(pub T);

/// `axum::extract::Query` with its rejection mapped to [`AppError`].
#[derive(Debug, Default, FromRequestParts)]
#[from_request(via(axum::extract::Query), rejection(AppError))]
pub struct Query<T>(pub T);

/// `axum::Json` (as extractor) with its rejection mapped to [`AppError`].
#[derive(Debug, FromRequest)]
#[from_request(via(axum::Json), rejection(AppError))]
pub struct Json<T>(pub T);

pub trait IntoValidationError {
    fn into_validation_error(self) -> ValidationError;
}

impl IntoValidationError for ValidationError {
    fn into_validation_error(self) -> ValidationError {
        self
    }
}

impl IntoValidationError for std::num::ParseIntError {
    fn into_validation_error(self) -> ValidationError {
        ValidationError::InvalidValue
    }
}

impl IntoValidationError for std::str::ParseBoolError {
    fn into_validation_error(self) -> ValidationError {
        ValidationError::InvalidValue
    }
}

impl IntoValidationError for uuid::Error {
    fn into_validation_error(self) -> ValidationError {
        ValidationError::InvalidValue
    }
}

impl IntoValidationError for chrono::ParseError {
    fn into_validation_error(self) -> ValidationError {
        ValidationError::InvalidValue
    }
}
