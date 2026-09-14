use std::{env, path::PathBuf};

mod env_reader;

use axum_extra::routing::TypedPath;

use crate::{
    error::AuthError,
    keys::{
        ENCRYPTION_BASES, KeyPaths, SIGNING_BASES, discover_key_paths, key_pair_paths, tls_paths,
    },
    types::{EndpointUrl, EntityId, RedirectTarget, ServiceUuid},
};

// OIN of the TVS RD (Routeringsdienst, operated by DICTU). The RD metadata
// signing certificate MUST carry this in its `Subject.serialNumber` (eID §9.1);
// it is checked alongside the chain-to-pinned-root validation so a different
// PKIoverheid participant's certificate cannot impersonate the RD. The same OIN
// appears in the RD EntityID (see the `RD_ENTITY_ID_*` constants below). It is
// environment-independent (production, pre-production and the mock share it).
pub const RD_OIN: &str = "00000004000000149000";

// TVS RD (Routeringsdienst) base URLs per environment. `Test` is the shared
// standalone TVS mock used by all development/test environments: its
// front-channel (metadata, browser redirects) is served over public TLS, while
// its mTLS back-channel uses the repo's test CA, pinned by the `tvs-mock`
// feature (see [`crate::saml::pki::BACKCHANNEL_ROOT_CA_PEM`]).
pub const TVS_TEST_BASE_URL: &str = "https://tvs-mock.eks-test.nl";
pub const TVS_PREPRODUCTION_BASE_URL: &str = "https://pp2.toegang.overheid.nl";
pub const TVS_PRODUCTION_BASE_URL: &str = "https://rd2.toegang.overheid.nl";

// Kiesraad DV (Dienstverlener) display name, sent in the SP metadata /
// AuthnRequest. Environment-independent.
pub const DV_SERVICE_NAME: &str = "Kiesraad";

// Kiesraad DV EntityID per environment (eID §10.2:
// `urn:nl-eid-gdi:1.0:DV:<OIN>:entities:<index>`), where the OIN
// (Organisatie-IdentificatieNummer) `00000004185618890000` is the Kiesraad's and
// also appears as `Subject.serialNumber` in our PKIoverheid certificates (TVS
// "Certificaatgebruik" §3.1). Index `9xxx` is reserved for test/pre-production,
// `0xxx` for production. Defined in code (not configured) and sourced from the
// TVS onboarding.
const DV_ENTITY_ID_PREPRODUCTION: &str = "urn:nl-eid-gdi:1.0:DV:00000004185618890000:entities:9001";
const DV_ENTITY_ID_PRODUCTION: &str = "urn:nl-eid-gdi:1.0:DV:00000004185618890000:entities:0001";

// TVS RD (Routeringsdienst) EntityID per environment (eID §10.2:
// `urn:nl-eid-gdi:1.0:RD:<OIN>:entities:<index>`, OIN = `RD_OIN`). Pinned so the
// expected Response/Assertion `Issuer` is a configured constant rather than a
// value taken from the (only self-signature-checked) metadata document. Sourced
// from the live RD metadata: production serves `:entities:0001`; pre-production
// serves `:entities:9002`, which the shared TVS mock mirrors.
const RD_ENTITY_ID_PRODUCTION: &str = "urn:nl-eid-gdi:1.0:RD:00000004000000149000:entities:0001";
const RD_ENTITY_ID_TEST: &str = "urn:nl-eid-gdi:1.0:RD:00000004000000149000:entities:9002";
const RD_ENTITY_ID_PREPRODUCTION: &str = "urn:nl-eid-gdi:1.0:RD:00000004000000149000:entities:9002";

// Kiesraad ServiceUUID registered with the TVS. The test value matches the
// standalone TVS mock's fixtures; pre-production and production share the same
// registered ServiceUUID (it does not change between those environments).
const DV_SERVICE_UUID_TEST: &str = "f847dc11-ac24-47b2-84a8-a057440ce56d";
const DV_SERVICE_UUID: &str = "4d475439-5847-4337-414c-50505a493141";

/// Deployment environment. This is the single environment-selecting input the
/// deployment provides; everything environment-specific (the TVS RD endpoints,
/// the Kiesraad DV EntityID and ServiceUUID, and the back-channel trust anchor)
/// derives from it (see the methods below). The remaining configuration is the
/// certificate directory, the public base URL, which AD to pre-select, and the
/// post-logout redirect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Environment {
    /// Local development and tests against the shared standalone TVS mock.
    #[default]
    Test,
    /// TVS pre-production (`pp2.toegang.overheid.nl`).
    Preproduction,
    /// TVS production (`rd2.toegang.overheid.nl`).
    Production,
}

