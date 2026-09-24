//! The SAML identifiers and URLs this crate passes around.
//!
//! Every one of these is text on the wire, and most of them sit next to each
//! other in the same argument list: a builder taking `(&str, &str, &str, &str)`
//! for an EntityID, a ServiceUUID, a destination URL and a NameID accepts any
//! permutation of them. They are separate types so it does not, and so the
//! value's own rules (a URL is absolute and free of characters that break out of
//! an HTML attribute; a message ID is an XML `NCName`) are checked once, where
//! the value enters the program, instead of being re-asserted at each use.
//!
//! Two kinds of constructor recur:
//!
//! - `from_static`, `const`, for the values pinned in [`crate::config`]. They are
//!   compile-time constants sourced from the TVS onboarding, not input; the tests
//!   at the bottom of this module check that each one would also pass `parse`.
//! - `parse`, fallible, for everything that arrives from outside: RD metadata, a
//!   query parameter, an assertion, the deployment's environment.

use std::{borrow::Cow, fmt};

use crate::error::{AuthError, Result};

/// Characters that must never reach an interpolation site.
///
/// A metadata-derived URL is interpolated into an HTML attribute
/// ([`crate::bindings::http_post::create_post_form`]), a Content-Security-Policy
/// header ([`crate::bindings::http_post::autosubmit_csp`]) and an HTTP request
/// target ([`crate::bindings::soap::send_soap_request`]). Rejecting these means
/// it cannot break out of an attribute, inject a CSP directive, or smuggle a
/// request. A well-formed URL never contains them unescaped.
const URL_ILLEGAL_CHARS: &str = "\"'<>`;\\";

/// SAML 2.0 metadata caps an `entityID` at 1024 characters; the same bound is a
/// sane ceiling for the other identifiers, none of which is more than a URI.
const MAX_IDENTIFIER_LEN: usize = 1024;

/// Reject text that is empty, over-long, or carries whitespace or control
/// characters. Shared by the identifier types, whose values are all single-token
/// URIs or URNs.
fn check_token(what: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(AuthError::Config(format!("{what} is empty")));
    }
    if value.len() > MAX_IDENTIFIER_LEN {
        return Err(AuthError::Config(format!(
            "{what} is longer than {MAX_IDENTIFIER_LEN} characters"
        )));
    }
    if let Some(bad) = value
        .chars()
        .find(|c| c.is_whitespace() || c.is_control() || URL_ILLEGAL_CHARS.contains(*c))
    {
        return Err(AuthError::Config(format!(
            "{what} contains an illegal character {bad:?}: {value}"
        )));
    }
    Ok(())
}

/// A SAML `EntityID` (eID §10.2): the identity of the DV, the RD, or an AD.
///
/// The DV and RD EntityIDs are pinned constants (see [`crate::config`]); the one
/// in a fetched metadata document is parsed and then required to equal the
/// pinned value, so the document can never introduce an identity of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityId(Cow<'static, str>);

impl EntityId {
    /// A pinned EntityID from [`crate::config`].
    pub const fn from_static(entity_id: &'static str) -> Self {
        Self(Cow::Borrowed(entity_id))
    }

    /// The "not configured" EntityID of a state built without a deployment
    /// configuration (see [`EndpointUrl::unset`]).
    pub const fn unset() -> Self {
        Self(Cow::Borrowed(""))
    }

