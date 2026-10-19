//! Fuzz harness for the WebAuthn assertion check (see `crate::fuzzing`).
//!
//! Two modes, picked by the first byte: raw JSON as a browser could post it,
//! and a structured variation of a validly signed assertion whose verdict is
//! predicted from the specification (§7.2) and compared with the verifier's.

use std::sync::LazyLock;

use arbitrary::{Arbitrary, Unstructured};
use base64::{
    Engine,
    prelude::{BASE64_STANDARD, BASE64_URL_SAFE, BASE64_URL_SAFE_NO_PAD},
};
use p256::ecdsa::{Signature, SigningKey, signature::Signer};
use sha2::{Digest, Sha256};
use url::{Origin, Url};

use super::assertion::{Assertion, Expected};

const ORIGIN: &str = "https://csb.example.nl";
const RP_ID: &str = "csb.example.nl";
const CHALLENGE: [u8; 32] = [7; 32];
const USER_PRESENT_AND_VERIFIED: u8 = 0x05;
/// Backup eligible, backup state, attested data, extension data.
const FORBIDDEN_FLAGS: u8 = 0x08 | 0x10 | 0x40 | 0x80;
/// Client data members the verifier reads; never added as extra fields.
const CLIENT_DATA_MEMBERS: &[&str] = &["type", "challenge", "origin", "crossOrigin", "topOrigin"];

/// Origins worth trying besides the configured one.
const ORIGINS: &[&str] = &[
    "https://csb.example.nl:443",
    "https://CSB.Example.NL",
    "https://csb.example.nl/",
    "https://csb.example.nl/login?x#y",
    "https://user:pw@csb.example.nl",
    "https://csb.example.nl.",
    "https://csb.example.nl:8443",
    "http://csb.example.nl",
    "https://evil.example.nl",
    "https://csb.example.nl.evil.example",
    "https://csb.example.nl@evil.example",
    "null",
    "",
];

struct Fixture {
    key: SigningKey,
    other_key: SigningKey,
    challenge: String,
    origin: Origin,
    rp_id_hash: [u8; 32],
    /// The bytes the canonical valid assertion's signature covers.
    valid_signed: Signed,
}

static FIXTURE: LazyLock<Fixture> = LazyLock::new(|| {
    let mut fixture = Fixture {
        key: SigningKey::from_bytes(&[0x42; 32].into()).expect("valid scalar"),
        other_key: SigningKey::from_bytes(&[0x43; 32].into()).expect("valid scalar"),
        challenge: BASE64_URL_SAFE_NO_PAD.encode(CHALLENGE),
        origin: Url::parse(ORIGIN).expect("origin").origin(),
        rp_id_hash: Sha256::digest(RP_ID.as_bytes()).into(),
        valid_signed: Signed::default(),
    };
    fixture.valid_signed = Variation::default().build(&fixture).signed;
    fixture
});

pub fn webauthn_assertion(data: &[u8]) {
    let fixture = &*FIXTURE;
    match data.split_first() {
        Some((mode, rest)) if mode & 1 == 0 => check_raw(fixture, rest),
        Some((_, rest)) => {
            if let Ok(variation) = Variation::arbitrary_take_rest(Unstructured::new(rest)) {
                check_variation(fixture, &variation);
            }
        }
        None => {}
    }
}

/// The canonical valid assertion behind the raw-mode marker byte: a seed.
pub fn valid_assertion_seed() -> Vec<u8> {
    let mut seed = vec![0];
    seed.extend(Variation::default().build(&FIXTURE).json.into_bytes());
    seed
}

fn expected(fixture: &Fixture) -> Expected<'_> {
    Expected {
        challenge: &fixture.challenge,
        origin: &fixture.origin,
        rp_id_hash: &fixture.rp_id_hash,
        public_key: fixture.key.verifying_key(),
    }
}

/// Any JSON: only bytes the key signed may ever be accepted.
fn check_raw(fixture: &Fixture, data: &[u8]) {
    let Ok(json) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(assertion) = serde_json::from_str::<Assertion>(json) else {
        return;
    };
    let _ = assertion.challenge();
    if assertion.verify(&expected(fixture)).is_ok() {
        assert_eq!(
            signed_parts(json).as_ref(),
            Some(&fixture.valid_signed),
            "accepted an assertion the key never signed: {json}"
        );
    }
}

fn check_variation(fixture: &Fixture, variation: &Variation) {
    let built = variation.build(fixture);
    let assertion: Assertion = match serde_json::from_str(&built.json) {
        Ok(assertion) => assertion,
        Err(error) => {
            assert!(
                !variation.must_accept(),
                "valid assertion did not parse: {error}: {}",
                built.json
            );
            return;
        }
    };
    let verdict = assertion.verify(&expected(fixture));
    if let Ok(sign_count) = verdict {
        assert!(
            variation.may_accept(&built),
            "accepted an assertion the spec rejects: {variation:?}"
        );
        assert_eq!(sign_count, variation.sign_count, "wrong signature counter");
    } else {
        assert!(
            !variation.must_accept(),
            "rejected a valid assertion ({verdict:?}): {variation:?}"
        );
    }
}

