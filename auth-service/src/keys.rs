use crate::{
    config::TlsConfig,
    error::{AuthError, Result},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use secrecy::{ExposeSecret, SecretString};
use sha1::{Digest, Sha1};
use sha2::Sha256;
use std::{
    fmt,
    path::{Path, PathBuf},
};
use tokio::fs;
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Key material
// ---------------------------------------------------------------------------
//
// The four values a key pair is made of are all text, and three of them are
// base64-ish blobs that look alike at a call site. They are separate types so
// the compiler rejects a private key handed to something that wanted a
// certificate, a PEM handed to something that wanted the bare base64 body, or a
// thumbprint handed to something that wanted a certificate. Each carries its own
// parsing, so an invalid value cannot exist.

const PEM_CERT_BEGIN: &str = "-----BEGIN CERTIFICATE-----";
const PEM_CERT_END: &str = "-----END CERTIFICATE-----";

/// An RSA private key in PEM form.
///
/// Wraps [`SecretString`], so the key never reaches `Debug` or a log line and is
/// zeroized on drop. `expose_pem` is the single exit point and is
/// crate-private: the key text cannot be extracted by an embedding application,
/// only handed to [`crate::saml::crypto`].
#[derive(Clone)]
pub struct PrivateKeyPem(SecretString);

impl PrivateKeyPem {
    /// Wrap the PEM text of a private key.
    pub fn new(pem: impl Into<String>) -> Self {
        Self(SecretString::from(pem.into()))
    }

    /// The absent private key of a public-only certificate: one advertised in
    /// metadata that never produces a signature (see [`load_cert`]).
    pub fn absent() -> Self {
        Self::new(String::new())
    }

    /// Whether this key can actually sign or decrypt ([`Self::absent`] cannot).
    pub fn is_present(&self) -> bool {
        !self.0.expose_secret().is_empty()
    }

    /// The PEM text, for the crypto backend only.
    pub(crate) fn expose_pem(&self) -> &str {
        self.0.expose_secret()
    }
}

impl fmt::Debug for PrivateKeyPem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PrivateKeyPem(<redacted>)")
    }
}

/// A PEM-armoured X.509 certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertificatePem(String);

