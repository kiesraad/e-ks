//! Reading [`AuthConfig`](super::AuthConfig)'s inputs from the environment.
//!
//! Split out of `config/mod.rs` so the module the rest of the crate depends on
//! carries only the configuration types, and holds the lookup closure in a
//! reader rather than threading `&mut F` through every function.

use std::{env, path::PathBuf};

use super::{Environment, PreselectedAd};
use crate::error::AuthError;

/// Reads the deployment's environment variables through an injected lookup, so
/// tests can drive config parsing without touching the process environment.
pub(super) struct EnvReader<F> {
    lookup: F,
}

impl<F> EnvReader<F>
where
    F: FnMut(&str) -> Result<String, env::VarError>,
{
    pub(super) fn new(lookup: F) -> Self {
        Self { lookup }
    }

    /// `TVS_ENV` selects the environment; every environment-specific value (TVS
    /// RD endpoint, DV EntityID/ServiceUUID, back-channel trust) derives from
    /// it. Required outside `tvs-mock` builds, so a real deployment must choose
    /// deliberately rather than silently using the test mock.
    pub(super) fn environment(&mut self) -> Result<Environment, AuthError> {
        let environment: Environment = self
            .required("TVS_ENV")?
            .parse()
            .map_err(AuthError::Config)?;

        // `tvs-mock` pins the committed test CA and embeds test keys, so it is
        // only valid against the TVS mock; refuse a real TVS_ENV with the
        // feature on.
        #[cfg(feature = "tvs-mock")]
        if environment != Environment::Test {
            return Err(AuthError::Config(format!(
                "the `tvs-mock` feature is compiled in but TVS_ENV={environment:?} is a real \
                 TVS environment; tvs-mock embeds test keys and pins the test CA, so it must \
                 only run against the TVS mock (TVS_ENV=test)"
            )));
        }

        Ok(environment)
    }

    /// `CERTS_DIR` holds the DV cert/key bundle. Required outside `tvs-mock`;
    /// with the feature the embedded bundle is extracted as a fallback (works
    /// on a deployed host), both when the variable is unset and when it points
    /// somewhere that holds no bundle.
    pub(super) async fn certs_dir(&mut self) -> Result<PathBuf, AuthError> {
        let configured = (self.lookup)("CERTS_DIR").ok().map(PathBuf::from);

        #[cfg(feature = "tvs-mock")]
        return crate::tvs_mock::certs_dir(configured).await;

        #[cfg(not(feature = "tvs-mock"))]
        return configured.ok_or_else(|| missing("CERTS_DIR"));
    }

    /// `PRESELECTED_AD` chooses which AD to pre-select in the AuthnRequest
    /// Scoping (eID §7.3). Unset or empty defaults to `Select` (no Scoping).
    pub(super) fn preselected_ad(&mut self) -> Result<PreselectedAd, AuthError> {
        match (self.lookup)("PRESELECTED_AD") {
            Ok(value) if !value.trim().is_empty() => value.parse().map_err(AuthError::Config),
            _ => Ok(PreselectedAd::default()),
        }
    }

    /// `BASE_URL` is the public origin the SP ACS/SLO URLs derive from. A
    /// non-https BASE_URL downgrades the `__Host-` flow cookie (see
    /// `handlers::flow`), so https is required for real environments.
    pub(super) fn base_url(&mut self, environment: Environment) -> Result<String, AuthError> {
        let base_url = self.required("BASE_URL")?;
        if environment != Environment::Test && !base_url.starts_with("https://") {
            return Err(AuthError::Config(format!(
                "BASE_URL must be https for TVS_ENV={environment:?} (got {base_url:?}); a \
                 non-https origin downgrades the __Host- SSO flow cookie"
            )));
        }
        Ok(base_url)
    }

    /// Read an env var, falling back to the `tvs-mock` default when the variable
    /// is absent and that feature is enabled, or erroring otherwise.
    fn required(&mut self, name: &'static str) -> Result<String, AuthError> {
        if let Ok(value) = (self.lookup)(name) {
            return Ok(value);
        }
        #[cfg(feature = "tvs-mock")]
        if let Some(value) = crate::tvs_mock::default_for(name) {
            return Ok(value);
        }
        Err(missing(name))
    }
}

/// A missing-required-variable [`AuthError::Config`].
fn missing(name: &str) -> AuthError {
    AuthError::Config(format!("missing environment variable: {name}"))
}