/// The bytes a signature covers.
#[derive(Debug, Default, PartialEq, Eq)]
struct Signed {
    authenticator_data: Vec<u8>,
    client_data_json: Vec<u8>,
}

fn signed_parts(json: &str) -> Option<Signed> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let field = |name: &str| {
        BASE64_URL_SAFE_NO_PAD
            .decode(value["response"][name].as_str()?)
            .ok()
    };
    Some(Signed {
        authenticator_data: field("authenticatorData")?,
        client_data_json: field("clientDataJSON")?,
    })
}

/// What [`Variation::build`] signed and posted.
struct Built {
    json: String,
    signed: Signed,
    /// The key's own signature over `signed`.
    valid_signature: Signature,
    posted_signature: Vec<u8>,
}

#[derive(Debug, Arbitrary)]
enum Text {
    Canonical,
    Listed(u8),
    Other(String),
}

impl Text {
    fn resolve<'a>(&'a self, canonical: &'a str, listed: &[&'a str]) -> &'a str {
        match self {
            Self::Listed(index) if !listed.is_empty() => listed[usize::from(*index) % listed.len()],
            Self::Canonical | Self::Listed(_) => canonical,
            Self::Other(text) => text,
        }
    }
}

#[derive(Debug, Arbitrary)]
enum ChallengeText {
    Canonical,
    Padded,
    StandardAlphabet,
    Bytes(Vec<u8>),
    Other(String),
}

#[derive(Debug, Arbitrary)]
enum SignatureMode {
    Valid,
    OtherKey,
    /// The same signature with `s` negated (ECDSA malleability).
    HighS,
    FlipBit(u16),
    Truncate(u8),
    Other(Vec<u8>),
}

/// One assertion: every field the verifier looks at, canonical by default.
#[derive(Debug, Arbitrary)]
struct Variation {
    credential_type: Text,
    ceremony: Text,
    challenge: ChallengeText,
    origin: Text,
    cross_origin: Option<bool>,
    top_origin: Option<String>,
    extra_client_field: Option<(String, String)>,
    client_data_bom: bool,
    rp_id: Text,
    flags: Option<u8>,
    sign_count: u32,
    trailing_data: Vec<u8>,
    extension_results: Option<String>,
    signature: SignatureMode,
}

impl Default for Variation {
    fn default() -> Self {
        Self {
            credential_type: Text::Canonical,
            ceremony: Text::Canonical,
            challenge: ChallengeText::Canonical,
            origin: Text::Canonical,
            cross_origin: None,
            top_origin: None,
            extra_client_field: None,
            client_data_bom: false,
            rp_id: Text::Canonical,
            flags: None,
            sign_count: 7,
            trailing_data: Vec::new(),
            extension_results: None,
            signature: SignatureMode::Valid,
        }
    }
}

impl Variation {
    fn credential_type(&self) -> &str {
        self.credential_type
            .resolve("public-key", &["Public-Key", "public-key "])
    }

    fn ceremony(&self) -> &str {
        self.ceremony.resolve("webauthn.get", &["webauthn.create"])
    }

    fn origin(&self) -> &str {
        self.origin.resolve(ORIGIN, ORIGINS)
    }

    fn rp_id(&self) -> &str {
        self.rp_id
            .resolve(RP_ID, &["CSB.example.nl", "example.nl", "csb.example.nl."])
    }

    fn challenge(&self) -> String {
        match &self.challenge {
            ChallengeText::Canonical => BASE64_URL_SAFE_NO_PAD.encode(CHALLENGE),
            ChallengeText::Padded => BASE64_URL_SAFE.encode(CHALLENGE),
            ChallengeText::StandardAlphabet => BASE64_STANDARD.encode(CHALLENGE),
            ChallengeText::Bytes(bytes) => BASE64_URL_SAFE_NO_PAD.encode(bytes),
            ChallengeText::Other(text) => text.clone(),
        }
    }

    fn flags(&self) -> u8 {
        self.flags.unwrap_or(USER_PRESENT_AND_VERIFIED)
    }

    fn client_data_json(&self) -> Vec<u8> {
        let mut client_data = serde_json::json!({
            "type": self.ceremony(),
            "challenge": self.challenge(),
            "origin": self.origin(),
        });
        if let Some(cross_origin) = self.cross_origin {
            client_data["crossOrigin"] = cross_origin.into();
        }
        if let Some(top_origin) = &self.top_origin {
            client_data["topOrigin"] = top_origin.as_str().into();
        }
        if let Some((name, value)) = &self.extra_client_field
            && !CLIENT_DATA_MEMBERS.contains(&name.as_str())
        {
            client_data[name] = value.as_str().into();
        }
        let mut json = serde_json::to_vec(&client_data).expect("json");
        if self.client_data_bom {
            json.splice(0..0, *b"\xEF\xBB\xBF");
        }
        json
    }