impl CertificatePem {
    /// Parse PEM text that actually carries a certificate: the BEGIN/END armour
    /// around a non-empty base64 body.
    ///
    /// Checking here turns a truncated, empty, or wrong-format certificate file
    /// into a config error at load time (with the path in the caller's message)
    /// instead of an opaque crypto failure on the first signature.
    pub fn parse(pem: impl Into<String>) -> Result<Self> {
        let pem = pem.into();
        if !pem.contains(PEM_CERT_BEGIN) || !pem.contains(PEM_CERT_END) {
            return Err(AuthError::Config(format!(
                "not a PEM certificate: expected a {PEM_CERT_BEGIN} block"
            )));
        }
        CertificateBase64::parse(&strip_pem_armour(&pem))?;
        Ok(Self(pem))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The certificate body as base64 text, without armour or whitespace.
    pub fn to_base64(&self) -> CertificateBase64 {
        CertificateBase64(strip_pem_armour(&self.0))
    }

    /// The DER bytes of this certificate. [`Self::parse`] already decoded the
    /// body once, so this cannot fail.
    pub fn to_der(&self) -> Vec<u8> {
        self.to_base64().to_der()
    }

    /// The `KeyName` identifier for this certificate: the lowercase-hex SHA-1
    /// thumbprint of its DER encoding, the convention SAML metadata uses for
    /// `<ds:KeyName>` (and what the TVS Routeringsdienst emits, both in its
    /// `KeyDescriptor`s and in the signature over its metadata). Used purely as a
    /// lookup key to match a signature's `KeyInfo` against a cert from verified
    /// metadata; not a security primitive, so SHA-1 here is not a weakness.
    ///
    pub fn key_name(&self) -> KeyName {
        KeyName::from_digest(&Sha1::digest(self.to_der()))
    }

    /// Every `<ds:KeyName>` form a peer might use to reference this certificate:
    /// the SHA-1 thumbprint (TVS RD convention) and the SHA-256 thumbprint (DigiD
    /// AD convention). Used by [`KeyPair::matches_key_name`] to look a
    /// signature's `KeyInfo` up against a cert from verified metadata regardless
    /// of which thumbprint algorithm the signer chose (eID SAML: "KeyName MAY be
    /// any string").
    pub fn key_names(&self) -> [KeyName; 2] {
        let der = self.to_der();
        [
            KeyName::from_digest(&Sha1::digest(&der)),
            KeyName::from_digest(&Sha256::digest(&der)),
        ]
    }
}

impl fmt::Display for CertificatePem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The DER bytes of a certificate as base64 text, with no PEM armour and no
/// whitespace: what a `<ds:X509Certificate>` element carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertificateBase64(String);

impl CertificateBase64 {
    /// Parse the text of a `<ds:X509Certificate>`, dropping the line breaks and
    /// indentation XML is free to add. Rejects an empty or non-base64 body, so a
    /// malformed metadata `KeyDescriptor` cannot become a certificate that later
    /// code treats as real.
    pub fn parse(text: &str) -> Result<Self> {
        let body: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        if body.is_empty() {
            return Err(AuthError::Xml("empty X509Certificate value".to_string()));
        }
        BASE64
            .decode(body.as_bytes())
            .map_err(|e| AuthError::Xml(format!("X509Certificate is not valid base64: {e}")))?;
        Ok(Self(body))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Wrap the body back into PEM, with the canonical 64-character lines.
    /// Inverse of [`CertificatePem::to_base64`].
    pub fn to_pem(&self) -> CertificatePem {
        let wrapped = self
            .0
            .as_bytes()
            .chunks(64)
            .map(|c| std::str::from_utf8(c).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n");
        CertificatePem(format!("{PEM_CERT_BEGIN}\n{wrapped}\n{PEM_CERT_END}"))
    }

    /// The DER bytes. [`Self::parse`] already decoded them once, so this cannot
    /// fail.
    pub fn to_der(&self) -> Vec<u8> {
        BASE64.decode(self.0.as_bytes()).unwrap_or_default()
    }
}

impl fmt::Display for CertificateBase64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A `<ds:KeyName>`: the lowercase-hex thumbprint of a DER certificate.
///
/// Used purely as a lookup key, to match a signature's `KeyInfo` against a
/// certificate from verified metadata (see [`KeyPair::matches_key_name`]); trust
/// never rests on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyName(String);

impl KeyName {
    /// Parse a thumbprint: trimmed, non-empty, hexadecimal, normalised to
    /// lowercase so two spellings of the same thumbprint compare equal.
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim();
        if s.is_empty() || !s.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(AuthError::Xml(format!(
                "KeyName is not a hex thumbprint: {s:?}"
            )));
        }
        Ok(Self(s.to_ascii_lowercase()))
    }

    fn from_digest(bytes: &[u8]) -> Self {
        Self(bytes.iter().map(|b| format!("{b:02x}")).collect())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for KeyName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The base64 body of a PEM block: every line that is not armour, with all
/// whitespace removed.
fn strip_pem_armour(cert_pem: &str) -> String {
    cert_pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .flat_map(str::chars)
        .filter(|c| !c.is_whitespace())
        .collect()
}

// ---------------------------------------------------------------------------
// Key pairs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct KeyPaths {
    pub cert: PathBuf,
    pub key: PathBuf,
}

#[derive(Debug, Clone)]
pub struct KeyPair {
    pub cert_pem: CertificatePem,
    /// The private key, or [`PrivateKeyPem::absent`] for a public-only
    /// certificate (one advertised in metadata that never signs anything).
    pub key_pem: PrivateKeyPem,
    /// SHA-1 thumbprint of the certificate (used to match KeyName in signatures).
    pub key_name: KeyName,
    /// The certificate as base64 text, without PEM headers or whitespace.
    pub cert_base64: CertificateBase64,
}

impl KeyPair {
    /// Build a [`KeyPair`] from a certificate, deriving `key_name` and
    /// `cert_base64` from it. Pass [`PrivateKeyPem::absent`] for public-only
    /// certificates (metadata-advertised certs that never produce a signature).
    pub fn from_pem(cert_pem: CertificatePem, key_pem: PrivateKeyPem) -> Self {
        Self {
            key_name: cert_pem.key_name(),
            cert_base64: cert_pem.to_base64(),
            cert_pem,
            key_pem,
        }
    }

