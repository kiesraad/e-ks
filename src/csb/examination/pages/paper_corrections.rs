use axum::{
    extract::State,
    response::{IntoResponse, Redirect, Response},
};

use crate::{AppError, AppRequestState, CsbStore, Session};

use super::{
    CsbPaperCorrectionsStartPath, CsbPaperCorrectionsStopPath, CsbPoliticalGroupPath, PgIndexPath,
};

/// Put the committee session in paper-corrections mode for this stream. App
/// routes then serve the political group interface over the stream's
/// paper-corrected data (see `store_middleware`).
pub async fn start_paper_corrections<S: AppRequestState>(
    _: CsbPaperCorrectionsStartPath,
    State(state): State<S>,
    session: Session,
    store: CsbStore,
) -> Result<Response, AppError> {
    // Also rotates the CSRF token, so a stale tab cannot submit changes
    // against this stream's data.
    state
        .sessions()
        .set_paper_correction_stream_id(&session, Some(store.stream_id))
        .await?;

    Ok(Redirect::to(&PgIndexPath.to_string()).into_response())
}

/// Leave paper-corrections mode and return to the stream's examination page.
pub async fn stop_paper_corrections<S: AppRequestState>(
    path: CsbPaperCorrectionsStopPath,
    State(state): State<S>,
    session: Session,
) -> Result<Response, AppError> {
    state
        .sessions()
        .set_paper_correction_stream_id(&session, None)
        .await?;

    Ok(Redirect::to(
        &CsbPoliticalGroupPath {
            stream_id: path.stream_id,
        }
        .to_string(),
    )
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    use crate::{AppState, CsbAction, ElectionConfig, PgStoreData, StreamId};

    /// Persist a CSB stream carrying a single import event and return its id.
    async fn seed_csb_store(state: &AppState) -> StreamId {
        let stream_id = StreamId::new();
        let store = state
            .csb_store_for_stream(stream_id, ElectionConfig::EK27)
            .await
            .unwrap()
            .acting_as_test_user();
        store
            .update(CsbAction::Import {
                hash: [0u8; 32],
                source_stream_id: StreamId::new(),
                snapshot: Box::new(PgStoreData::default()),
            })
            .await
            .unwrap();
        stream_id
    }

    #[tokio::test]
    async fn start_sets_correction_stream_and_redirects_to_app_index() {
        let state = crate::AppState::new_for_tests().await;
        let stream_id = seed_csb_store(&state).await;
        let store = state
            .csb_store_for_stream(stream_id, ElectionConfig::EK27)
            .await
            .unwrap()
            .acting_as_test_user();
        let session = Session::new_test_committee();
        let token = session.token_string();
        let old_csrf = session.csrf_token().to_string();
        // The session middleware only hands a handler a session that is in the
        // store; the handler updates it and never re-creates it.
        state.sessions.insert(session.clone()).await;

        let response = start_paper_corrections(
            CsbPaperCorrectionsStartPath { stream_id },
            State(state.clone()),
            session,
            store,
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers().get("Location").unwrap(), "/");
        let stored = state
            .sessions
            .get_existing(Some(token.as_str()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.test_paper_correction_stream_id(), Some(stream_id));
        // Forms rendered before the switch no longer pass the CSRF guard.
        assert!(!stored.csrf_matches(&old_csrf));
    }

    #[tokio::test]
    async fn stop_clears_correction_stream_and_redirects_to_examination() {
        let state = crate::AppState::new_for_tests().await;
        let stream_id = StreamId::new();
        let mut session = Session::new_test_committee();
        session
            .set_paper_correction_stream_id(Some(stream_id))
            .expect("committee session");
        let token = session.token_string();
        let old_csrf = session.csrf_token().to_string();
        state.sessions.insert(session.clone()).await;

        let response = stop_paper_corrections(
            CsbPaperCorrectionsStopPath { stream_id },
            State(state.clone()),
            session,
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response.headers().get("Location").unwrap(),
            &format!("/csb/examination/{stream_id}")
        );
        let stored = state
            .sessions
            .get_existing(Some(token.as_str()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.test_paper_correction_stream_id(), None);
        assert!(!stored.csrf_matches(&old_csrf));
    }
}