impl Environment {
    /// Whether this is the production environment (affects AD EntityID selection
    /// and certificate-subject expectations).
    pub fn is_production(self) -> bool {
        matches!(self, Self::Production)
    }

    /// TVS RD (Routeringsdienst) base URL: the shared standalone TVS mock for
    /// `Test`, the real TVS RD otherwise.
    pub fn rd_base_url(self) -> &'static str {
        match self {
            Self::Test => TVS_TEST_BASE_URL,
            Self::Preproduction => TVS_PREPRODUCTION_BASE_URL,
            Self::Production => TVS_PRODUCTION_BASE_URL,
        }
    }

    /// The Kiesraad DV EntityID (SP issuer) for this environment.
    pub fn dv_entity_id(self) -> EntityId {
        EntityId::from_static(match self {
            // Test reuses the pre-production `9001` index (the standalone mock
            // does not enforce a distinct test EntityID).
            Self::Test | Self::Preproduction => DV_ENTITY_ID_PREPRODUCTION,
            Self::Production => DV_ENTITY_ID_PRODUCTION,
        })
    }

    /// The expected TVS RD (Routeringsdienst) EntityID for this environment.
    /// Used to pin the RD identity: the loaded metadata's `entityID` must equal
    /// this, and it is the expected `Issuer` for the Response and Assertion.
    pub fn rd_entity_id(self) -> EntityId {
        EntityId::from_static(match self {
            Self::Test => RD_ENTITY_ID_TEST,
            Self::Preproduction => RD_ENTITY_ID_PREPRODUCTION,
            Self::Production => RD_ENTITY_ID_PRODUCTION,
        })
    }

    /// The Kiesraad ServiceUUID registered with the TVS. Pre-production and
    /// production share the same value; only the local test mock differs.
    pub fn dv_service_uuid(self) -> ServiceUuid {
        ServiceUuid::from_static(match self {
            Self::Test => DV_SERVICE_UUID_TEST,
            Self::Preproduction | Self::Production => DV_SERVICE_UUID,
        })
    }
}

impl std::str::FromStr for Environment {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            // `local`/`development` accepted as aliases for dev setups.
            "test" | "local" | "development" => Ok(Self::Test),
            "preproduction" | "preprod" => Ok(Self::Preproduction),
            "production" | "prod" => Ok(Self::Production),
            other => Err(format!(
                "invalid environment {other:?}: expected one of test, preproduction, production"
            )),
        }
    }
}

// AD (Authenticatiedienst) EntityIDs emitted in the AuthnRequest
// `Scoping/IDPList/IDPEntry@ProviderID` (eID §7.3) to pre-select a single AD.
// EntityID format follows Koppelvlakspecificatie eID SAML v4.4 §10.2:
// `urn:nl-eid-gdi:1.0:<ROLE>:<OIN>:entities:<index>` for DigiD and the
// `urn:etoegang:...` scheme for eHerkenning/eIDAS. Production uses the `0xxx`
// index, pre-production (TVS pp2) the `9xxx` index. These are defined in code
// (not configured) and sourced from the TVS onboarding; the configurable part
// is only which AD to pre-select (see [`PreselectedAd`]).
const DIGID_AD_PRODUCTION: &str = "urn:nl-eid-gdi:1.0:AD:00000004166909913000:entities:0001";
const DIGID_AD_PREPRODUCTION: &str = "urn:nl-eid-gdi:1.0:AD:00000004166909913000:entities:9002";
const EHERKENNING_AD_PRODUCTION: &str = "urn:etoegang:HM:00000003520354760000:entities:0113";
const EHERKENNING_AD_PREPRODUCTION: &str = "urn:etoegang:HM:00000003520354760000:entities:9713";
const EIDAS_AD_PRODUCTION: &str = "urn:etoegang:EB:00000004000000149000:entities:0001";
const EIDAS_AD_PREPRODUCTION: &str = "urn:etoegang:EB:00000004000000149000:entities:9009";