    /// Whether a signature's `<ds:KeyName>` identifies this certificate.
    ///
    /// The eID §7.6 message tables require a `<KeyInfo>` with a `<KeyName>` (or
    /// `<X509Certificate>`) but do not fix the KeyName format. The TVS
    /// Routeringsdienst references its certs by their SHA-1 thumbprint; the
    /// DigiD Authenticatiedienst signs the Assertion referencing the same kind
    /// of cert by its **SHA-256** thumbprint. Both forms identify the cert from
    /// verified metadata (a lookup key, not a security primitive), so accept
    /// either thumbprint algorithm.
    pub fn matches_key_name(&self, candidate: &str) -> bool {
        let Ok(candidate) = KeyName::parse(candidate) else {
            return false;
        };
        self.key_name == candidate || self.cert_pem.key_names().contains(&candidate)
    }
}

/// A DV private key offered for `EncryptedID` decryption, with the `KeyName` it
/// is advertised under in the DV metadata.
///
/// A named struct rather than a `(&str, &str)` pair: two adjacent strings are
/// trivially swappable at a call site, and one of them is a private key.
#[derive(Debug, Clone, Copy)]
pub struct DecryptionKey<'a> {
    pub key_pem: &'a PrivateKeyPem,
    /// Diagnostics only: the crypto backend unwraps the `EncryptedKey` with the
    /// private key itself, never by matching this name.
    pub key_name: &'a KeyName,
}

impl<'a> DecryptionKey<'a> {
    /// The decryption keys of a [`KeySet`], in configured order.
    pub fn from_key_set(keys: &'a KeySet) -> Vec<Self> {
        keys.encryption
            .iter()
            .map(|k| Self {
                key_pem: &k.key_pem,
                key_name: &k.key_name,
            })
            .collect()
    }
}

#[derive(Default, Debug, Clone)]
pub struct KeySet {
    pub signing: Vec<KeyPair>,
    pub encryption: Vec<KeyPair>,
}

impl KeySet {
    /// The key used to sign outgoing messages.
    ///
    /// [`load_key_set`] rejects an empty signing list, so this only fails on a
    /// hand-built `KeySet` (e.g. [`Default::default`], as used by
    /// [`AuthServiceState::new_empty`](crate::AuthServiceState::new_empty) for
    /// dev-login-only boots). An error rather than a panic so such a deployment
    /// answers a SAML request with the "login unavailable" page instead of
    /// taking the process down.
    pub fn primary_signing(&self) -> Result<&KeyPair> {
        self.signing.first().ok_or_else(|| {
            AuthError::Config("no DV signing key is configured (check CERTS_DIR)".to_string())
        })
    }
}

/// Load one cert/key pair from disk. Both file paths are named in the error, so
/// a missing or malformed bundle file is diagnosable from the log alone.
pub async fn load_key_pair(paths: &KeyPaths) -> Result<KeyPair> {
    // SECURITY: never log key_pem contents; only the public path it came from.
    debug!(
        "[keys] Loading key pair: cert={}, key={}",
        paths.cert.display(),
        paths.key.display()
    );
    let cert_pem = read_cert(&paths.cert).await?;
    let key_pem = fs::read_to_string(&paths.key).await.map_err(|e| {
        AuthError::Config(format!("Failed to read key {}: {e}", paths.key.display()))
    })?;

    let pair = KeyPair::from_pem(cert_pem, PrivateKeyPem::new(key_pem));
    debug!(
        "[keys] Loaded key pair: key_name={} (cert={}, cert_len={}, private_key_present={})",
        pair.key_name,
        paths.cert.display(),
        pair.cert_pem.as_str().len(),
        pair.key_pem.is_present()
    );
    Ok(pair)
}

/// Read and parse a PEM certificate, naming the path in either failure.
async fn read_cert(cert_path: &Path) -> Result<CertificatePem> {
    let text = fs::read_to_string(cert_path).await.map_err(|e| {
        AuthError::Config(format!("Failed to read cert {}: {e}", cert_path.display()))
    })?;
    CertificatePem::parse(text)
        .map_err(|e| AuthError::Config(format!("Invalid cert {}: {e}", cert_path.display())))
}

pub async fn load_key_set(signing: &[KeyPaths], encryption: &[KeyPaths]) -> Result<KeySet> {
    debug!(
        "[keys] load_key_set: {} signing path(s), {} encryption path(s)",
        signing.len(),
        encryption.len()
    );
    if signing.is_empty() {
        return Err(AuthError::Config(
            "at least one signing key pair is required".to_string(),
        ));
    }
    let signing = load_key_pairs(signing).await?;
    let encryption = load_key_pairs(encryption).await?;
    debug!(
        "[keys] load_key_set OK: signing={}, encryption={}",
        signing.len(),
        encryption.len()
    );
    Ok(KeySet {
        signing,
        encryption,
    })
}

/// Load every pair in `paths`, in order, stopping at the first failure.
async fn load_key_pairs(paths: &[KeyPaths]) -> Result<Vec<KeyPair>> {
    let mut pairs = Vec::with_capacity(paths.len());
    for path in paths {
        pairs.push(load_key_pair(path).await?);
    }
    Ok(pairs)
}

/// Load a certificate (public key only) into a [`KeyPair`] with no private key.
/// Used for certificates advertised in metadata but never used to produce a
/// signature (e.g. the DV's mTLS client certificate).
pub async fn load_cert(cert_path: &Path) -> Result<KeyPair> {
    Ok(KeyPair::from_pem(
        read_cert(cert_path).await?,
        PrivateKeyPem::absent(),
    ))
}

/// Load the DV's mTLS client certificate so it can be published as an extra
/// `use="signing"` KeyDescriptor in the SP metadata. eID §8.3 requires the TLS
/// client certificate to be advertised as a signing certificate.
///
/// Best-effort: the mTLS handshake reads the certificate separately at request
/// time (see [`crate::bindings::soap`]), so a missing or unreadable cert here
/// is logged and omitted from the metadata rather than failing startup. An empty
/// path (an unconfigured [`TlsConfig`], e.g. in tests) is treated as "not
/// configured" without a warning.
pub async fn load_metadata_tls_cert(cert_path: &Path) -> Option<KeyPair> {
    if cert_path.as_os_str().is_empty() {
        return None;
    }
    match load_cert(cert_path).await {
        Ok(cert) => Some(cert),
        Err(e) => {
            warn!("[keys] TLS client cert not published in SP metadata: {e}");
            None
        }
    }
}

// ---------------------------------------------------------------------------
// On-disk bundle layout
// ---------------------------------------------------------------------------

// File layout of the DV cert/key bundle under `certs_dir`, defined once so every
// caller derives the same paths. The `tvs-mock` `FIXTURES` list mirrors these
// names (it needs literals for `include_bytes!`).
const TLS_CERT_FILE: &str = "dv-tls.pem";
const TLS_KEY_FILE: &str = "dv-tls-key.pem";
pub const SIGNING_BASES: &[&str] = &["dv-signing-1", "dv-signing-2"];
pub const ENCRYPTION_BASES: &[&str] = &["dv-encryption-1", "dv-encryption-2"];

/// The cert/key file paths for a key-family base name under `certs_dir`.
pub fn key_pair_paths(certs_dir: &Path, base: &str) -> KeyPaths {
    KeyPaths {
        cert: certs_dir.join(format!("{base}.pem")),
        key: certs_dir.join(format!("{base}-key.pem")),
    }
}

/// The mTLS client cert/key paths under `certs_dir` (eID §9.4 back-channel).
pub fn tls_paths(certs_dir: &Path) -> TlsConfig {
    TlsConfig {
        client_cert: certs_dir.join(TLS_CERT_FILE),
        client_key: certs_dir.join(TLS_KEY_FILE),
    }
}

/// Build the cert/key paths for the key families under `certs_dir`.
///
/// The first base's key pair is mandatory: the list always contains at least
/// one. Every base after it (e.g. a second key kept for rollover) is optional:
/// it is included only when its certificate file is present on disk, so a
/// single-key bundle publishes a single key pair in the metadata.
pub async fn discover_key_paths(certs_dir: &Path, bases: &[&str]) -> Vec<KeyPaths> {
    let mut discovered = Vec::with_capacity(bases.len());
    for (index, base) in bases.iter().enumerate() {
        let paths = key_pair_paths(certs_dir, base);
        if index > 0 && !file_exists(&paths.cert).await {
            continue;
        }
        discovered.push(paths);
    }
    discovered
}

/// Whether `path` exists, treating an unanswerable stat (e.g. a permission
/// error on a parent directory) as absent, like [`Path::exists`].
pub(crate) async fn file_exists(path: &Path) -> bool {
    fs::try_exists(path).await.unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_PEM: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/dv-signing-1.pem"
    ));

    fn fake_cert() -> CertificatePem {
        CertificatePem::parse(FIXTURE_PEM).expect("fixture PEM parses")
    }

    #[test]
    fn to_base64_strips_pem_headers() {
        let b64 = fake_cert().to_base64();
        assert!(!b64.as_str().contains("BEGIN"));
        assert!(!b64.as_str().contains("END"));
        assert!(!b64.as_str().contains('\n'));
        assert!(!b64.as_str().is_empty());
    }

    #[test]
    fn certificate_pem_rejects_non_pem_input() {
        // A truncated or wrong-format cert file is refused at load time rather
        // than failing opaquely on the first signature.
        for bad in ["", "not a certificate", "-----BEGIN CERTIFICATE-----"] {
            assert!(
                CertificatePem::parse(bad).is_err(),
                "{bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn certificate_pem_rejects_empty_block() {
        let empty = format!("{PEM_CERT_BEGIN}\n\n{PEM_CERT_END}");
        assert!(CertificatePem::parse(empty).is_err());
    }

    #[test]
    fn certificate_base64_rejects_empty_and_non_base64() {
        assert!(CertificateBase64::parse("   \n  ").is_err());
        assert!(CertificateBase64::parse("not base64!!").is_err());
    }

    #[test]
    fn certificate_base64_ignores_xml_whitespace() {
        // A `<ds:X509Certificate>` may wrap and indent its body.
        let b64 = fake_cert().to_base64();
        let indented = format!("\n    {}\n    ", b64.as_str());
        assert_eq!(CertificateBase64::parse(&indented).unwrap(), b64);
    }

    #[test]
    fn key_name_is_hex_sha1() {
        let name = fake_cert().key_name();
        assert_eq!(name.as_str().len(), 40, "SHA-1 hex should be 40 chars");
        assert!(name.as_str().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn key_name_is_deterministic() {
        assert_eq!(fake_cert().key_name(), fake_cert().key_name());
    }

    #[test]
    fn key_names_yields_sha1_and_sha256() {
        let names = fake_cert().key_names();
        assert_eq!(names[0].as_str().len(), 40, "first is SHA-1 (40 hex chars)");
        assert_eq!(
            names[1].as_str().len(),
            64,
            "second is SHA-256 (64 hex chars)"
        );
        assert_eq!(names[0], fake_cert().key_name());
    }

    #[test]
    fn key_name_parse_normalises_case_and_whitespace() {
        let upper = KeyName::parse("  DEADBEEF  ").unwrap();
        assert_eq!(upper.as_str(), "deadbeef");
    }

    #[test]
    fn key_name_parse_rejects_non_thumbprints() {
        for bad in ["", "   ", "not-hex", "dead beef"] {
            assert!(KeyName::parse(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    fn fake_key_pair() -> KeyPair {
        KeyPair::from_pem(fake_cert(), PrivateKeyPem::absent())
    }

    #[test]
    fn matches_key_name_accepts_sha1_thumbprint() {
        let kp = fake_key_pair();
        assert!(kp.matches_key_name(fake_cert().key_names()[0].as_str()));
    }

    #[test]
    fn matches_key_name_accepts_sha256_thumbprint() {
        // The DigiD AD references its signing cert by SHA-256 thumbprint, not
        // the SHA-1 form stored in `key_name`; both must match.
        let kp = fake_key_pair();
        let sha256 = fake_cert().key_names()[1].clone();
        assert_ne!(sha256, kp.key_name);
        assert!(kp.matches_key_name(sha256.as_str()));
        assert!(
            kp.matches_key_name(&format!("  {}  ", sha256.as_str().to_uppercase())),
            "trims and case-folds input"
        );
    }

    #[test]
    fn matches_key_name_rejects_unknown() {
        let kp = fake_key_pair();
        assert!(!kp.matches_key_name("deadbeef"));
    }

    #[test]
    fn matches_key_name_rejects_malformed_candidate() {
        // A `<ds:KeyName>` that is not a thumbprint at all never matches, and
        // never panics.
        let kp = fake_key_pair();
        assert!(!kp.matches_key_name(""));
        assert!(!kp.matches_key_name("<script>"));
    }

    #[test]
    fn private_key_debug_never_reveals_the_key() {
        let key = PrivateKeyPem::new("-----BEGIN PRIVATE KEY-----secret-----END PRIVATE KEY-----");
        let rendered = format!("{key:?}");
        assert!(!rendered.contains("secret"), "{rendered}");
        assert!(key.is_present());
        assert!(!PrivateKeyPem::absent().is_present());
    }

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
    }

    #[test]
    fn to_pem_round_trips_to_base64() {
        // Wrapping the stripped base64 back into PEM and stripping it again is a
        // no-op, and the wrapped body uses the canonical 64-char lines.
        let b64 = fake_cert().to_base64();
        let pem = b64.to_pem();
        assert!(pem.as_str().starts_with("-----BEGIN CERTIFICATE-----\n"));
        assert!(pem.as_str().ends_with("\n-----END CERTIFICATE-----"));
        assert_eq!(pem.to_base64(), b64);
        for line in pem
            .as_str()
            .lines()
            .filter(|l| !l.starts_with("-----") && !l.is_empty())
        {
            assert!(line.len() <= 64);
        }
    }

    #[tokio::test]
    async fn load_key_set_errors_when_signing_empty() {
        // An empty signing list is a config error, so `primary_signing` is
        // guaranteed to succeed on any loaded KeySet.
        let err = load_key_set(&[], &[]).await.unwrap_err();
        assert!(
            matches!(&err, AuthError::Config(m) if m.contains("signing key pair")),
            "{err:?}"
        );
    }

    #[test]
    fn primary_signing_errors_on_an_unloaded_key_set() {
        // The dev-login-only boot (`AuthServiceState::new_empty`) has no keys:
        // a SAML request against it must report an error, never panic.
        let err = KeySet::default().primary_signing().unwrap_err();
        assert!(
            matches!(&err, AuthError::Config(m) if m.contains("no DV signing key")),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn load_key_set_errors_when_cert_missing() {
        // A signing entry whose cert file does not exist fails with a Config error.
        let paths = KeyPaths {
            cert: fixtures_dir().join("does-not-exist.pem"),
            key: fixtures_dir().join("does-not-exist-key.pem"),
        };
        let err = load_key_set(std::slice::from_ref(&paths), &[])
            .await
            .unwrap_err();
        assert!(
            matches!(&err, AuthError::Config(m) if m.contains("Failed to read cert")),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn load_key_set_errors_when_cert_is_not_pem() {
        // A file that exists but holds no certificate names the path in the error.
        let dir = std::env::temp_dir().join("eks-keys-test-not-pem");
        std::fs::create_dir_all(&dir).unwrap();
        let paths = key_pair_paths(&dir, "dv-signing-1");
        std::fs::write(&paths.cert, b"garbage").unwrap();
        std::fs::write(&paths.key, b"key").unwrap();
        let err = load_key_set(std::slice::from_ref(&paths), &[])
            .await
            .unwrap_err();
        assert!(
            matches!(&err, AuthError::Config(m) if m.contains("Invalid cert")),
            "{err:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn load_key_set_errors_when_key_missing() {
        // Cert present, private key absent: the key-read branch reports the error.
        let paths = KeyPaths {
            cert: fixtures_dir().join("dv-signing-1.pem"),
            key: fixtures_dir().join("does-not-exist-key.pem"),
        };
        let err = load_key_set(std::slice::from_ref(&paths), &[])
            .await
            .unwrap_err();
        assert!(
            matches!(&err, AuthError::Config(m) if m.contains("Failed to read key")),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn load_key_set_loads_fixture_pair() {
        // The success path: the committed DV signing fixture loads and derives a
        // key name and public base64.
        let paths = KeyPaths {
            cert: fixtures_dir().join("dv-signing-1.pem"),
            key: fixtures_dir().join("dv-signing-1-key.pem"),
        };
        let set = load_key_set(std::slice::from_ref(&paths), &[])
            .await
            .unwrap();
        assert_eq!(set.signing.len(), 1);
        assert_eq!(set.encryption.len(), 0);
        let primary = set.primary_signing().unwrap();
        assert_eq!(primary.key_name.as_str().len(), 40);
        assert!(!primary.cert_base64.as_str().is_empty());
        assert!(primary.key_pem.is_present());
    }

    #[tokio::test]
    async fn load_cert_reads_public_cert_without_private_key() {
        let cert = load_cert(&fixtures_dir().join("dv-tls.pem")).await.unwrap();
        assert_eq!(cert.key_name.as_str().len(), 40);
        assert!(!cert.cert_base64.as_str().is_empty());
        // A public-only cert carries no private key.
        assert!(!cert.key_pem.is_present());
    }

    #[tokio::test]
    async fn load_cert_errors_when_file_missing() {
        let err = load_cert(&fixtures_dir().join("nope.pem"))
            .await
            .unwrap_err();
        assert!(
            matches!(&err, AuthError::Config(m) if m.contains("Failed to read cert")),
            "{err:?}"
        );
    }
}
