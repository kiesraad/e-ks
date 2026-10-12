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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};

    #[tokio::test]
    async fn request_locale_uses_session() {
        let mut request = Request::builder().uri("/").body(Body::empty()).unwrap();
        request
            .extensions_mut()
            .insert(Session::new_test_with_locale(Locale::En));
        let (mut parts, _body) = request.into_parts();

        let locale = Locale::from_request_parts(&mut parts, &()).await.unwrap();

        assert_eq!(locale, Locale::En);
    }
}