/// Which Authenticatiedienst (AD) the DV pre-selects via the AuthnRequest
/// `Scoping/IDPList` (eID §7.3), or [`PreselectedAd::Select`] to send no
/// `Scoping` so the TVS presents its own AD selection screen.
///
/// The concrete EntityIDs are defined in code (see the `*_AD_*` constants) and
/// resolved per environment by [`PreselectedAd::entity_id`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreselectedAd {
    /// Send no `Scoping`; the user chooses an AD on the TVS selection screen.
    #[default]
    Select,
    DigiD,
    EHerkenning,
    Eidas,
}

impl PreselectedAd {
    /// The AD EntityID to emit in `Scoping/IDPList/IDPEntry@ProviderID`, or
    /// `None` for [`PreselectedAd::Select`] (no `Scoping` element). `Production`
    /// and pre-production resolve to different EntityIDs.
    pub fn entity_id(self, is_production: bool) -> Option<EntityId> {
        Some(EntityId::from_static(match (self, is_production) {
            (PreselectedAd::Select, _) => return None,
            (PreselectedAd::DigiD, true) => DIGID_AD_PRODUCTION,
            (PreselectedAd::DigiD, false) => DIGID_AD_PREPRODUCTION,
            (PreselectedAd::EHerkenning, true) => EHERKENNING_AD_PRODUCTION,
            (PreselectedAd::EHerkenning, false) => EHERKENNING_AD_PREPRODUCTION,
            (PreselectedAd::Eidas, true) => EIDAS_AD_PRODUCTION,
            (PreselectedAd::Eidas, false) => EIDAS_AD_PREPRODUCTION,
        }))
    }
}

