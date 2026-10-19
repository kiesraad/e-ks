use crate::{
    AppError, AppRequestState, Locale, Session,
    common::{PgIndexPath, SwitchLanguagePath},
    redirect_to_referer,
};
use axum::{extract::State, response::Redirect};
use axum_extra::{TypedHeader, extract::Form, headers};
use serde::Deserialize;

#[derive(Default, Deserialize, Clone, Debug)]
pub struct LanguageSwitch {
    lang: Locale,
}

pub async fn switch_language<S: AppRequestState>(
    _: SwitchLanguagePath,
    TypedHeader(referer): TypedHeader<headers::Referer>,
    State(state): State<S>,
    session: Session,
    Form(form): Form<LanguageSwitch>,
) -> Result<Redirect, AppError> {
    state.sessions().set_locale(&session, form.lang).await?;

    // Back to the page the switch was made on, never off-site (see
    // [`redirect_to_referer`]).
    Ok(redirect_to_referer(&referer, PgIndexPath))
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode, header},
        middleware,
    };
    use axum_extra::routing::RouterExt;
    use tower::ServiceExt;

    use crate::AppState;

    use super::*;

    #[tokio::test]
    async fn switch_language_updates_session_and_redirects() {
        let state = AppState::new_for_tests().await;
        let app = Router::new()
            .typed_post(switch_language::<crate::AppState>)
            .layer(middleware::from_fn_with_state(
                state.clone(),
                crate::session_middleware,
            ))
            .with_state(state.clone());

        let session = crate::Session::new_test();
        let token = session.token_string();
        let csrf = session.csrf_token().clone();
        state.sessions().insert(session).await;

        let request = Request::builder()
            .method("POST")
            .uri("/language")
            .header(header::REFERER, "https://example.com/return")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(
                header::COOKIE,
                format!("{}={}", crate::SESSION_COOKIE_NAME, token),
            )
            .body(Body::from(format!("csrf_token={csrf}&lang=en")))
            .unwrap();

        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        // Only the referrer's path survives, so the redirect stays on this
        // origin even when the header names another host.
        assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/return");

        let session = state
            .sessions
            .get(&token)
            .await
            .expect("load session")
            .expect("session");
        assert_eq!(session.locale, Locale::En);
    }
}