    /// An EntityID read from a metadata document or another external source.
    pub fn parse(entity_id: impl Into<String>) -> Result<Self> {
        let entity_id = entity_id.into();
        check_token("EntityID", &entity_id)?;
        Ok(Self(Cow::Owned(entity_id)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq<str> for EntityId {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

/// The `ServiceUUID` the DV is registered under with the TVS (eID §7.3.1.1,
/// §7.6.3.4): sent in the AuthnRequest and the SP metadata, and required to come
/// back unchanged in the assertion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceUuid(Cow<'static, str>);

impl ServiceUuid {
    /// The pinned ServiceUUID for an environment (see [`crate::config`]).
    pub const fn from_static(uuid: &'static str) -> Self {
        Self(Cow::Borrowed(uuid))
    }

    /// The "not configured" ServiceUUID of a state built without a deployment
    /// configuration (see [`EndpointUrl::unset`]).
    pub const fn unset() -> Self {
        Self(Cow::Borrowed(""))
    }

    /// A ServiceUUID from outside; must be a UUID in the canonical hyphenated
    /// form the TVS registration uses.
    pub fn parse(uuid: &str) -> Result<Self> {
        uuid::Uuid::try_parse(uuid)
            .map_err(|e| AuthError::Config(format!("ServiceUUID {uuid:?} is not a UUID: {e}")))?;
        Ok(Self(Cow::Owned(uuid.to_ascii_lowercase())))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ServiceUuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An absolute endpoint URL: an RD endpoint from verified metadata, or one of
/// the DV's own endpoints derived from `BASE_URL`.
///
/// Constructing one is the *only* validation these URLs get, so a value of this
/// type is safe to interpolate into an HTML attribute, a CSP header, or an HTTP
/// request target: it carries no quote, angle bracket, backtick, semicolon,
/// backslash, whitespace, or control character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointUrl(String);

impl EndpointUrl {
    /// An RD endpoint `Location` from metadata. eID §9.4 requires TLS on all
    /// channels, so anything but `https` is refused; `what` names the endpoint
    /// in the error.
    pub fn from_metadata(url: &str, what: &str) -> Result<Self> {
        let Some(host) = url.strip_prefix("https://") else {
            return Err(AuthError::Config(format!(
                "metadata {what} endpoint is not an https URL: {url}"
            )));
        };
        if host.is_empty() {
            return Err(AuthError::Config(format!(
                "metadata {what} endpoint has no host: {url}"
            )));
        }
        check_token(&format!("metadata {what} endpoint"), url)?;
        Ok(Self(url.to_owned()))
    }

    /// Require an RD endpoint's host to be `domain` or a subdomain of it, so a
    /// metadata document cannot point an endpoint at an arbitrary host (see
    /// [`crate::config::RD_ENDPOINT_DOMAIN`]). `what` names the endpoint in the
    /// error.
    ///
    /// The host is compared as DNS labels, case-insensitively, after stripping
    /// an optional numeric port: `evil-toegang.overheid.nl` and
    /// `toegang.overheid.nl.evil.example` do not match `toegang.overheid.nl`. A
    /// user-info part, an IP literal, or an empty label is rejected outright.
    pub fn require_domain(&self, domain: &str, what: &str) -> Result<()> {
        let host = self.host().ok_or_else(|| {
            AuthError::Config(format!(
                "metadata {what} endpoint has no plain DNS host name: {}",
                self.0
            ))
        })?;
        let host = host.to_ascii_lowercase();
        let domain = domain.to_ascii_lowercase();
        let in_domain = host == domain
            || host
                .strip_suffix(&domain)
                .is_some_and(|prefix| prefix.ends_with('.'));
        if !in_domain {
            return Err(AuthError::Config(format!(
                "metadata {what} endpoint host {host} is not under the pinned RD domain {domain}: {}",
                self.0
            )));
        }
        Ok(())
    }

    /// The DNS host name of this URL, if it is one: the authority without a
    /// user-info part or IP literal, with a trailing numeric port stripped, made
    /// of non-empty labels of letters, digits and hyphens.
    fn host(&self) -> Option<&str> {
        let rest = self
            .0
            .strip_prefix("https://")
            .or_else(|| self.0.strip_prefix("http://"))?;
        let authority = rest.split(['/', '?', '#']).next()?;
        if authority.contains('@') {
            return None;
        }
        let host = match authority.rsplit_once(':') {
            Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
                host
            }
            Some(_) => return None,
            None => authority,
        };
        let is_label = |label: &str| {
            !label.is_empty() && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        };
        let is_dns_name = host.split('.').all(is_label)
            // An all-numeric final label is an IPv4 address, not a domain name.
            && !host.rsplit('.').next()?.bytes().all(|b| b.is_ascii_digit());
        is_dns_name.then_some(host)
    }

    /// One of the DV's own endpoints, derived from the configured `BASE_URL`.
    /// Plain `http` is accepted here (and only here) because local development
    /// runs the SP on `http://localhost`; see [`Self::is_https`], which decides
    /// whether the flow cookie may be `Secure`.
    pub fn from_base_url(url: impl Into<String>, what: &str) -> Result<Self> {
        let url = url.into();
        let Some(rest) = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
        else {
            return Err(AuthError::Config(format!(
                "{what} is not an absolute http(s) URL: {url}"
            )));
        };
        if rest.is_empty() {
            return Err(AuthError::Config(format!("{what} has no host: {url}")));
        }
        check_token(what, &url)?;
        Ok(Self(url))
    }

    /// The "not configured" URL of a state built without a deployment
    /// configuration ([`crate::AuthServiceState::new_empty`], and tests). No SAML
    /// flow can run against it; it exists so [`AuthConfig`](crate::config::AuthConfig)
    /// can still have a `Default`.
    pub fn unset() -> Self {
        Self(String::new())
    }

    /// Whether the SP is reached over https, and so whether its cookies may be
    /// `Secure` + `__Host-` prefixed (see [`crate::handlers`]).
    pub fn is_https(&self) -> bool {
        self.0.starts_with("https://")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for EndpointUrl {
    fn default() -> Self {
        Self::unset()
    }
}

impl fmt::Display for EndpointUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where the browser is sent after a flow completes: an absolute path on this
/// application, never an absolute or protocol-relative URL.
///
/// Both rules are load-bearing. A value that is not representable in a
/// `Location` header **panics** [`axum::response::Redirect`], and a `//host` (or
/// `/\host`) path is a protocol-relative URL: it sends the browser off this
/// origin while looking like a local path, which is an open redirect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedirectTarget(Cow<'static, str>);

impl RedirectTarget {
    /// The application root, used when no post-logout page is configured.
    pub const fn root() -> Self {
        Self(Cow::Borrowed("/"))
    }

    /// A path known at compile time, typically an
    /// [`axum_extra::routing::TypedPath`]'s `PATH`.
    pub const fn from_static(path: &'static str) -> Self {
        Self(Cow::Borrowed(path))
    }

    /// A path built at runtime.
    pub fn parse(path: impl Into<String>) -> Result<Self> {
        let path = path.into();
        check_token("redirect target", &path)?;
        if !path.starts_with('/') {
            return Err(AuthError::Config(format!(
                "redirect target is not an absolute path: {path}"
            )));
        }
        // `//host` and `/\host` both leave this origin; browsers accept either
        // as protocol-relative.
        if path.starts_with("//") || path.starts_with("/\\") {
            return Err(AuthError::Config(format!(
                "redirect target leaves this origin: {path}"
            )));
        }
        Ok(Self(Cow::Owned(path)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for RedirectTarget {
    fn default() -> Self {
        Self::root()
    }
}

impl fmt::Display for RedirectTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The `@ID` of a SAML message this DV issued, and the `@InResponseTo` that
/// names it on the way back.
///
/// Constrained to an XML `NCName`, which is what the SAML schema declares
/// (`xsd:ID`). That is not cosmetic: an incoming `InResponseTo` becomes a
/// pending-request store key and part of the flow cookie's value, and an
/// `NCName` cannot carry the `;` or `,` that would let it break the cookie.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MessageId(String);

/// How much of a message ID is logged: enough to correlate two log lines,
/// short enough not to fill them.
const ID_LOG_PREFIX_CHARS: usize = 20;

impl MessageId {
    /// A fresh random ID for an outgoing message. Underscore-prefixed, so it is
    /// an `NCName` even though it starts with a digit-capable UUID.
    pub fn generate() -> Self {
        Self(format!("_{}", uuid::Uuid::new_v4().simple()))
    }

    /// An ID that arrived from outside: an `InResponseTo`, or one read back from
    /// the pending-request store.
    pub fn parse(id: impl Into<String>) -> Result<Self> {
        let id = id.into();
        check_token("message ID", &id)?;
        let is_ncname_start = |c: char| c.is_ascii_alphabetic() || c == '_';
        let is_ncname_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.');
        if !id.starts_with(is_ncname_start) || !id.chars().all(is_ncname_char) {
            return Err(AuthError::Xml(format!(
                "message ID is not an XML NCName: {id:?}"
            )));
        }
        Ok(Self(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }

    /// A short prefix for correlating log lines. Character-wise, so it can never
    /// split a multi-byte character the way a byte slice would.
    pub fn log_prefix(&self) -> String {
        self.0.chars().take(ID_LOG_PREFIX_CHARS).collect()
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The Subject `NameID` of an assertion (eID §7.6.3): a TransientID naming the
/// authenticated session at the RD.
///
/// SECURITY: linkable to a specific authentication session, so it is never
/// logged. The embedding application persists it to build the later
/// `LogoutRequest` (eID §7.7.1).
#[derive(Clone, PartialEq, Eq)]
pub struct NameId(String);

impl NameId {
    /// Parse a Subject NameID.
    ///
    /// Bounded and non-empty, but deliberately *not* held to
    /// the other identifiers' character rules: the RD picks this value's format, and
    /// the only places it goes (an askama-escaped `<saml:NameID>` and the
    /// application's session record) are safe for any text. What must be refused
    /// is an empty one, which would put an empty `<NameID>` in a LogoutRequest,
    /// an unbounded one, and control characters.
    pub fn parse(name_id: impl Into<String>) -> Result<Self> {
        let name_id = name_id.into();
        let trimmed = name_id.trim();
        if trimmed.is_empty() {
            return Err(AuthError::Xml("NameID is empty".to_string()));
        }
        if trimmed.len() > MAX_IDENTIFIER_LEN {
            return Err(AuthError::Xml(format!(
                "NameID is longer than {MAX_IDENTIFIER_LEN} characters"
            )));
        }
        if trimmed.chars().any(char::is_control) {
            return Err(AuthError::Xml(
                "NameID contains a control character".to_string(),
            ));
        }
        Ok(Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Debug for NameId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NameId(<{} chars>)", self.0.len())
    }
}

impl fmt::Display for NameId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A SAML artifact (`SAMLart`, eID §7.4): the one-time reference the RD hands
/// the browser, which the DV exchanges for the assertion over the back-channel.
///
/// Opaque and single-use, so a short prefix is safe to log for correlation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact(String);

/// A SAML type 0x0004 artifact is 44 bytes, i.e. 60 base64 characters. The cap
/// is generous: it only has to stop an unbounded query parameter from being
/// signed into an ArtifactResolve.
const MAX_ARTIFACT_LEN: usize = 512;

impl Artifact {
    /// Parse the `SAMLart` query parameter. Base64, so anything outside the
    /// base64 alphabet is refused before the value is signed into an
    /// ArtifactResolve and sent to the RD.
    pub fn parse(artifact: &str) -> Result<Self> {
        if artifact.is_empty() || artifact.len() > MAX_ARTIFACT_LEN {
            return Err(AuthError::Config(format!(
                "SAMLart is empty or longer than {MAX_ARTIFACT_LEN} characters"
            )));
        }
        // Both base64 alphabets: SAML specifies the standard one, but accepting
        // the URL-safe spelling costs nothing (neither carries a character that
        // matters to anything downstream) and cannot surprise a live flow.
        if !artifact
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '-' | '_'))
        {
            return Err(AuthError::Config("SAMLart is not base64 text".to_string()));
        }
        Ok(Self(artifact.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A short prefix for correlating log lines.
    pub fn log_prefix(&self) -> String {
        self.0.chars().take(ID_LOG_PREFIX_CHARS).collect()
    }
}

impl fmt::Display for Artifact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_id_rejects_injection_shaped_values() {
        for bad in ["", " ", "urn:with space", "urn:x\"y", "urn:x<y", "urn:x;y"] {
            assert!(EntityId::parse(bad).is_err(), "{bad:?} must be rejected");
        }
        assert!(EntityId::parse("a".repeat(MAX_IDENTIFIER_LEN + 1)).is_err());
        assert_eq!(
            EntityId::parse("urn:nl-eid-gdi:1.0:DV:x:entities:0001")
                .unwrap()
                .as_str(),
            "urn:nl-eid-gdi:1.0:DV:x:entities:0001"
        );
    }

    #[test]
    fn service_uuid_requires_a_uuid() {
        assert!(ServiceUuid::parse("not-a-uuid").is_err());
        assert_eq!(
            ServiceUuid::parse("F847DC11-AC24-47B2-84A8-A057440CE56D")
                .unwrap()
                .as_str(),
            "f847dc11-ac24-47b2-84a8-a057440ce56d",
            "normalised to the lowercase form the TVS registration uses"
        );
    }

    #[test]
    fn metadata_endpoint_must_be_https() {
        // eID §9.4: TLS on every channel.
        assert!(EndpointUrl::from_metadata("http://rd.test/sso", "SSO").is_err());
        assert!(EndpointUrl::from_metadata("https://", "SSO").is_err());
        assert!(EndpointUrl::from_metadata("https://rd.test/sso", "SSO").is_ok());
    }

    #[test]
    fn metadata_endpoint_domain_pin_accepts_the_domain_and_its_subdomains() {
        for url in [
            "https://toegang.overheid.nl/kvs/rd/metadata",
            "https://artifact-pp2.toegang.overheid.nl/kvs/rd/resolve_artifact",
            "https://a.b.toegang.overheid.nl/",
            "https://Artifact-RD2.Toegang.Overheid.NL/x",
            "https://artifact-pp2.toegang.overheid.nl:443/x",
            "https://artifact-pp2.toegang.overheid.nl",
            "https://artifact-pp2.toegang.overheid.nl?x=1",
        ] {
            let url = EndpointUrl::from_metadata(url, "ARS").unwrap();
            assert!(
                url.require_domain("toegang.overheid.nl", "ARS").is_ok(),
                "{url} must be accepted"
            );
        }
    }

    #[test]
    fn metadata_endpoint_domain_pin_rejects_other_hosts() {
        for url in [
            // Other domains, including look-alikes around the pinned one.
            "https://evil.example/kvs/rd/resolve_artifact",
            "https://evil-toegang.overheid.nl/x",
            "https://toegang.overheid.nl.evil.example/x",
            "https://xtoegang.overheid.nl/x",
            "https://overheid.nl/x",
            // A user-info part: the real host is the part after `@`.
            "https://toegang.overheid.nl@evil.example/x",
            "https://toegang.overheid.nl:443@evil.example/x",
            // Not DNS names at all.
            "https://127.0.0.1/x",
            "https://[::1]/x",
            "https://.toegang.overheid.nl/x",
            "https://a..toegang.overheid.nl/x",
            "https://toegang.overheid.nl:port/x",
            "https://toegang.overheid.nl:/x",
            "https://toegang.overheid.nl./x",
        ] {
            let url = EndpointUrl::from_metadata(url, "ARS").unwrap();
            assert!(
                url.require_domain("toegang.overheid.nl", "ARS").is_err(),
                "{url} must be rejected"
            );
        }
    }

    #[test]
    fn metadata_endpoint_rejects_interpolation_breakouts() {
        // A poisoned Location must not be able to escape an HTML attribute, add
        // a CSP directive, or smuggle an HTTP request target.
        for bad in [
            "https://rd.test/\" onload=\"x",
            "https://rd.test/a;script-src *",
            "https://rd.test/a b",
            "https://rd.test/a\nb",
            "https://rd.test/<script>",
        ] {
            assert!(
                EndpointUrl::from_metadata(bad, "SSO").is_err(),
                "{bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn base_url_endpoint_allows_http_for_local_development() {
        let dev = EndpointUrl::from_base_url("http://localhost:3000/saml/sp/acs", "ACS").unwrap();
        assert!(!dev.is_https());
        let prod = EndpointUrl::from_base_url("https://eks.test/saml/sp/acs", "ACS").unwrap();
        assert!(prod.is_https());
        assert!(EndpointUrl::from_base_url("/saml/sp/acs", "ACS").is_err());
        assert!(!EndpointUrl::unset().is_https());
    }

    #[test]
    fn redirect_target_must_stay_on_this_origin() {
        assert_eq!(RedirectTarget::root().as_str(), "/");
        assert!(RedirectTarget::parse("/logged-out").is_ok());
        assert!(RedirectTarget::parse("/a/b?c=d").is_ok());
        for bad in [
            "",
            "logged-out",            // not absolute
            "https://evil.example/", // absolute URL
            "//evil.example/",       // protocol-relative
            "/\\evil.example/",      // protocol-relative, backslash form
            "/logged out",           // not header-safe
            "/logged\nout",          // not header-safe
        ] {
            assert!(
                RedirectTarget::parse(bad).is_err(),
                "{bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn message_id_must_be_an_ncname() {
        assert!(MessageId::parse("_abc123").is_ok());
        assert!(MessageId::parse("id-1.2").is_ok());
        for bad in ["", "1leading-digit", "has space", "semi;colon", "com,ma"] {
            assert!(MessageId::parse(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn generated_message_id_round_trips_through_parse() {
        let id = MessageId::generate();
        assert_eq!(MessageId::parse(id.as_str()).unwrap(), id);
    }

    #[test]
    fn message_id_log_prefix_is_bounded_and_char_safe() {
        let id = MessageId::generate();
        assert_eq!(id.log_prefix().chars().count(), ID_LOG_PREFIX_CHARS);
        // A short ID is logged whole rather than panicking on a byte slice.
        assert_eq!(MessageId::parse("_ab").unwrap().log_prefix(), "_ab");
    }

    #[test]
    fn name_id_is_never_revealed_by_debug() {
        let name_id = NameId::parse("urn:transient:abc").unwrap();
        assert!(!format!("{name_id:?}").contains("abc"), "{name_id:?}");
    }

    #[test]
    fn name_id_is_bounded_but_accepts_whatever_format_the_rd_picks() {
        // An empty NameID would go into a LogoutRequest as an empty element.
        assert!(NameId::parse("").is_err());
        assert!(NameId::parse("   ").is_err());
        assert!(NameId::parse("a".repeat(MAX_IDENTIFIER_LEN + 1)).is_err());
        assert!(NameId::parse("id\u{0}with-nul").is_err());
        // The RD owns the format; anything else printable round-trips, trimmed.
        assert_eq!(NameId::parse("  _abc+/=  ").unwrap().as_str(), "_abc+/=");
    }

    #[test]
    fn artifact_must_be_bounded_base64() {
        assert!(Artifact::parse("AAQAAB/example+base64==").is_ok());
        assert!(
            Artifact::parse("AAQAAB_example-base64==").is_ok(),
            "url-safe alphabet"
        );
        assert!(Artifact::parse("").is_err());
        assert!(Artifact::parse("has space").is_err());
        assert!(Artifact::parse("<script>").is_err());
        assert!(Artifact::parse(&"A".repeat(MAX_ARTIFACT_LEN + 1)).is_err());
    }

    #[test]
    fn artifact_log_prefix_is_bounded() {
        let artifact = Artifact::parse(&"A".repeat(60)).unwrap();
        assert_eq!(artifact.log_prefix().len(), ID_LOG_PREFIX_CHARS);
    }
}
