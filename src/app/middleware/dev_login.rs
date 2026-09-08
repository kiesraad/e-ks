use axum::{
    extract::{Query, State},
    http::{HeaderName, HeaderValue},
    response::{IntoResponse, Redirect},
};
use axum_extra::extract::CookieJar;
use secrecy::SecretString;
use serde::Deserialize;

use crate::{
    AppError, AppState, CsbMainAction, CsbUser, ElectionConfig, Locale, PgEvent, PgStoreData,
    Session, SessionUser, StreamId,
    auth::session_extractor::{establish_session, user_agent_hash},
    common::{PgIndexPath, SelectElectionPath},
    csb::index::CsbIndexPath,
    store::Store,
    structs::{common::Appellation, political_groups::PoliticalGroup},
    utils::format_hash,
};

pub const DEV_LOGIN_PATH: &str = "/dev/login";

/// Response header on the dev-login redirect carrying the chain hash of the
/// stream's last event, so end-to-end tests can drive the CSB import flow.
pub const LAST_EVENT_HASH_HEADER: HeaderName = HeaderName::from_static("x-last-event-hash");

/// Placeholder `NameID` for dev-login sessions, which skip the SAML flow.
const DEV_LOGIN_NAME_ID: &str = "dev-login-placeholder-name-id";

#[derive(Debug, Deserialize)]
pub struct DevLoginQuery {
    bsn: Option<String>,
    fixtures: Option<bool>,
    select_election: Option<bool>,
    csb: Option<bool>,
    name: Option<String>,
}

/// Dev login. By default the session belongs to a political group
/// ([`SessionUser::PoliticalGroup`]) that only sees its own stream.
///
/// When `csb=true` the login is instead a member of the central electoral
/// committee ([`SessionUser::CentralElectoralCommittee`]), giving access to
/// the shared committee main stream and to all imported streams. No
/// per-session committee stream is created.
pub async fn dev_login(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<DevLoginQuery>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    let mut login = DevLogin::new(&state, &query, &headers);
    let (redirect_to, last_event_hash) = login.run().await?;
    let session = login.session;

    let mut response = (
        establish_session(&state.sessions, jar, session).await,
        Redirect::to(&redirect_to),
    )
        .into_response();

    if let Some(hash) = last_event_hash {
        // The hash is uppercase hex with spaces, always a valid header value
        let value = HeaderValue::from_str(&hash).map_err(|_| AppError::InternalServerError)?;
        response.headers_mut().insert(LAST_EVENT_HASH_HEADER, value);
    }

    Ok(response)
}

/// A dev login in progress: the session being established, and the request that
/// asked for it.
struct DevLogin<'a> {
    state: &'a AppState,
    query: &'a DevLoginQuery,
    session: Session,
}

impl<'a> DevLogin<'a> {
    /// Builds the session: its identity from `csb` (a committee member, or a
    /// political group with its stream derived from `bsn`), and its locale and
    /// user agent from the request headers.
    fn new(state: &'a AppState, query: &'a DevLoginQuery, headers: &axum::http::HeaderMap) -> Self {
        let locale = Locale::from_headers(headers);
        let mut session = match query.csb {
            // Committee members share the CSB main stream, and dev logins are
            // not told apart in the audit log, so no BSN-derived stream is
            // involved.
            Some(true) => {
                Session::for_committee(CsbUser::Developer, state.config.default_election, locale)
            }
            _ => Session::for_political_group(
                derive_dev_stream_id(state, query.bsn.as_deref()),
                DEV_LOGIN_NAME_ID.to_string(),
                None,
                locale,
            ),
        };
        session.set_user_agent_hash(user_agent_hash(headers));

        Self {
            state,
            query,
            session,
        }
    }

    /// Sets up the stores the session's identity gives access to, and returns
    /// the redirect path plus the chain hash of the stream's last event.
    async fn run(&mut self) -> Result<(String, Option<String>), AppError> {
        // Checked for every identity, so a malformed name is rejected up front.
        let fixture_name = self.fixture_name()?;

        match self.session.user.clone() {
            SessionUser::CentralElectoralCommittee { user, election, .. } => {
                Ok((self.csb(user, election).await?, None))
            }
            SessionUser::PoliticalGroup { stream_id, .. } => self.pg(stream_id, fixture_name).await,
        }
    }

    /// The appellation the `name` query parameter asks the fixture group to be
    /// given, if any.
    fn fixture_name(&self) -> Result<Option<Appellation>, AppError> {
        self.query
            .name
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(str::parse)
            .transpose()
            .map_err(|_| AppError::UserError("invalid political group name".to_string()))
    }

