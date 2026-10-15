//! The login challenge (§13.4.3 of the WebAuthn specification,
//! <https://w3c.github.io/webauthn/#sctn-cryptographic-challenges>): 32
//! random bytes minted per visit of the login page and registered one-shot in
//! the pending-request store (15-minute TTL). The finish step consumes the
//! challenge the key signed, so each one answers at most one login (replay
//! defence) and must have been issued by this server.

use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};

use super::LoginFailure;
use crate::PendingRequestStore;

const CHALLENGE_LEN: usize = 32;

/// A freshly minted challenge, registered one-shot.
pub(crate) struct Challenge(String);

impl Challenge {
    /// Mints a random challenge and registers it in the one-shot store.
    pub(crate) async fn issue(pending: &PendingRequestStore) -> Self {
        let challenge = BASE64_URL_SAFE_NO_PAD.encode(rand::random::<[u8; CHALLENGE_LEN]>());
        pending.register(pending_challenge_id(&challenge)).await;
        Self(challenge)
    }

    /// The challenge as base64url, for the ceremony options.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Consumes `challenge` (base64url, as the client data carries it) from the
/// one-shot store: a second finish with the same challenge is a replay,
/// whether or not the first one succeeded.
pub(crate) async fn consume_challenge(
    pending: &PendingRequestStore,
    challenge: &str,
) -> Result<(), LoginFailure> {
    let well_formed = BASE64_URL_SAFE_NO_PAD
        .decode(challenge)
        .is_ok_and(|bytes| bytes.len() == CHALLENGE_LEN);
    if !well_formed {
        return Err(LoginFailure::ChallengeMalformed);
    }
    if !pending
        .consume_if_pending(&pending_challenge_id(challenge))
        .await
    {
        return Err(LoginFailure::ChallengeNotPending);
    }
    Ok(())
}

/// Pending-request id for a login challenge. Namespaced so challenges can
/// never collide with SAML AuthnRequest ids in the shared store.
fn pending_challenge_id(challenge: &str) -> String {
    format!("csb-webauthn:{challenge}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn issued_challenge_is_consumed_once() {
        let pending = PendingRequestStore::default();
        let challenge = Challenge::issue(&pending).await;
        let raw = BASE64_URL_SAFE_NO_PAD
            .decode(challenge.as_str())
            .expect("base64url");
        assert_eq!(raw.len(), CHALLENGE_LEN);

        assert!(
            consume_challenge(&pending, challenge.as_str())
                .await
                .is_ok()
        );
        assert!(matches!(
            consume_challenge(&pending, challenge.as_str()).await,
            Err(LoginFailure::ChallengeNotPending)
        ));
    }

    #[tokio::test]
    async fn unissued_or_malformed_challenges_are_rejected() {
        let pending = PendingRequestStore::default();
        let _ = Challenge::issue(&pending).await;

        let unissued = BASE64_URL_SAFE_NO_PAD.encode([9; CHALLENGE_LEN]);
        assert!(matches!(
            consume_challenge(&pending, &unissued).await,
            Err(LoginFailure::ChallengeNotPending)
        ));
        let short = BASE64_URL_SAFE_NO_PAD.encode([9; CHALLENGE_LEN - 1]);
        for malformed in ["", "not base64!", "bm90LWlzc3VlZA", short.as_str()] {
            assert!(
                matches!(
                    consume_challenge(&pending, malformed).await,
                    Err(LoginFailure::ChallengeMalformed)
                ),
                "{malformed:?}"
            );
        }
    }
}
