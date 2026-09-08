use askama::Template;
use axum::{
    extract::State,
    http::{HeaderValue, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;

use crate::{
    AnyLocale, AppError, AppRequestState, Context, Province, Session, SessionPageValues,
    SessionUser, WaterCouncil,
    common::{PgIndexPath, SelectElectionForm},
    csb::index::CsbIndexPath,
    filters,
};

use super::SelectElectionPath;

#[derive(Template)]
#[template(path = "pg/common/pages/select_election.html")]
struct SelectElectionTemplate {
    elections: Vec<crate::ElectionConfig>,
    title_locale: AnyLocale,
    provinces: &'static [Province],
    water_councils: &'static [WaterCouncil],
}

pub async fn select_election<S: AppRequestState>(
    _: SelectElectionPath,
    session: Session,
    State(state): State<S>,
) -> Result<Response, AppError> {
    if session.user.election().is_some() {
        return Ok(Redirect::to(&PgIndexPath.to_string()).into_response());
    }

    state.sessions().update(&session).await;

    let template = SelectElectionTemplate {
        elections: crate::ElectionConfig::type_options(),
        title_locale: AnyLocale::from(session.locale),
        provinces: Province::ALL,
        water_councils: WaterCouncil::ALL,
    };

    let values = SessionPageValues {
        locale: session.locale,
        csrf_token: session.csrf_token().0.clone(),
    };
    let html = template
        .render_with_values(&values)
        .map_err(AppError::TemplateError)?;

    let mut response = Html(html).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

pub async fn select_election_submit<S: AppRequestState>(
    _: SelectElectionPath,
    State(state): State<S>,
    jar: CookieJar,
    mut session: Session,
    axum::Form(form): axum::Form<SelectElectionForm>,
) -> Result<Response, AppError> {
    #[cfg(not(feature = "fixtures"))]
    let _ = jar;

    // Committee sessions use CSB stores, not app stores; never create an
    // `PgStore` in their `(stream_id, election)` partition.
    if matches!(session.user, SessionUser::CentralElectoralCommittee { .. }) {
        return Ok(Redirect::to(&CsbIndexPath {}.to_string()).into_response());
    }

    let Some(election) = form.election_config() else {
        return Ok(Redirect::to(&SelectElectionPath.to_string()).into_response());
    };

    // Only available with the `fixtures` feature: this is a test/dev shortcut
    // into the committee (CSB) role. An explicit escalation: a brand-new
    // committee session replaces the political-group one (the old token dies
    // in `establish_session`), it is never mutated into one.
    #[cfg(feature = "fixtures")]
    if form.login_as_csb() {
        let user = crate::CsbUser::Developer;

        if form.load_fixtures() {
            crate::csb::import::fixture::import_csb_fixture(&state, election, user.clone()).await?;
        }

        let mut committee = Session::for_committee(user, election, session.locale);
        if let Some(user_agent_hash) = session.user_agent_hash.clone() {
            committee.set_user_agent_hash(user_agent_hash);
        }
        let jar =
            crate::auth::session_extractor::establish_session(state.sessions(), jar, committee)
                .await;

        return Ok((jar, Redirect::to(&CsbIndexPath {}.to_string())).into_response());
    }

    // use the stream ID derived from the authenticated login
    let SessionUser::PoliticalGroup {
        stream_id,
        election: current,
        ..
    } = &mut session.user
    else {
        return Ok(Redirect::to(&CsbIndexPath {}.to_string()).into_response());
    };

    let _store = state
        .store_for_stream(*stream_id, election, form.load_fixtures())
        .await?;

    *current = Some(election);
    // Invalidate forms rendered before an election was picked, so a stale tab
    // cannot submit against the election chosen here.
    session.rotate_csrf_token();
    state.sessions().update(&session).await;

    Ok(Redirect::to(&PgIndexPath.to_string()).into_response())
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

    use crate::{AppState, ElectionConfig, Session, session_middleware};

    use super::*;

    #[tokio::test]
    async fn select_election_redirects_when_election_already_set() {
        let state = AppState::new_for_tests().await;
        let app = Router::new()
            .typed_get(select_election::<crate::AppState>)
            .layer(middleware::from_fn_with_state(
                state.clone(),
                session_middleware,
            ))
            .with_state(state.clone());

        let mut session = Session::new_test();
        session.set_test_election(ElectionConfig::EK27);
        let token = session.token_string();
        state.sessions().insert(session).await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/select-election")
                    .header(
                        header::COOKIE,
                        format!("{}={}", crate::SESSION_COOKIE_NAME, token),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/");
    }

    /// TVS "Checklist Testen" v2.1 T7: a logout option must be on screen from
    /// the moment of login. This page is the first screen a newly authenticated
    /// user without an election lands on, and it does not use the main layout.
    #[tokio::test]
    async fn select_election_offers_logout() {
        let state = AppState::new_for_tests().await;
        let app = Router::new()
            .typed_get(select_election::<crate::AppState>)
            .layer(middleware::from_fn_with_state(
                state.clone(),
                session_middleware,
            ))
            .with_state(state.clone());

        let session = Session::new_test();
        let token = session.token_string();
        state.sessions().insert(session).await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/select-election")
                    .header(
                        header::COOKIE,
                        format!("{}={}", crate::SESSION_COOKIE_NAME, token),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = crate::test_utils::response_body_string(response).await;
        assert!(body.contains(r#"class="logout-form""#), "{body}");
        assert!(
            body.contains(&format!(r#"action="{}""#, crate::common::LogoutPath)),
            "{body}"
        );
    }

    #[tokio::test]
    async fn select_election_submit_sets_current_election() {
        let state = AppState::new_for_tests().await;
        let app = Router::new()
            .typed_get(select_election::<crate::AppState>)
            .typed_post(select_election_submit::<crate::AppState>)
            .layer(middleware::from_fn_with_state(
                state.clone(),
                session_middleware,
            ))
            .with_state(state.clone());

        let session = Session::new_test();
        let token = session.token_string();
        state.sessions().insert(session).await;

        // Fetch page to receive a CSRF token
        let get = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/select-election")
                    .header(
                        header::COOKIE,
                        format!("{}={}", crate::SESSION_COOKIE_NAME, token),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");
        assert_eq!(get.status(), StatusCode::OK);

        let session = state
            .sessions
            .get(&token)
            .await
            .expect("load session")
            .expect("session");
        let csrf = session.csrf_token();

        let body = format!("csrf_token={csrf}&election=EK27");
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/select-election")
                    .header(
                        header::COOKIE,
                        format!("{}={}", crate::SESSION_COOKIE_NAME, token),
                    )
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/");

        let session = state
            .sessions
            .get(&token)
            .await
            .expect("load session")
            .expect("session");
        assert_eq!(session.user.election(), Some(ElectionConfig::EK27));
    }

    /// The fixtures-only CSB shortcut replaces the political-group session
    /// with a brand-new committee session: the submitted token is dead
    /// afterwards, and the freshly minted one reaches CSB routes.
    #[cfg(feature = "fixtures")]
    #[tokio::test]
    async fn login_as_csb_replaces_the_session_with_a_committee_one() {
        let state = AppState::new_for_tests().await;
        let app = Router::new()
            .typed_post(select_election_submit::<crate::AppState>)
            .layer(middleware::from_fn_with_state(
                state.clone(),
                session_middleware,
            ))
            .with_state(state.clone());

        let session = Session::new_test();
        let old_token = session.token_string();
        let csrf = session.csrf_token().to_string();
        state.sessions().insert(session).await;

        let body = format!("csrf_token={csrf}&election=EK27&login_as_csb=true");
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/select-election")
                    .header(
                        header::COOKIE,
                        format!("{}={}", crate::SESSION_COOKIE_NAME, old_token),
                    )
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/csb");

        // The escalation minted a fresh session and killed the old token.
        let new_token = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter_map(|value| value.split(';').next())
            .filter_map(|pair| pair.split_once('='))
            .find(|(name, _)| *name == crate::SESSION_COOKIE_NAME)
            .map(|(_, token)| token.to_string())
            .expect("new session cookie");
        assert_ne!(new_token, old_token);
        assert!(
            state
                .sessions
                .get(&old_token)
                .await
                .expect("load")
                .is_none(),
            "the political-group session must not survive the escalation"
        );

        // The new session is a complete committee identity that survives
        // persistence and passes the CSB gate.
        let committee = state
            .sessions
            .get(&new_token)
            .await
            .expect("load session")
            .expect("session");
        assert_eq!(committee.scope(), crate::Scope::CentralElectoralCommittee);
        assert!(committee.require_csb_user().is_ok());
        assert_eq!(committee.user.election(), Some(ElectionConfig::EK27));
    }
}