    /// Logs the login on the shared committee main stream, optionally imports
    /// the CSB fixture, and returns the redirect path.
    async fn csb(&mut self, user: CsbUser, election: ElectionConfig) -> Result<String, AppError> {
        // All committee members share a single stream.
        let store = self.state.csb_main_store(election).await?;
        store.update(CsbMainAction::Login.by(user.clone())).await?;

        #[cfg(feature = "fixtures")]
        if self.query.fixtures.unwrap_or(false) {
            crate::csb::import::fixture::import_csb_fixture(self.state, election, user).await?;
        }
        #[cfg(not(feature = "fixtures"))]
        let _ = user;

        Ok(CsbIndexPath {}.to_string())
    }

    /// Sets up the political group's own stream and returns the redirect path
    /// plus the chain hash of the stream's last event (for end-to-end tests).
    async fn pg(
        &mut self,
        stream_id: StreamId,
        fixture_name: Option<Appellation>,
    ) -> Result<(String, Option<String>), AppError> {
        if self.query.select_election.unwrap_or(false) {
            return Ok((SelectElectionPath.to_string(), None));
        }

        let election = self.state.config.default_election;
        let (store, was_new) = self.ensure_store(stream_id, fixture_name, election).await?;

        if was_new {
            store.update(PgEvent::DeveloperLogin { stream_id }).await?;
        }
        let last_event_hash = store
            .data
            .read()
            .events
            .last()
            .map(|e| format_hash(&e.hash, false));

        if let SessionUser::PoliticalGroup {
            election: current, ..
        } = &mut self.session.user
        {
            *current = Some(election);
        }
        Ok((PgIndexPath.to_string(), last_event_hash))
    }

    /// Opens the session's store, creating the political group (and loading the
    /// fixtures, if asked for) when the stream is still empty.
    async fn ensure_store(
        &self,
        stream_id: StreamId,
        fixture_name: Option<Appellation>,
        election: ElectionConfig,
    ) -> Result<(Store<PgStoreData>, bool), AppError> {
        let store = self
            .state
            .store_registry
            .get_or_create(stream_id, election)
            .await?;
        let store_is_empty = store.data.read().events.is_empty();

        if store_is_empty {
            PoliticalGroup::default()
                .create(&crate::PgStore::own(store.clone()))
                .await?;
        }

        if self.query.fixtures.unwrap_or(false) {
            #[cfg(feature = "fixtures")]
            {
                crate::fixtures::load(&crate::PgStore::own(store.clone()), fixture_name).await?;
                return Ok((store, store_is_empty));
            }
        }
        #[cfg(not(feature = "fixtures"))]
        let _ = fixture_name;

        Ok((store, store_is_empty))
    }
}

/// Derives the dev stream id from the requested BSN, or mints a fresh stream
/// when no BSN was given.
fn derive_dev_stream_id(state: &AppState, bsn: Option<&str>) -> StreamId {
    let Some(id_code) = bsn.filter(|s| !s.is_empty()).map(SecretString::from) else {
        return StreamId::new();
    };

    state.id_deriver.derive_stream_id(&id_code)
}