impl std::str::FromStr for PreselectedAd {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "select" => Ok(Self::Select),
            "digid" => Ok(Self::DigiD),
            "eherkenning" => Ok(Self::EHerkenning),
            "eidas" => Ok(Self::Eidas),
            other => Err(format!(
                "invalid preselected AD {other:?}: expected one of Select, DigiD, eHerkenning, eIDAS"
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DvConfig {
    pub entity_id: EntityId,
    pub service_uuid: ServiceUuid,
    pub service_name: String,
    pub acs_url: EndpointUrl,
    pub slo_url: EndpointUrl,
    pub signing: Vec<KeyPaths>,
    pub encryption: Vec<KeyPaths>,
}

impl Default for DvConfig {
    /// The unconfigured DV of an [`AuthConfig::default`]: no identity, no
    /// endpoints, no keys. Every SAML flow against it fails; see
    /// [`AuthServiceState::new_empty`](crate::AuthServiceState::new_empty).
    fn default() -> Self {
        Self {
            entity_id: EntityId::unset(),
            service_uuid: ServiceUuid::unset(),
            service_name: String::new(),
            acs_url: EndpointUrl::unset(),
            slo_url: EndpointUrl::unset(),
            signing: Vec::new(),
            encryption: Vec::new(),
        }
    }
}

/// Bootstrap configuration for the IdP (RD). Only the metadata URL is held
/// here; entity_id, endpoints, and signing/encryption keys are fetched from
/// the metadata document at startup and live in `AuthServiceState`.
#[derive(Debug, Clone, Default)]
pub struct RdConfig {
    pub metadata_url: String,
}

#[derive(Debug, Clone, Default)]
pub struct TlsConfig {
    pub client_cert: PathBuf,
    pub client_key: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct AuthConfig {
    pub environment: Environment,
    pub certs_dir: PathBuf,
    pub tls: TlsConfig,
    pub dv: DvConfig,
    pub rd: RdConfig,
    /// Which AD to pre-select in the outgoing AuthnRequest `Scoping` (eID §7.3),
    /// or [`PreselectedAd::Select`] to send no `Scoping` and let the TVS show
    /// its AD picker. The concrete EntityID is defined in code, not configured.
    pub preselected_ad: PreselectedAd,
    /// Where the browser is sent once logout completes (after the SLO round-trip
    /// with the RD) or when `/logout` is hit without an active session. The
    /// embedding application sets this to its own logout-confirmation page;
    /// defaults to the application root.
    pub post_logout_redirect: RedirectTarget,
}

impl AuthConfig {
    /// The AD EntityID to emit in the AuthnRequest `Scoping`, resolved for this
    /// config's environment, or `None` for [`PreselectedAd::Select`].
    pub fn preselected_ad_entity_id(&self) -> Option<EntityId> {
        self.preselected_ad
            .entity_id(self.environment.is_production())
    }

    /// Override `certs_dir` and rebuild every cert/key path that derives from
    /// it, replacing any previously set paths. Use this when the fixture bundle
    /// is extracted to a runtime location (the `tvs-mock` feature) or in tests.
    pub fn with_certs_dir(mut self, certs_dir: PathBuf) -> Self {
        self.tls = tls_paths(&certs_dir);
        // Unlike `discover_key_paths`, every pair is included unconditionally:
        // the caller is asserting the bundle layout, not discovering it on disk.
        self.dv.signing = SIGNING_BASES
            .iter()
            .map(|base| key_pair_paths(&certs_dir, base))
            .collect();
        self.dv.encryption = ENCRYPTION_BASES
            .iter()
            .map(|base| key_pair_paths(&certs_dir, base))
            .collect();
        self.certs_dir = certs_dir;
        self
    }

    /// Build the [`AuthConfig`] from environment variables.
    ///
    /// The deployment provides three TVS inputs plus the public origin:
    ///
    /// - `TVS_ENV`: the [`Environment`] (`test` / `preproduction` / `production`),
    /// - `CERTS_DIR`: directory holding the DV certificate/key bundle,
    /// - `PRESELECTED_AD`: which AD to pre-select; unset or empty defaults to
    ///   [`PreselectedAd::Select`] (no `Scoping`, so the TVS shows its own AD
    ///   selection screen),
    /// - `BASE_URL`: the public origin (used to derive the SP ACS/SLO URLs).
    ///
    /// Everything else (the TVS RD endpoint, the Kiesraad DV EntityID /
    /// ServiceUUID, the back-channel trust anchor, and the individual cert/key
    /// paths) derives from those inputs (see [`Environment`]).
    ///
    /// With the `tvs-mock` feature enabled this targets the online (shared) TVS
    /// mock: `TVS_ENV` defaults to `test`, `BASE_URL` to `http://localhost:3000`,
    /// and the committed DV cert/key bundle is embedded into the binary and
    /// extracted at runtime, so the auth flow works on a host with no `CERTS_DIR`
    /// on disk. An explicit environment variable still overrides each default.
    /// Without the feature, `TVS_ENV`, `CERTS_DIR`, and `BASE_URL` are required.
    pub async fn from_env() -> Result<Self, AuthError> {
        Self::from_env_with(|name| env::var(name)).await
    }

    async fn from_env_with<F>(lookup: F) -> Result<Self, AuthError>
    where
        F: FnMut(&str) -> Result<String, env::VarError>,
    {
        let mut env = env_reader::EnvReader::new(lookup);
        let environment = env.environment()?;
        let certs_dir = env.certs_dir().await?;
        let preselected_ad = env.preselected_ad()?;
        let base_url = env.base_url(environment)?;
        Self::derive(environment, certs_dir, preselected_ad, &base_url).await
    }

    /// Everything else derives from `{environment, certs_dir, base_url}`.
    ///
    /// Fallible only through the derived endpoint URLs: a `BASE_URL` that is not
    /// an absolute http(s) origin is a deployment error, and failing here keeps
    /// the SP metadata and the ACS/SLO checks from ever seeing an unusable value.
    async fn derive(
        environment: Environment,
        certs_dir: PathBuf,
        preselected_ad: PreselectedAd,
        base_url: &str,
    ) -> Result<Self, AuthError> {
        let dv = DvConfig {
            entity_id: environment.dv_entity_id(),
            service_uuid: environment.dv_service_uuid(),
            service_name: DV_SERVICE_NAME.to_string(),
            // Derive the externally-advertised URLs from the same TypedPath the
            // router mounts, so the SP metadata and the live routes cannot drift.
            acs_url: EndpointUrl::from_base_url(
                format!("{base_url}{}", crate::SamlAcsPath::PATH),
                "ACS URL",
            )?,
            slo_url: EndpointUrl::from_base_url(
                format!("{base_url}{}", crate::SamlLogoutPath::PATH),
                "SLO URL",
            )?,
            signing: discover_key_paths(&certs_dir, SIGNING_BASES).await,
            encryption: discover_key_paths(&certs_dir, ENCRYPTION_BASES).await,
        };

        let rd = RdConfig {
            // `Test` points at the shared standalone TVS mock; real environments
            // at the TVS RD (see `Environment::rd_base_url`).
            metadata_url: format!("{}/kvs/rd/metadata", environment.rd_base_url()),
        };

        let tls = tls_paths(&certs_dir);

        Ok(AuthConfig {
            environment,
            certs_dir,
            tls,
            dv,
            rd,
            preselected_ad,
            // The embedding application overrides this with its own
            // logout-confirmation page after building the config.
            post_logout_redirect: RedirectTarget::root(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_emits_no_scoping() {
        assert_eq!(PreselectedAd::Select.entity_id(true), None);
        assert_eq!(PreselectedAd::Select.entity_id(false), None);
    }

    #[test]
    fn ad_entity_ids_match_tvs_onboarding() {
        // Exact EntityIDs from the TVS onboarding; guard against typos.
        assert_eq!(
            PreselectedAd::DigiD.entity_id(true),
            Some(EntityId::from_static(
                "urn:nl-eid-gdi:1.0:AD:00000004166909913000:entities:0001"
            ))
        );
        assert_eq!(
            PreselectedAd::DigiD.entity_id(false),
            Some(EntityId::from_static(
                "urn:nl-eid-gdi:1.0:AD:00000004166909913000:entities:9002"
            ))
        );
        assert_eq!(
            PreselectedAd::EHerkenning.entity_id(true),
            Some(EntityId::from_static(
                "urn:etoegang:HM:00000003520354760000:entities:0113"
            ))
        );
        assert_eq!(
            PreselectedAd::EHerkenning.entity_id(false),
            Some(EntityId::from_static(
                "urn:etoegang:HM:00000003520354760000:entities:9713"
            ))
        );
        assert_eq!(
            PreselectedAd::Eidas.entity_id(true),
            Some(EntityId::from_static(
                "urn:etoegang:EB:00000004000000149000:entities:0001"
            ))
        );
        assert_eq!(
            PreselectedAd::Eidas.entity_id(false),
            Some(EntityId::from_static(
                "urn:etoegang:EB:00000004000000149000:entities:9009"
            ))
        );
    }

    #[test]
    fn parses_known_values_case_insensitively() {
        assert_eq!("Select".parse(), Ok(PreselectedAd::Select));
        assert_eq!("DigiD".parse(), Ok(PreselectedAd::DigiD));
        assert_eq!("digid".parse(), Ok(PreselectedAd::DigiD));
        assert_eq!("eHerkenning".parse(), Ok(PreselectedAd::EHerkenning));
        assert_eq!("eIDAS".parse(), Ok(PreselectedAd::Eidas));
        assert!("nonsense".parse::<PreselectedAd>().is_err());
    }

    #[test]
    fn environment_parses_canonical_and_aliases() {
        assert_eq!("test".parse(), Ok(Environment::Test));
        assert_eq!("local".parse(), Ok(Environment::Test));
        assert_eq!("development".parse(), Ok(Environment::Test));
        assert_eq!("preproduction".parse(), Ok(Environment::Preproduction));
        assert_eq!("Production".parse(), Ok(Environment::Production));
        assert!("staging".parse::<Environment>().is_err());
    }

    #[test]
    fn environment_derives_per_env_properties() {
        assert!(!Environment::Test.is_production());
        assert!(!Environment::Preproduction.is_production());
        assert!(Environment::Production.is_production());

        assert_eq!(Environment::Test.rd_base_url(), TVS_TEST_BASE_URL);
        assert_eq!(
            Environment::Preproduction.rd_base_url(),
            TVS_PREPRODUCTION_BASE_URL
        );
        assert_eq!(
            Environment::Production.rd_base_url(),
            TVS_PRODUCTION_BASE_URL
        );
    }

    #[test]
    fn production_dv_entity_id_uses_its_own_index() {
        // Production has a distinct `0xxx` index (the Test/Preproduction branch
        // reuses `9001`); guard the production branch of `dv_entity_id`.
        assert_eq!(
            Environment::Production.dv_entity_id().as_str(),
            "urn:nl-eid-gdi:1.0:DV:00000004185618890000:entities:0001"
        );
    }

    #[test]
    fn rd_entity_id_is_pinned_per_environment() {
        // The expected RD Issuer is a pinned constant per environment, not taken
        // from the (only self-signature-checked) metadata.
        assert_eq!(
            Environment::Test.rd_entity_id().as_str(),
            "urn:nl-eid-gdi:1.0:RD:00000004000000149000:entities:9002"
        );
        assert_eq!(
            Environment::Preproduction.rd_entity_id().as_str(),
            "urn:nl-eid-gdi:1.0:RD:00000004000000149000:entities:9002"
        );
        assert_eq!(
            Environment::Production.rd_entity_id().as_str(),
            "urn:nl-eid-gdi:1.0:RD:00000004000000149000:entities:0001"
        );
    }

    #[test]
    fn preselected_ad_entity_id_resolves_for_config_environment() {
        let cfg = AuthConfig {
            environment: Environment::Production,
            preselected_ad: PreselectedAd::DigiD,
            ..AuthConfig::default()
        };
        assert_eq!(
            cfg.preselected_ad_entity_id(),
            Some(EntityId::from_static(DIGID_AD_PRODUCTION))
        );

        // Pre-production resolves to the 9xxx index; Select yields no Scoping.
        let cfg = AuthConfig {
            environment: Environment::Preproduction,
            preselected_ad: PreselectedAd::DigiD,
            ..AuthConfig::default()
        };
        assert_eq!(
            cfg.preselected_ad_entity_id(),
            Some(EntityId::from_static(DIGID_AD_PREPRODUCTION))
        );

        let cfg = AuthConfig::default();
        assert_eq!(cfg.preselected_ad_entity_id(), None);
    }

    #[test]
    fn post_logout_redirect_defaults_to_root() {
        // Unset (the embedding application has not overridden it) => "/".
        let cfg = AuthConfig::default();
        assert_eq!(cfg.post_logout_redirect.as_str(), "/");

        // Set => the configured page.
        let cfg = AuthConfig {
            post_logout_redirect: RedirectTarget::from_static("/logged-out"),
            ..AuthConfig::default()
        };
        assert_eq!(cfg.post_logout_redirect.as_str(), "/logged-out");
    }

    #[test]
    fn with_certs_dir_rebuilds_every_derived_path() {
        let dir = PathBuf::from("/opt/eks/certs");
        let cfg = AuthConfig::default().with_certs_dir(dir.clone());

        assert_eq!(cfg.certs_dir, dir);
        assert_eq!(cfg.tls.client_cert, dir.join("dv-tls.pem"));
        assert_eq!(cfg.tls.client_key, dir.join("dv-tls-key.pem"));

        // Both signing and encryption families get their `-1`/`-2` pairs rebuilt
        // under the new directory.
        assert_eq!(cfg.dv.signing.len(), 2);
        assert_eq!(cfg.dv.signing[0].cert, dir.join("dv-signing-1.pem"));
        assert_eq!(cfg.dv.signing[0].key, dir.join("dv-signing-1-key.pem"));
        assert_eq!(cfg.dv.signing[1].cert, dir.join("dv-signing-2.pem"));

        assert_eq!(cfg.dv.encryption.len(), 2);
        assert_eq!(cfg.dv.encryption[0].cert, dir.join("dv-encryption-1.pem"));
        assert_eq!(cfg.dv.encryption[1].cert, dir.join("dv-encryption-2.pem"));
    }

    #[test]
    fn dv_identity_matches_tvs_onboarding() {
        // Test reuses the pre-production EntityID; production has its own index.
        assert_eq!(
            Environment::Test.dv_entity_id().as_str(),
            "urn:nl-eid-gdi:1.0:DV:00000004185618890000:entities:9001"
        );
        assert_eq!(
            Environment::Preproduction.dv_entity_id().as_str(),
            "urn:nl-eid-gdi:1.0:DV:00000004185618890000:entities:9001"
        );
        assert_eq!(
            Environment::Test.dv_service_uuid().as_str(),
            "f847dc11-ac24-47b2-84a8-a057440ce56d"
        );
        // Pre-production and production share the same registered ServiceUUID.
        assert_eq!(
            Environment::Preproduction.dv_service_uuid().as_str(),
            "4d475439-5847-4337-414c-50505a493141"
        );
        assert_eq!(
            Environment::Production.dv_service_uuid(),
            Environment::Preproduction.dv_service_uuid()
        );
    }

    #[tokio::test]
    async fn first_key_mandatory_second_omitted_when_cert_absent() {
        // A certs dir with no key files at all: the first pair is always
        // present, the absent second pair is dropped.
        let dir = PathBuf::from("/eks-key-paths-test/does-not-exist");
        let paths = discover_key_paths(&dir, SIGNING_BASES).await;
        assert_eq!(paths.len(), 1, "the first key pair is mandatory");
        assert_eq!(paths[0].cert, dir.join("dv-signing-1.pem"));
        assert_eq!(paths[0].key, dir.join("dv-signing-1-key.pem"));
    }

    #[tokio::test]
    async fn second_key_included_when_cert_file_exists() {
        let dir = env::temp_dir().join(format!(
            "eks-key-paths-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("dv-signing-2.pem"), b"cert").unwrap();

        let paths = discover_key_paths(&dir, SIGNING_BASES).await;
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(
            paths.len(),
            2,
            "second key pair discovered via its default cert file"
        );
        assert_eq!(paths[1].cert, dir.join("dv-signing-2.pem"));
        assert_eq!(paths[1].key, dir.join("dv-signing-2-key.pem"));
    }

    /// Build an env lookup closure backed by an explicit set of overrides.
    fn lookup_from(
        pairs: &[(&'static str, &'static str)],
    ) -> impl FnMut(&str) -> Result<String, env::VarError> {
        let map: std::collections::HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |name: &str| map.get(name).cloned().ok_or(env::VarError::NotPresent)
    }

    // Real-environment derivation: only valid without `tvs-mock` (which rejects a
    // real TVS_ENV).
    #[cfg(not(feature = "tvs-mock"))]
    #[tokio::test]
    async fn preproduction_derives_dv_identity_and_rd_endpoint() {
        let cfg = AuthConfig::from_env_with(lookup_from(&[
            ("TVS_ENV", "preproduction"),
            ("CERTS_DIR", "/tmp/certs"),
            ("PRESELECTED_AD", "DigiD"),
            ("BASE_URL", "https://preview.kandidaatstellen.nl"),
        ]))
        .await
        .unwrap();

        assert_eq!(cfg.environment, Environment::Preproduction);
        assert_eq!(
            cfg.dv.entity_id.as_str(),
            "urn:nl-eid-gdi:1.0:DV:00000004185618890000:entities:9001"
        );
        assert_eq!(
            cfg.dv.service_uuid.as_str(),
            "4d475439-5847-4337-414c-50505a493141"
        );
        assert_eq!(cfg.dv.service_name, "Kiesraad");
        assert_eq!(
            cfg.dv.acs_url.as_str(),
            "https://preview.kandidaatstellen.nl/saml/sp/acs"
        );
        assert_eq!(
            cfg.rd.metadata_url,
            "https://pp2.toegang.overheid.nl/kvs/rd/metadata"
        );
        assert_eq!(cfg.tls.client_cert, PathBuf::from("/tmp/certs/dv-tls.pem"));
        assert_eq!(cfg.preselected_ad, PreselectedAd::DigiD);
    }

    #[cfg(not(feature = "tvs-mock"))]
    #[tokio::test]
    async fn preselected_ad_defaults_to_select_when_unset_or_empty() {
        // Unset: no PRESELECTED_AD entry at all.
        let cfg = AuthConfig::from_env_with(lookup_from(&[
            ("TVS_ENV", "preproduction"),
            ("CERTS_DIR", "/tmp/certs"),
            ("BASE_URL", "https://preview.kandidaatstellen.nl"),
        ]))
        .await
        .unwrap();
        assert_eq!(cfg.preselected_ad, PreselectedAd::Select);

        // Present but empty/whitespace: still falls back to Select.
        let cfg = AuthConfig::from_env_with(lookup_from(&[
            ("TVS_ENV", "preproduction"),
            ("CERTS_DIR", "/tmp/certs"),
            ("PRESELECTED_AD", "  "),
            ("BASE_URL", "https://preview.kandidaatstellen.nl"),
        ]))
        .await
        .unwrap();
        assert_eq!(cfg.preselected_ad, PreselectedAd::Select);
    }

    #[tokio::test]
    async fn invalid_environment_is_an_error() {
        let err = AuthConfig::from_env_with(lookup_from(&[
            ("TVS_ENV", "staging"),
            ("CERTS_DIR", "/tmp/certs"),
            ("PRESELECTED_AD", "DigiD"),
            ("BASE_URL", "https://example.test"),
        ]))
        .await
        .unwrap_err();
        assert!(matches!(err, AuthError::Config(_)));
    }

    // tvs-mock must refuse a real TVS_ENV (it pins the test CA / keys).
    #[cfg(feature = "tvs-mock")]
    #[tokio::test]
    async fn tvs_mock_refuses_real_environment() {
        for env in ["preproduction", "production"] {
            let err = AuthConfig::from_env_with(lookup_from(&[("TVS_ENV", env)]))
                .await
                .unwrap_err();
            assert!(
                matches!(&err, AuthError::Config(m) if m.contains("tvs-mock")),
                "env={env}: {err:?}"
            );
        }
    }

    // With `tvs-mock` on, TVS_ENV and BASE_URL fall back to their embedded
    // defaults and the DV bundle is materialized from the embedded fixtures, so
    // the config builds with no environment variables set at all.
    #[cfg(feature = "tvs-mock")]
    #[tokio::test]
    async fn tvs_mock_builds_from_embedded_defaults_with_no_env() {
        let cfg = AuthConfig::from_env_with(lookup_from(&[])).await.unwrap();

        // TVS_ENV defaults to `test`; every derived value follows from it.
        assert_eq!(cfg.environment, Environment::Test);
        // BASE_URL defaults to the local dev origin.
        assert_eq!(cfg.dv.acs_url.as_str(), "http://localhost:3000/saml/sp/acs");
        assert_eq!(
            cfg.dv.slo_url.as_str(),
            "http://localhost:3000/saml/sp/logout"
        );
        assert_eq!(
            cfg.rd.metadata_url,
            format!("{TVS_TEST_BASE_URL}/kvs/rd/metadata")
        );
        // CERTS_DIR was absent, so the embedded bundle was extracted to a real
        // directory whose cert/key files exist.
        assert!(cfg.certs_dir.join("dv-signing-1.pem").exists());
        assert!(cfg.tls.client_cert.exists());
        // PRESELECTED_AD unset => Select (no Scoping).
        assert_eq!(cfg.preselected_ad, PreselectedAd::Select);
    }

    // A CERTS_DIR holding a real bundle, for the tvs-mock override tests.
    #[cfg(feature = "tvs-mock")]
    fn bundle_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eks-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for base in ["dv-tls", "dv-signing-1", "dv-encryption-1"] {
            std::fs::write(dir.join(format!("{base}.pem")), b"cert").unwrap();
            std::fs::write(dir.join(format!("{base}-key.pem")), b"key").unwrap();
        }
        dir
    }

    // An explicit CERTS_DIR / PRESELECTED_AD still override the tvs-mock
    // defaults (exercises the env-var branches rather than the embedded ones).
    #[cfg(feature = "tvs-mock")]
    #[tokio::test]
    async fn tvs_mock_honors_explicit_certs_dir_and_preselected_ad() {
        let dir = bundle_dir("explicit-certs");
        let certs_dir = dir.to_string_lossy().into_owned();
        let cfg = AuthConfig::from_env_with(move |name: &str| match name {
            "TVS_ENV" => Ok("test".to_string()),
            "CERTS_DIR" => Ok(certs_dir.clone()),
            "PRESELECTED_AD" => Ok("DigiD".to_string()),
            "BASE_URL" => Ok("http://localhost:3000".to_string()),
            _ => Err(env::VarError::NotPresent),
        })
        .await
        .unwrap();

        assert_eq!(cfg.certs_dir, dir);
        assert_eq!(cfg.tls.client_cert, dir.join("dv-tls.pem"));
        assert_eq!(cfg.preselected_ad, PreselectedAd::DigiD);
    }

    // A CERTS_DIR left over from an older layout points at a directory with no
    // bundle in it; a mock build carries its own keys, so it falls back to the
    // embedded bundle rather than failing to start.
    #[cfg(feature = "tvs-mock")]
    #[tokio::test]
    async fn tvs_mock_falls_back_when_certs_dir_holds_no_bundle() {
        let stale = "src/fixtures/tvs";
        let cfg = AuthConfig::from_env_with(lookup_from(&[
            ("TVS_ENV", "test"),
            ("CERTS_DIR", stale),
            ("BASE_URL", "http://localhost:3000"),
        ]))
        .await
        .unwrap();

        assert_ne!(cfg.certs_dir, PathBuf::from(stale));
        assert!(cfg.certs_dir.join("dv-signing-1.pem").exists());
        assert!(cfg.tls.client_cert.exists());
    }

    // A real environment must reject a non-https BASE_URL (cookie downgrade).
    #[cfg(not(feature = "tvs-mock"))]
    #[tokio::test]
    async fn non_https_base_url_rejected_for_real_environment() {
        let err = AuthConfig::from_env_with(lookup_from(&[
            ("TVS_ENV", "preproduction"),
            ("CERTS_DIR", "/tmp/certs"),
            ("BASE_URL", "http://preview.kandidaatstellen.nl"),
        ]))
        .await
        .unwrap_err();
        assert!(
            matches!(&err, AuthError::Config(m) if m.contains("BASE_URL must be https")),
            "{err:?}"
        );
    }
}
