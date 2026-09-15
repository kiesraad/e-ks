use askama::Template;
use axum::{
    extract::State,
    response::{IntoResponse, Redirect, Response},
};

use crate::{
    AnyLocale, AppError, AppRequestState, Context, ElectionConfig, HtmlTemplate, Province, Session,
    SessionUser, WaterCouncil,
    common::{PgIndexPath, SwitchElectionForm},
    csb::index::CsbIndexPath,
    filters,
};

use super::SwitchElectionPath;

#[derive(Template)]
#[template(path = "pg/common/pages/switch_election.html")]
struct SwitchElectionTemplate {
    elections: Vec<ElectionConfig>,
    existing_elections: Vec<ElectionConfig>,
    current_election: ElectionConfig,
    title_locale: AnyLocale,
    current_type: &'static str,
    selected_domain: Option<&'static str>,
    provinces: &'static [Province],
    water_councils: &'static [WaterCouncil],
}

pub async fn switch_election<S: AppRequestState>(
    _: SwitchElectionPath,
    State(state): State<S>,
    context: Context,
) -> Result<Response, AppError> {
    let existing_elections = existing_elections_for(&state, &context.session).await?;

    Ok(HtmlTemplate(
        SwitchElectionTemplate {
            current_election: context.election,
            title_locale: AnyLocale::from(context.session.locale),
            current_type: context.election.code(),
            selected_domain: context.election.domain_code(),
            provinces: Province::ALL,
            water_councils: WaterCouncil::ALL,
            elections: ElectionConfig::type_options(),
            existing_elections,
        },
        context,
    )
    .into_response())
}

async fn existing_elections_for<S: AppRequestState>(
    state: &S,
    session: &Session,
) -> Result<Vec<ElectionConfig>, AppError> {
    match &session.user {
        SessionUser::PoliticalGroup { stream_id, .. } => {
            state.existing_elections_for_stream(*stream_id).await
        }
        // A committee session (in paper-corrections mode) has no PG stream.
        SessionUser::CentralElectoralCommittee { .. } => Ok(Vec::new()),
    }
}

pub async fn switch_election_submit<S: AppRequestState>(
    _: SwitchElectionPath,
    State(state): State<S>,
    mut session: Session,
    axum::Form(form): axum::Form<SwitchElectionForm>,
) -> Result<Response, AppError> {
    let Some(election) = form.into_election_config() else {
        return Ok(Redirect::to(&SwitchElectionPath.to_string()).into_response());
    };

    // Committee sessions use CSB stores, not app stores; never create an
    // `PgStore` in their `(stream_id, election)` partition. Mirrors the guard
    // in `select_election_submit`, which this route can otherwise bypass while
    // a committee session is in paper-corrections mode.
    let SessionUser::PoliticalGroup {
        stream_id,
        election: current,
        ..
    } = &mut session.user
    else {
        return Ok(Redirect::to(&CsbIndexPath {}.to_string()).into_response());
    };

    // Short-circuit if already on this election.
    if *current == Some(election) {
        return Ok(Redirect::to(&PgIndexPath.to_string()).into_response());
    }

    // Ensure the store exists for the new election.
    state.store_for_stream(*stream_id, election, false).await?;

    *current = Some(election);
    // Invalidate forms rendered for the previous election, so a stale tab
    // cannot submit its data against the newly selected one.
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

    use crate::{AppState, ElectionConfig, Province, session_middleware, store_middleware};

    use super::*;

    #[tokio::test]
    async fn switch_election_submit_changes_session_election() {
        let state = AppState::new_for_tests().await;
        let app = Router::new()
            .typed_get(switch_election::<crate::AppState>)
            .typed_post(switch_election_submit::<crate::AppState>)
            .layer(middleware::from_fn_with_state(
                state.clone(),
                store_middleware,
            ))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                session_middleware,
            ))
            .with_state(state.clone());

        // Pre-create a session with a stream and a starting election.
        let stream_id = crate::StreamId::new();
        state
            .store_for_stream(stream_id, ElectionConfig::EK27, false)
            .await
            .expect("store");

        let mut session = crate::Session::new_test_for_stream(stream_id);
        session.set_test_election(ElectionConfig::EK27);
        let token_value = session.token_string();
        let csrf_token = session.csrf_token().clone();
        state.sessions().insert(session).await;

        let cookie = format!("{}={}", crate::SESSION_COOKIE_NAME, token_value);

        // Submit switch to PS27 Groningen
        let body = format!("csrf_token={csrf_token}&election=PS27&domain_province=prov1");
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/switch-election")
                    .header(header::COOKIE, &cookie)
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/");

        // Verify session current_election was updated (stream_id stays the same).
        let session = state
            .sessions
            .get(&token_value)
            .await
            .expect("load session")
            .expect("session");
        assert_eq!(session.test_stream_id(), stream_id);
        assert_eq!(
            session.user.election(),
            Some(ElectionConfig::PS27(Province::Groningen))
        );
    }
}