#[cfg(all(feature = "dev-features", test))]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode, header},
        response::Response,
    };
    use tower::ServiceExt;

    use secrecy::SecretString;

    use crate::{
        AppState, CsbAction, CsbMainAction, CsbMainEvent, CsbUser, ElectionConfig, Locale, PgEvent,
        PgStore, Scope, Session, StreamId, router, store::StoreEvent,
        test_utils::response_body_string,
    };

    const TEST_ID_CODE: &str = "999999990";

    fn derive_test_id(state: &AppState, id_code_str: &str) -> StreamId {
        let id_code: SecretString = id_code_str.into();
        state.id_deriver.derive_stream_id(&id_code)
    }

    /// Build a fresh test state and a router wired to it.
    async fn test_app() -> (AppState, axum::Router) {
        let state = AppState::new_for_tests().await;
        let app = router::create(state.clone()).with_state(state.clone());
        (state, app)
    }

    /// A `GET /dev/login` request for the test user with the given extra query.
    fn dev_login_request(query: &str) -> Request<Body> {
        Request::builder()
            .uri(format!("/dev/login?bsn={TEST_ID_CODE}&{query}"))
            .body(Body::empty())
            .expect("valid request")
    }

    /// The session cookie's `name=value` pair.
    fn cookie_value(response: &Response) -> &str {
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter_map(|value| value.split(';').next())
            .find(|pair| pair.starts_with(crate::SESSION_COOKIE_NAME))
            .expect("session cookie value")
    }

    /// Resolve the session that a dev-login response established.
    async fn session_from(state: &AppState, response: &Response) -> Session {
        let token = cookie_value(response)
            .split_once('=')
            .map(|(_, value)| value)
            .expect("session token");
        state
            .sessions
            .get(token)
            .await
            .expect("load session")
            .expect("session")
    }

    /// Open the per-stream store for the dev-login test user.
    async fn open_store(state: &AppState) -> PgStore {
        let expected_id = derive_test_id(state, TEST_ID_CODE);
        PgStore::own(
            state
                .store_registry
                .get_or_create(expected_id, ElectionConfig::EK27)
                .await
                .expect("store"),
        )
    }

    /// Log in with the given dev-login request, then load `path` with the
    /// session cookie and return the response.
    async fn login_then_get(
        app: axum::Router,
        login_request: Request<Body>,
        path: &str,
    ) -> Response {
        let login = app.clone().oneshot(login_request).await.expect("response");

        app.oneshot(
            Request::builder()
                .uri(path)
                .header(header::COOKIE, cookie_value(&login))
                .body(Body::empty())
                .expect("valid request"),
        )
        .await
        .expect("response")
    }

    /// Dev-login without fixtures and then load the home page, asserting it
    /// rendered successfully.
    async fn login_without_fixtures_then_home(app: axum::Router) {
        let response = login_then_get(app, dev_login_request("fixtures=false"), "/").await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Kiesraad - Kandidaatstelling"));
    }

    #[tokio::test]
    async fn dev_login_sets_cookie_and_redirects_home() {
        let (state, app) = test_app().await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/dev/login?bsn={TEST_ID_CODE}&fixtures=false"))
                    .header(header::ACCEPT_LANGUAGE, "en")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/");

        let session = session_from(&state, &response).await;
        assert_eq!(session.locale, Locale::En);
        assert_eq!(
            session.test_stream_id(),
            derive_test_id(&state, TEST_ID_CODE)
        );
    }

    #[tokio::test]
    async fn dev_login_without_fixtures_keeps_store_empty() {
        let (state, app) = test_app().await;
        login_without_fixtures_then_home(app).await;

        let store = open_store(&state).await;
        assert_eq!(store.get_person_count(), 0);
        assert_eq!(store.get_candidate_list_count(), 0);
    }

    #[tokio::test]
    async fn dev_login_without_fixtures_adds_dev_login_event() {
        let (state, app) = test_app().await;
        login_without_fixtures_then_home(app).await;

        let store = open_store(&state).await;
        assert!(matches!(
            store.get_events().as_slice(),
            &[
                StoreEvent {
                    payload: PgEvent::UpdatePoliticalGroup(..),
                    ..
                },
                StoreEvent {
                    payload: PgEvent::DeveloperLogin { .. },
                    ..
                }
            ],
        ))
    }

    #[tokio::test]
    async fn dev_login_csb_without_fixtures_adds_dev_login_event() {
        let (state, app) = test_app().await;

        app.oneshot(dev_login_csb_request("fixtures=false"))
            .await
            .expect("response");

        let store = state
            .csb_main_store(ElectionConfig::EK27)
            .await
            .expect("main store");

        assert!(matches!(
            store.data.read().events.as_slice(),
            &[StoreEvent {
                payload: CsbMainEvent {
                    user: CsbUser::Developer,
                    action: CsbMainAction::Login,
                },
                ..
            }]
        ));
    }

    #[tokio::test]
    async fn dev_login_select_election_skips_election_setup() {
        let (state, app) = test_app().await;

        let response = app
            .oneshot(dev_login_request("select_election=true"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response.headers().get(header::LOCATION).unwrap(),
            "/select-election"
        );

        let session = session_from(&state, &response).await;
        assert_eq!(
            session.test_stream_id(),
            derive_test_id(&state, TEST_ID_CODE)
        );
        assert_eq!(session.user.election(), None);
    }

    #[cfg(feature = "fixtures")]
    #[tokio::test]
    async fn dev_login_with_fixtures_loads_fixture_data() {
        let (state, app) = test_app().await;

        let response = app
            .oneshot(dev_login_request("fixtures=true"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let store = open_store(&state).await;
        assert!(store.get_person_count() > 0);
        assert!(store.get_candidate_list_count() > 0);
    }

    /// The `name` query sets the fixture group's appellation and the redirect
    /// carries the chain hash of the stream's last event.
    #[cfg(feature = "fixtures")]
    #[tokio::test]
    async fn dev_login_with_fixtures_uses_name_and_returns_last_event_hash() {
        let (state, app) = test_app().await;

        let response = app
            .oneshot(dev_login_request("fixtures=true&name=Unieke%20Groep"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let store = open_store(&state).await;
        assert_eq!(
            store
                .get_political_group()
                .appellation
                .expect("appellation")
                .to_string(),
            "Unieke Groep"
        );

        let last_hash = store.get_events().last().expect("events").hash;
        let header = response
            .headers()
            .get(crate::app::middleware::dev_login::LAST_EVENT_HASH_HEADER)
            .expect("hash header")
            .to_str()
            .expect("ascii header");
        assert_eq!(header, crate::utils::format_hash(&last_hash, false));
    }

    /// Dev logins that pass no BSN each get their own political group, so
    /// end-to-end tests running in parallel against one server never share a
    /// stream.
    #[tokio::test]
    async fn dev_login_without_bsn_gets_a_stream_of_its_own() {
        let (state, app) = test_app().await;
        let mut stream_ids = std::collections::HashSet::new();

        for _ in 0..8 {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/dev/login?fixtures=false")
                        .body(Body::empty())
                        .expect("valid request"),
                )
                .await
                .expect("response");

            let stream_id = session_from(&state, &response).await.test_stream_id();
            assert!(stream_ids.insert(stream_id), "reused stream {stream_id}");
        }
    }

    #[tokio::test]
    async fn dev_login_scopes_session_to_political_group() {
        let (state, app) = test_app().await;

        let response = app
            .oneshot(dev_login_request("fixtures=false"))
            .await
            .expect("response");

        let session = session_from(&state, &response).await;
        assert_eq!(session.scope(), Scope::PoliticalGroup);
    }

    /// A `GET /dev/login?csb=true` request for the given query.
    fn dev_login_csb_request(query: &str) -> Request<Body> {
        Request::builder()
            .uri(format!("/dev/login?csb=true&bsn={TEST_ID_CODE}&{query}"))
            .body(Body::empty())
            .expect("valid request")
    }

    #[cfg(feature = "fixtures")]
    #[tokio::test]
    async fn dev_login_csb_with_fixtures_creates_imported_stream() {
        let (state, app) = test_app().await;

        let response = app
            .oneshot(dev_login_csb_request("fixtures=true"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let csb_stores = state
            .csb_store_registry
            .stores_by_scope()
            .await
            .expect("csb stores");

        assert_eq!(csb_stores.len(), 1);
        let csb_store = &csb_stores[0];

        let events = csb_store.data.read().events.clone();
        // The import comes first, followed by the fixture omissions.
        let (event, omissions) = events.split_first().expect("the import event");
        assert!(!omissions.is_empty());
        assert!(
            omissions
                .iter()
                .all(|event| matches!(event.payload.action, CsbAction::CreateOmission(_)))
        );

        let event = event.clone();
        let StoreEvent {
            payload:
                crate::CsbEvent {
                    action: CsbAction::Import { hash, snapshot, .. },
                    ..
                },
            ..
        } = event
        else {
            panic!("unexpected event: {event:?}");
        };

        assert_eq!(hash, crate::csb::import::fixture::FIXTURE_IMPORT_HASH);
        assert!(!snapshot.persons.is_empty());
        assert!(!snapshot.candidate_lists.is_empty());
    }

    #[tokio::test]
    async fn dev_login_csb_scopes_session_to_committee() {
        let (state, app) = test_app().await;

        let response = app
            .oneshot(dev_login_csb_request("fixtures=false"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        // Committee members land on their CSB page, not the app home.
        assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/csb");

        let session = session_from(&state, &response).await;
        assert_eq!(session.scope(), Scope::CentralElectoralCommittee);
        assert_eq!(session.user.election(), Some(ElectionConfig::EK27));
    }

    /// A committee session can reach the CSB import page.
    #[tokio::test]
    async fn csb_import_reachable_for_committee_session() {
        let (_state, app) = test_app().await;

        let response =
            login_then_get(app, dev_login_csb_request("fixtures=false"), "/csb/import").await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Import"));
    }

    /// A political-group session is rejected from CSB routes.
    #[tokio::test]
    async fn csb_import_rejected_for_political_group_session() {
        let (_state, app) = test_app().await;

        let response =
            login_then_get(app, dev_login_request("fixtures=false"), "/csb/import").await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    /// A committee session is kept off app routes (which use app stores) and sent
    /// to its CSB import page instead.
    #[tokio::test]
    async fn committee_session_redirected_off_app_routes() {
        let (_state, app) = test_app().await;

        let response = login_then_get(app, dev_login_csb_request("fixtures=false"), "/").await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/csb");
    }
}
