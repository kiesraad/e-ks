//! The `tvs-mock` deployment build: talk to the online (shared) TVS mock
//! service. It defaults `TVS_ENV` to `test` (so the RD endpoint resolves to the
//! TVS mock) and `BASE_URL` to `http://localhost:3000`, and embeds the committed
//! DV cert/key bundle into the binary, so the auth flow works on a host that has
//! no usable `CERTS_DIR` on disk (e.g. the branch test website, whose deployment
//! may still point `CERTS_DIR` at an old bundle location). It does not run a TVS
//! back-channel itself.
//!
//! This embeds the committed test private keys into the binary: it is strictly
//! for mock deployments and must never be enabled for a real environment.
use std::path::{Path, PathBuf};

use tokio::{fs, sync::OnceCell};

use crate::{
    error::AuthError,
    keys::{ENCRYPTION_BASES, SIGNING_BASES, key_pair_paths, tls_paths},
};

/// Default for a required env var when the variable is unset; an explicit
/// value still overrides this. `CERTS_DIR` is not
/// here because the bundle is embedded rather than read from a path, and
/// because a `CERTS_DIR` without a bundle also falls back to it (see
/// [`certs_dir`]).
pub(crate) fn default_for(name: &str) -> Option<String> {
    match name {
        "TVS_ENV" => Some("test".to_string()),
        "BASE_URL" => Some("http://localhost:3000".to_string()),
        _ => None,
    }
}

/// The committed DV cert/key bundle (`auth-service/fixtures`), embedded at
/// compile time. Paths are relative to this source file. Only the files the
/// DV side reads are embedded; the `rd-*` fixtures (for running an RD mock)
/// are not, since this build uses the online TVS mock service.
macro_rules! embedded_fixtures {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_bytes!(concat!("../fixtures/", $name)) as &[u8])),*]
    };
}

const FIXTURES: &[(&str, &[u8])] = embedded_fixtures![
    // mTLS client identity for the back-channel. (The test CA pinned for the
    // back-channel *server* is embedded separately and not read from a path;
    // see [`crate::saml::pki::BACKCHANNEL_ROOT_CA_PEM`].)
    "dv-tls.pem",
    "dv-tls-key.pem",
    // DV SAML signing keys (the second pair is optional, for rollover).
    "dv-signing-1.pem",
    "dv-signing-1-key.pem",
    "dv-signing-2.pem",
    "dv-signing-2-key.pem",
    // DV SAML encryption keys (the second pair is optional, for rollover).
    "dv-encryption-1.pem",
    "dv-encryption-1-key.pem",
    "dv-encryption-2.pem",
    "dv-encryption-2-key.pem",
];

/// The DV bundle directory for a mock build: `configured` (an explicit
/// `CERTS_DIR`) when it actually holds a bundle, otherwise the embedded
/// fixtures. A `CERTS_DIR` pointing at a directory without the bundle is a
/// stale deployment setting, not a reason to refuse to start: a mock build
/// carries the keys it needs, so fall back to them.
pub(crate) async fn certs_dir(configured: Option<PathBuf>) -> Result<PathBuf, AuthError> {
    if let Some(dir) = configured {
        if holds_bundle(&dir).await {
            return Ok(dir);
        }
        tracing::warn!(
            certs_dir = %dir.display(),
            "tvs-mock: CERTS_DIR does not hold a DV cert/key bundle; using the embedded test keys"
        );
    }
    embedded_certs_dir().await
}

/// Whether `dir` holds the mandatory part of a DV bundle: the mTLS client pair
/// and the first signing/encryption pair (the later pairs are rollover-only, so
/// [`crate::keys::discover_key_paths`] treats them as optional).
async fn holds_bundle(dir: &Path) -> bool {
    let tls = tls_paths(dir);
    let mut required = vec![tls.client_cert, tls.client_key];
    required.extend(
        [SIGNING_BASES, ENCRYPTION_BASES]
            .iter()
            .filter_map(|bases| bases.first())
            .flat_map(|base| {
                let paths = key_pair_paths(dir, base);
                [paths.cert, paths.key]
            }),
    );
    for path in &required {
        if !is_file(path).await {
            return false;
        }
    }
    true
}

async fn is_file(path: &Path) -> bool {
    fs::metadata(path).await.is_ok_and(|m| m.is_file())
}

/// Materialize the embedded DV bundle into a per-process temp directory the
/// first time it is needed and hand back its path, so the existing
/// path-based key loaders read it exactly like a real `CERTS_DIR`. The
/// extraction happens once per process (subsequent calls reuse the result).
async fn embedded_certs_dir() -> Result<PathBuf, AuthError> {
    static DIR: OnceCell<Result<PathBuf, String>> = OnceCell::const_new();
    DIR.get_or_init(materialize)
        .await
        .clone()
        .map_err(AuthError::Config)
}

async fn materialize() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("eks-tvs-mock-{}", std::process::id()));
    fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("create {}: {e}", dir.display()))?;
    for (name, bytes) in FIXTURES {
        let path = dir.join(name);
        fs::write(&path, bytes)
            .await
            .map_err(|e| format!("write {}: {e}", path.display()))?;
    }
    tracing::warn!(
        certs_dir = %dir.display(),
        "tvs-mock: using embedded DV test keys against the online TVS mock (mock builds only, never a real deployment)"
    );
    Ok(dir)
}
