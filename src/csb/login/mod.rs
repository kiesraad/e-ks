//! Login for CSB (central electoral committee) users: GitHub OAuth and
//! passkeys (WebAuthn), each enabled by its own configuration. Both are fully
//! separate from the political-group login (SAML DigiD/TVS, implemented in
//! the auth-service).
//!
//! **GitHub** ([`crate::GithubOauthConfig`]): committee members authenticate
//! with their GitHub account through the OAuth authorization-code flow and
//! must appear on the configured allowlist of numeric GitHub account ids.
//! - `GET /csb/login/start`, the target of the login button, registers a
//!   one-shot random `state` nonce in the pending-request store (15-minute
//!   TTL, single use) and binds it to the browser with a short-lived cookie,
//!   then redirects to GitHub. Nothing an attacker can forge a request to
//!   reach: a cross-site hit only mints a nonce for the victim's own browser,
//!   which is useless without the matching cookie, and the login-CSRF defence
//!   sits on the callback, where the nonce and cookie must agree.
//! - `GET /csb/login/callback` accepts the nonce only when it matches the
//!   browser's cookie (constant-time) and is still pending; the code is then
//!   exchanged server-side and the account id checked against the allowlist.
//!
//! **Passkeys** ([`crate::CsbPasskeyConfig`]): committee members register
//! passkeys from an existing CSB session (see `csb::passkeys`), so the first
//! passkey rides on a GitHub login and every later one on GitHub or a passkey.
//! At login they type their account name; the server answers with the
//! account's credential ids (username-first, so hardware keys without
//! resident storage work too) and the browser produces an assertion.
//! - `POST /csb/login/passkey/start` looks the name up and returns the
//!   WebAuthn request options. A name no account has gets a decoy challenge
//!   with stable fake credential ids, so the endpoint does not reveal which
//!   names exist. The ceremony state is sealed into an encrypted cookie
//!   together with a one-shot nonce (`passkey-login:{nonce}` in the
//!   pending-request store), the same browser binding as the OAuth `state`.
//! - `POST /csb/login/passkey/finish` opens the cookie, consumes the nonce
//!   (expiry and replay defence), verifies the assertion against the
//!   account's current passkeys and persists the updated signature counter.
//!
//! Both flows end in [`establish_committee_session`]: any pre-existing
//! session is dropped (fixation defence), a
//! [`crate::SessionUser::CentralElectoralCommittee`] session is created and
//! the login is recorded on the shared CSB main stream for the audit log.

mod github;
mod pages;
mod paths;
mod state_cookie;

pub use pages::public_router;
pub use paths::{
    CsbLoginCallbackPath, CsbLoginPath, CsbLoginStartPath, CsbPasskeyLoginFinishPath,
    CsbPasskeyLoginStartPath,
};

use axum::http::HeaderMap;
use axum_extra::extract::CookieJar;

use crate::{
    AppError, AppRequestState, Config, CsbMainAction, CsbUser, GithubOauthConfig, Locale, Session,
    auth::session_extractor::{establish_session, user_agent_hash},
};

/// Which login methods this deployment offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CsbLoginMethods {
    github: bool,
    passkey: bool,
}

/// The configured login methods, or 404 when this deployment has no CSB login.
fn require_csb_login(config: &Config) -> Result<CsbLoginMethods, AppError> {
    let methods = CsbLoginMethods {
        github: config.github_oauth.is_some(),
        passkey: config.csb_passkey.is_some(),
    };
    if methods.github || methods.passkey {
        Ok(methods)
    } else {
        Err(AppError::GenericNotFound)
    }
}

/// The GitHub OAuth config, or 404 when this deployment has no GitHub login.
fn require_github_oauth(config: &Config) -> Result<&GithubOauthConfig, AppError> {
    config
        .github_oauth
        .as_ref()
        .ok_or(AppError::GenericNotFound)
}

/// Pending-request id for an OAuth `state` nonce. Namespaced so nonces can
/// never collide with SAML AuthnRequest ids in the shared store.
fn pending_state_id(nonce: &str) -> String {
    format!("github-oauth:{nonce}")
}

/// Creates the committee session for an authenticated member and records
/// the login on the shared CSB main stream for the audit log. Shared by the
/// GitHub and the passkey login; returns the jar with the session cookie set.
async fn establish_committee_session<S: AppRequestState>(
    state: &S,
    user: CsbUser,
    jar: CookieJar,
    headers: &HeaderMap,
) -> Result<CookieJar, AppError> {
    let election = state.config().default_election;

    let mut session = Session::for_committee(user.clone(), election, Locale::default());
    session.set_user_agent_hash(user_agent_hash(headers));

    let store = state.csb_main_store(election).await?;
    store.update(CsbMainAction::Login.by(user)).await?;

    Ok(establish_session(state.sessions(), jar, session).await)
}

#[cfg(test)]
pub(crate) mod test_support {
    use secrecy::SecretString;

    use crate::{Config, GithubOauthConfig, GithubUserId, auth::passkey::test_support};

    /// The GitHub account id on the test allowlist.
    pub(crate) fn allowed_user_id() -> GithubUserId {
        "583231".parse().expect("valid id")
    }

    /// Test config with the GitHub OAuth login enabled.
    pub(crate) fn github_test_config() -> Config {
        let mut config = Config::new_test();
        config.github_oauth = Some(GithubOauthConfig {
            client_id: "Iv1.testclient".to_string(),
            client_secret: SecretString::from("test-secret"),
            allowed_user_ids: vec![allowed_user_id()],
        });
        config
    }

    /// Test config with the passkey login enabled.
    pub(crate) fn passkey_test_config() -> Config {
        let mut config = Config::new_test();
        config.csb_passkey = Some(test_support::test_passkey_config());
        config
    }

    /// Test config with both logins enabled.
    pub(crate) fn both_test_config() -> Config {
        let mut config = github_test_config();
        config.csb_passkey = Some(test_support::test_passkey_config());
        config
    }
}
