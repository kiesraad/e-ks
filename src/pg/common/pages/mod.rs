use auth_service::{AuthServiceState, AuthState};
use axum::{Router, extract::FromRef, routing::get};
use axum_extra::routing::{RouterExt, TypedPath};

use super::paths::{
    LoggedOutPath, LoginStartPath, LogoutPath, SelectElectionPath, SwitchElectionPath,
};

mod auth;
mod hide_download_warning;
mod index;
mod not_found;
mod robots;
mod select_election;
mod session_expiry;
mod switch_election;
mod switch_locale;
mod well_known;

use crate::AppRequestState;

pub use auth::auth_failure_response;
pub use not_found::not_found;

pub fn router<S: AppRequestState>() -> Router<S> {
    Router::new()
        .typed_get(index::index)
        .typed_get(switch_election::switch_election::<S>)
        .typed_post(switch_election::switch_election_submit::<S>)
        .typed_post(hide_download_warning::hide_download_warning)
}

/// Routes that need a session but NOT the store middleware.
///
/// `/select-election` must be reachable before a stream is chosen, and
/// `/language`, `/session` and `/logout` must be reachable by every session,
/// including committee (CSB) sessions, which `store_middleware` redirects away
/// from app routes. Living behind the session middleware gives the logout and
/// session-extend POSTs the same CSRF and user-agent checks as every other
/// mutating route.
pub fn session_only_router<S>() -> Router<S>
where
    S: AppRequestState + AuthState,
    AuthServiceState: FromRef<S>,
{
    Router::new()
        .typed_post(switch_locale::switch_language::<S>)
        .typed_get(select_election::select_election::<S>)
        .typed_post(select_election::select_election_submit::<S>)
        .typed_get(session_expiry::session_expiry)
        .typed_post(session_expiry::extend_session)
        .typed_get(auth::logout)
        .typed_post(auth::logout_submit::<S>)
}

/// Routes that need no session and no database: `/robots.txt` must always
/// answer crawlers, and RFC 9116 requires `/.well-known/security.txt` to be
/// retrievable anonymously. They stay behind the `eks-key` gate like every
/// other route, since that gate only ensures requests come through our CDN.
pub fn always_public_router<S: AppRequestState>() -> Router<S> {
    Router::new().typed_get(robots::robots_txt).nest(
        "/.well-known",
        Router::new().typed_get(well_known::security_txt),
    )
}

/// Routes mounted outside the session middleware (no session required):
/// - `/login`: GET shows the DigiD start page, POST starts SAML SSO. No CSRF
///   token pre-session; the fetch-metadata layer blocks cross-site POSTs.
/// - `/logged-out`: the post-logout confirmation (TVS T7, also the SLO
///   landing), reached once the session is gone.
pub fn public_router<S>() -> Router<S>
where
    S: AppRequestState + AuthState,
    AuthServiceState: FromRef<S>,
{
    Router::new()
        .route(
            LoginStartPath::PATH,
            get(auth::login_start).post(auth_service::handle_login::<S>),
        )
        .typed_get(auth::logged_out)
}

#[cfg(test)]
mod prefix_guard {
    use axum_extra::routing::TypedPath;

    use crate::{
        candidate_lists::CandidateListsPath, list_designation::ListDesignationUpdatePath,
        persons::PersonsPath, view::DOWNLOAD_WARNING_PREFIXES,
    };

    /// The view layer spells these prefixes out as string constants; this
    /// keeps the two in sync.
    #[test]
    fn download_warning_prefixes_match_typed_paths() {
        assert_eq!(
            DOWNLOAD_WARNING_PREFIXES,
            [
                ListDesignationUpdatePath::PATH,
                CandidateListsPath::PATH,
                PersonsPath::PATH,
            ]
        );
    }
}
