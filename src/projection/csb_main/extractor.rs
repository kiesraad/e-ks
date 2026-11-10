use axum::{extract::FromRequestParts, http::request::Parts};

use crate::{
    AppError, AppRequestState, CsbMainAction, CsbMainStream, CsbUser, Session,
    projection::CSB_MAIN_STREAM_ID,
};

impl<S: AppRequestState> FromRequestParts<S> for CsbMainStream {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let election = Session::from_request_parts(parts, state)
            .await?
            .require_current_election()?;

        let registry = state.csb_main_store_registry();
        registry.get_or_create(CSB_MAIN_STREAM_ID, election).await
    }
}

/// The main CSB stream paired with the committee member acting on it in this
/// request; [`CsbMainStore::update`] attributes every event to them, as
/// [`crate::CsbStore`] does for a political group's stream. Reads go through
/// [`std::ops::Deref`].
#[derive(Clone)]
pub struct CsbMainStore {
    stream: CsbMainStream,
    user: CsbUser,
}

impl std::ops::Deref for CsbMainStore {
    type Target = CsbMainStream;

    fn deref(&self) -> &Self::Target {
        &self.stream
    }
}

impl CsbMainStore {
    /// Persist an action, recorded as triggered by the acting committee member.
    pub async fn update(&self, action: CsbMainAction) -> Result<(), AppError> {
        self.stream.update(action.by(self.user.clone())).await
    }

    #[cfg(test)]
    pub fn new_for_test() -> Self {
        Self::new_for_test_with_election(crate::ElectionConfig::EK27)
    }

    #[cfg(test)]
    pub fn new_for_test_with_election(election: crate::ElectionConfig) -> Self {
        Self {
            stream: CsbMainStream::new_for_test_with_election(election),
            user: CsbUser::new_test(),
        }
    }
}

impl<S: AppRequestState> FromRequestParts<S> for CsbMainStore {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let user = Session::from_request_parts(parts, state)
            .await?
            .require_csb_user()?;
        let stream = CsbMainStream::from_request_parts(parts, state).await?;

        Ok(Self { stream, user })
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        routing::get,
    };
    use tower::ServiceExt;

    use super::*;
    use crate::{AppState, ElectionConfig, HasCsbUser, Locale};

    async fn log_out_as(state: AppState, session: Session) -> StatusCode {
        let app = Router::new()
            .route(
                "/",
                get(|store: CsbMainStore| async move {
                    store.update(CsbMainAction::Logout).await.unwrap()
                }),
            )
            .with_state(state);
        let mut request = Request::builder().uri("/").body(Body::empty()).unwrap();
        request.extensions_mut().insert(session);

        app.oneshot(request).await.unwrap().status()
    }

    #[tokio::test]
    async fn writes_as_the_session_user_and_rejects_political_groups() {
        let state = AppState::new_for_tests().await;
        let user = CsbUser::SecurityKey {
            username: "lid1".parse().unwrap(),
        };
        let session = Session::for_committee(user.clone(), ElectionConfig::EK27, Locale::En);

        assert_eq!(log_out_as(state.clone(), session).await, StatusCode::OK);
        let stream = state.csb_main_store(ElectionConfig::EK27).await.unwrap();
        let events = stream.snapshot().events.clone();
        assert_eq!(events.last().unwrap().payload.csb_user(), &user);

        let mut political_group = Session::new_test();
        political_group.set_test_election(ElectionConfig::EK27);
        assert_eq!(
            log_out_as(state, political_group).await,
            StatusCode::UNAUTHORIZED
        );
    }
}
