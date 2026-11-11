//! The login challenge (§13.4.3 of the WebAuthn specification,
//! <https://w3c.github.io/webauthn/#sctn-cryptographic-challenges>): 32
//! random bytes minted per visit of the login page and registered one-shot in
//! the pending-request store (15-minute TTL). The finish step consumes the
//! challenge the key signed, so each one answers at most one login (replay
//! defence) and must have been issued by this server.
//! It is pending only together with a secret the issuing browser holds in a
//! cookie, so an assertion cannot finish a login from another browser.

use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use secrecy::SecretString;

use super::LoginFailure;
use crate::{PendingRequestStore, utils::sha256_hex};

const CHALLENGE_LEN: usize = 32;

/// A freshly minted challenge, registered one-shot.
pub(crate) struct Challenge {
    challenge: String,
    /// Browser-binding secret, for the cookie.
    pub(crate) binding: SecretString,
}

impl Challenge {
    /// Mints a random challenge and registers it in the one-shot store.
    pub(crate) async fn issue(pending: &PendingRequestStore) -> Self {
        let random = || BASE64_URL_SAFE_NO_PAD.encode(rand::random::<[u8; CHALLENGE_LEN]>());
        let (challenge, binding) = (random(), random());
        pending
            .register(pending_challenge_id(&challenge, &binding))
            .await;
        Self {
            challenge,
            binding: binding.into(),
        }
    }

    /// The challenge as base64url, for the ceremony options.
    pub(crate) fn as_str(&self) -> &str {
        &self.challenge
    }
}

/// Consumes `challenge` (base64url, as the client data carries it) from the
/// one-shot store: a second finish with the same challenge is a replay,
/// whether or not the first one succeeded. `binding` is the cookie's value.
pub(crate) async fn consume_challenge(
    pending: &PendingRequestStore,
    challenge: &str,
    binding: &str,
) -> Result<(), LoginFailure> {
    let well_formed = BASE64_URL_SAFE_NO_PAD
        .decode(challenge)
        .is_ok_and(|bytes| bytes.len() == CHALLENGE_LEN);
    if !well_formed {
        return Err(LoginFailure::ChallengeMalformed);
    }
    if !pending
        .consume_if_pending(&pending_challenge_id(challenge, binding))
        .await
    {
        return Err(LoginFailure::ChallengeNotPending);
    }
    Ok(())
}

/// Pending-request id for a login challenge. Namespaced so challenges can
/// never collide with SAML AuthnRequest ids in the shared store. The binding
/// is hashed: the store never holds the cookie's secret.
fn pending_challenge_id(challenge: &str, binding: &str) -> String {
    format!(
        "csb-webauthn:{challenge}:{}",
        sha256_hex(binding.as_bytes())
    )
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;

    #[tokio::test]
    async fn issued_challenge_is_consumed_once() {
        let pending = PendingRequestStore::default();
        let challenge = Challenge::issue(&pending).await;
        let raw = BASE64_URL_SAFE_NO_PAD
            .decode(challenge.as_str())
            .expect("base64url");
        assert_eq!(raw.len(), CHALLENGE_LEN);

        let bound = challenge.binding.expose_secret();
        // Another browser neither passes nor burns it.
        for binding in ["", "b3RoZXI"] {
            assert!(matches!(
                consume_challenge(&pending, challenge.as_str(), binding).await,
                Err(LoginFailure::ChallengeNotPending)
            ));
        }
        assert!(
            consume_challenge(&pending, challenge.as_str(), bound)
                .await
                .is_ok()
        );
        assert!(matches!(
            consume_challenge(&pending, challenge.as_str(), bound).await,
            Err(LoginFailure::ChallengeNotPending)
        ));
    }

    #[tokio::test]
    async fn unissued_or_malformed_challenges_are_rejected() {
        let pending = PendingRequestStore::default();
        let binding = Challenge::issue(&pending).await.binding;
        let binding = binding.expose_secret();

        let unissued = BASE64_URL_SAFE_NO_PAD.encode([9; CHALLENGE_LEN]);
        assert!(matches!(
            consume_challenge(&pending, &unissued, binding).await,
            Err(LoginFailure::ChallengeNotPending)
        ));
        let short = BASE64_URL_SAFE_NO_PAD.encode([9; CHALLENGE_LEN - 1]);
        for malformed in ["", "not base64!", "bm90LWlzc3VlZA", short.as_str()] {
            assert!(
                matches!(
                    consume_challenge(&pending, malformed, binding).await,
                    Err(LoginFailure::ChallengeMalformed)
                ),
                "{malformed:?}"
            );
        }
    }
}
