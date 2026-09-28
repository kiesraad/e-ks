use crate::{Locale, Session};
use axum::{extract::FromRequestParts, http::request::Parts};
use std::convert::Infallible;

impl<S> FromRequestParts<S> for Locale
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        if let Some(locale) = parts
            .extensions
            .get::<Session>()
            .map(|session| session.locale)
        {
            return Ok(locale);
        }

        Ok(Locale::default())
    }
}