    fn build(&self, fixture: &Fixture) -> Built {
        let mut authenticator_data = Sha256::digest(self.rp_id().as_bytes()).to_vec();
        authenticator_data.push(self.flags());
        authenticator_data.extend_from_slice(&self.sign_count.to_be_bytes());
        authenticator_data.extend_from_slice(&self.trailing_data);
        let signed = Signed {
            authenticator_data,
            client_data_json: self.client_data_json(),
        };

        let mut message = signed.authenticator_data.clone();
        message.extend_from_slice(&Sha256::digest(&signed.client_data_json));
        let valid_signature: Signature = fixture.key.sign(&message);
        let posted_signature = self.posted_signature(fixture, &message, valid_signature);

        let mut extensions = serde_json::Map::new();
        if let Some(name) = &self.extension_results {
            extensions.insert(name.clone(), true.into());
        }
        let b64 = |bytes: &[u8]| BASE64_URL_SAFE_NO_PAD.encode(bytes);
        let json = serde_json::json!({
            "id": b64(&[0x11; 64]),
            "rawId": b64(&[0x11; 64]),
            "type": self.credential_type(),
            "response": {
                "authenticatorData": b64(&signed.authenticator_data),
                "clientDataJSON": b64(&signed.client_data_json),
                "signature": b64(&posted_signature),
            },
            "clientExtensionResults": extensions,
        })
        .to_string();

        Built {
            json,
            signed,
            valid_signature,
            posted_signature,
        }
    }

    fn posted_signature(&self, fixture: &Fixture, message: &[u8], valid: Signature) -> Vec<u8> {
        let der = |signature: Signature| signature.to_der().as_bytes().to_vec();
        match &self.signature {
            SignatureMode::Valid => der(valid),
            SignatureMode::OtherKey => der(fixture.other_key.sign(message)),
            SignatureMode::HighS => der(high_s(valid)),
            SignatureMode::FlipBit(bit) => {
                let mut bytes = der(valid);
                let bit = usize::from(*bit) % (bytes.len() * 8);
                bytes[bit / 8] ^= 1 << (bit % 8);
                bytes
            }
            SignatureMode::Truncate(len) => {
                let mut bytes = der(valid);
                bytes.truncate(usize::from(*len));
                bytes
            }
            SignatureMode::Other(bytes) => bytes.clone(),
        }
    }

    /// The spec's conditions on what the key signed (§7.2 steps 10 to 19 and
    /// 23) and on the signature: an accepted assertion meets all of them.
    fn may_accept(&self, built: &Built) -> bool {
        let flags = self.flags();
        self.credential_type() == "public-key"
            && self.ceremony() == "webauthn.get"
            && self.challenge() == BASE64_URL_SAFE_NO_PAD.encode(CHALLENGE)
            && same_origin(self.origin())
            && self.cross_origin != Some(true)
            && self.top_origin.is_none()
            && built.signed.authenticator_data[..32] == *Sha256::digest(RP_ID.as_bytes())
            && flags & USER_PRESENT_AND_VERIFIED == USER_PRESENT_AND_VERIFIED
            && flags & FORBIDDEN_FLAGS == 0
            && self.trailing_data.is_empty()
            && self.extension_results.is_none()
            && same_signature(built)
    }

    /// Canonical in every respect: the verifier must accept it.
    fn must_accept(&self) -> bool {
        let flags = self.flags();
        self.credential_type() == "public-key"
            && self.ceremony() == "webauthn.get"
            && self.challenge() == BASE64_URL_SAFE_NO_PAD.encode(CHALLENGE)
            && self.origin() == ORIGIN
            && self.cross_origin != Some(true)
            && self.top_origin.is_none()
            && self.rp_id() == RP_ID
            && flags & USER_PRESENT_AND_VERIFIED == USER_PRESENT_AND_VERIFIED
            && flags & FORBIDDEN_FLAGS == 0
            && self.trailing_data.is_empty()
            && self.extension_results.is_none()
            && matches!(self.signature, SignatureMode::Valid)
    }
}

fn high_s(signature: Signature) -> Signature {
    Signature::from_scalars(signature.r(), -signature.s()).expect("nonzero scalars")
}

/// The posted signature is the key's over this message: its `(r, s)` or the
/// negated twin, however encoded.
fn same_signature(built: &Built) -> bool {
    Signature::from_der(&built.posted_signature).is_ok_and(|posted| {
        posted == built.valid_signature || posted == high_s(built.valid_signature)
    })
}

/// The configured origin by the URL standard: https, this host, port 443.
fn same_origin(origin: &str) -> bool {
    Url::parse(origin).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some(RP_ID)
            && url.port_or_known_default() == Some(443)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The oracle is not vacuous: the canonical assertion verifies.
    #[test]
    fn canonical_variation_is_accepted() {
        let built = Variation::default().build(&FIXTURE);
        let assertion: Assertion = serde_json::from_str(&built.json).expect("assertion json");
        assert_eq!(assertion.verify(&expected(&FIXTURE)), Ok(7));
        assert!(Variation::default().must_accept());
    }
}
