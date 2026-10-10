//! Shared parsing for `STORAGE_URL` values.

use url::Url;

use crate::AppError;

/// Supported `STORAGE_URL` schemes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageScheme {
    Memory,
    Postgres,
}

impl StorageScheme {
    /// Parse the scheme of a storage URL, with uniform error messages.
    pub fn parse(storage_url: &str) -> Result<Self, AppError> {
        let url = Url::parse(storage_url)
            .map_err(|err| AppError::ConfigLoadError(format!("Invalid storage URL: {err}")))?;

        match url.scheme() {
            "memory" => Ok(Self::Memory),
            "postgres" | "postgresql" => Ok(Self::Postgres),
            scheme => Err(AppError::ConfigLoadError(format!(
                "Unsupported storage scheme: {scheme}, supported schemes are: memory://, postgres://"
            ))),
        }
    }
}

/// Error for `postgres://` URLs in builds without the `database` feature.
#[cfg(not(feature = "database"))]
pub fn database_disabled_error() -> AppError {
    AppError::ConfigLoadError("Database storage disabled (enable feature \"database\")".to_string())
}
