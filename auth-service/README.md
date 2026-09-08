# auth-service

SAML 2.0 **Service Provider** for the Dutch TVS *Routeringsdienst* (eID login via
DigiD / eHerkenning / eIDAS).

This crate implements the **DV (Dienstverlener / Service Provider)** side of the
*Koppelvlakspecificatie eID SAML v4.4* interface, talking to the TVS RD
(*Routeringsdienst*, the IdP). It is consumed as a library: an embedding
application mounts the [`router`](src/lib.rs) and implements the
[`AuthState`](src/state.rs) trait; this crate owns all SAML message building,
cryptography, and validation.

A non-authoritative extract of the requirements this code targets lives in
[eid-saml-4.4-requirements.md](eid-saml-4.4-requirements.md); `§` references in the
source and below point at it (and, where noted, at the OASIS `saml-*-2.0-os` specs
and the TVS *Checklist Testen*).

> The crate never touches the application's session. On success it hands the
> embedding app a verified subject identifier via `AuthState::on_authenticated`;
> the app creates and owns the session.

## Abbreviations

| Abbreviation | Meaning |
|---|---|
| ACS | Assertion Consumer Service: the SP endpoint that receives the authentication result (here: the artifact) |
| AD | *Authenticatiedienst*: the actual authentication service behind the RD (DigiD, eHerkenning, eIDAS) |
| ARS | Artifact Resolution Service: the RD's back-channel SOAP endpoint that exchanges an artifact for the `ArtifactResponse` |
| c14n | XML canonicalization (exclusive c14n is used in all signatures) |
| CSP | Content Security Policy (HTTP response header) |
| CSRF | Cross-Site Request Forgery |
| DV | *Dienstverlener*: Service Provider; the role this crate implements |
| eID | The Dutch electronic-identity system; the *Koppelvlakspecificatie eID SAML* defines this DV↔RD interface |
| eIDAS | EU *electronic Identification, Authentication and trust Services*: cross-border European login |
| IdP | Identity Provider: the SAML role the RD plays towards this SP |
| LoA | Level of Assurance (*betrouwbaarheidsniveau*) of the authentication |
| mTLS | Mutual TLS: both client and server authenticate with certificates |
| OIN | *Organisatie-identificatienummer*: Dutch government organisation number, carried in PKIoverheid certificates |
| PII | Personally Identifiable Information |
| PKIoverheid | The Dutch government PKI; its root CAs anchor all participant certificates |
| RD | *Routeringsdienst*: routing service between DV and the ADs; the IdP this SP talks to (TVS) |
| SAML | Security Assertion Markup Language (v2.0) |
| SLO | Single Logout |
| SLS | Single Logout Service: the SP endpoint that receives the `LogoutResponse` |
| SP | Service Provider (SAML term for the DV role) |
| SSO | Single Sign-On |
| TVS | *Toegangsverleningsservice*: the government RD implementation this crate connects to |
| UA | User-Agent (the end user's browser) |
| XML-DSig | XML Digital Signature |
| XML-Enc | XML Encryption |
| XSW | XML Signature Wrapping: signature-relocation attack class the validation defends against |

## Endpoints

The router mounts the protocol endpoints (their paths are fixed; the ACS and SLS
paths are advertised in the SP metadata):

| Method + path | Role | Channel |
|---|---|---|
| `GET /saml/sp/metadata` | Serve the signed DV SP metadata (§8.3) | front, browser/RD |
| `GET /saml/sp/acs` | Assertion Consumer Service (HTTP-Artifact, §7.4) | front, browser |
| `GET /login/error` | Query-clean landing page for a failed authentication | front, browser |
| `POST /saml/sp/logout` | Receive the RD `LogoutResponse` (§7.7.2) | front, browser |
| `GET /saml/sp/autosubmit.js` | Script the HTTP-POST binding page submits | front, browser |

The two browser-facing *entry points* are **not** mounted by `router` and are
not in the SP metadata, so the app mounts them at URLs (and methods) of its
choosing:

| Path | Role | Handler |
|---|---|---|
| `/login` | Start SSO (§7.1 step 2 / §3.1.1) | [`handle_login`](src/handlers/login.rs) |
| `/logout` | Start SP-initiated logout (§7.7.1 / §3.1.1.1) | [`handle_logout`](src/handlers/logout.rs) |

## Channels

Two channels are used, and they have very different trust properties:

- **Front-channel**: the user's browser, over public TLS (https). Carries the
  AuthnRequest (out), the artifact (in), and both logout messages. Everything
  here is attacker-reachable, so nothing on it is trusted without a signature.
- **Back-channel**: direct DV→RD HTTPS with **mutual TLS** (§9.4): PKIoverheid
  client certificate, TLS ≥ 1.2, and the RD server pinned to the back-channel
  root CA ([`pki`](src/saml/pki.rs)). Carries the SOAP `ArtifactResolve` /
  `ArtifactResponse` exchange that actually delivers the assertion.

## The authentication happy flow

The three participants are the **Browser**, the **DV** (this crate), and the
**RD / IdP (TVS)**.

1. **Browser → DV:** `/login` (the app's login route).
2. **DV:** builds + signs the AuthnRequest, registers the request-id (replay
   store), and sets the flow cookie (bound to the request-id + UA).
3. **DV → Browser:** `200` with an HTML auto-POST form.
4. **Browser → RD:** `POST {sso_url}` with `SAMLRequest=base64(AuthnRequest)`
   (HTTP-POST binding).
5. **RD:** the user authenticates at DigiD / eHerkenning / eIDAS.
6. **RD → Browser → DV:** `GET /saml/sp/acs?SAMLart=<opaque artifact>` via a
   `302` (HTTP-Artifact binding).
7. **DV → RD:** `POST {ars_url}` with `SOAP(ArtifactResolve)` over the mTLS
   back-channel.
8. **RD → DV:** `SOAP(ArtifactResponse)`.
9. **DV:** validates the chain, decrypts the SubjectID, verifies the flow
   cookie, and consumes the request-id.
10. **DV → Browser:** `302` to the app's post-login page; meanwhile
    `on_authenticated(subject_id)` fires and the app creates a session.

### Messages, formats, and channels

| # | Message | Built/parsed by | Wire format | Channel |
|---|---|---|---|---|
| 1 | **AuthnRequest** | [`create_authn_request`](src/saml/messages.rs) | `samlp:AuthnRequest` XML, enveloped XML-DSig | base64 in a `SAMLRequest` form field, auto-POSTed (HTTP-POST binding) |
| 2 | **Artifact** | RD | opaque `SAMLart` value | query parameter on the ACS redirect (HTTP-Artifact binding) |
| 3 | **ArtifactResolve** | [`create_artifact_resolve`](src/saml/messages.rs) | `samlp:ArtifactResolve` XML, enveloped XML-DSig, wrapped in a SOAP 1.1 envelope | `POST text/xml` over **mTLS** to the ARS |
| 4 | **ArtifactResponse** | RD → [validation](src/saml/validation/) | SOAP → `samlp:ArtifactResponse` (RD-signed) → `samlp:Response` → `saml:Assertion` (SubjectIDs as `EncryptedID`) | SOAP body of the mTLS response |

All outgoing signatures use **exclusive c14n + enveloped transform, RSA-SHA256,
DigestMethod SHA-256** (the algorithms are fixed in the
[`templates/saml/*.xml`](templates/saml/), which are the source of truth for the
outgoing wire format.

The AuthnRequest additionally carries `ForceAuthn="true"`, a
`RequestedAuthnContext Comparison="minimum"` for the DV's minimum LoA, the
`IntendedAudience` + `ServiceUUID` extension attributes, and (optionally) a
`Scoping/IDPList` that pre-selects a single AD (DigiD / eHerkenning / eIDAS).

## What we verify / validate

Verification runs at two times: **RD metadata trust** is established once at
startup (and on each background refresh); the **per-login chain** is validated on
every ACS callback.

### RD metadata trust anchor ([`idp_metadata.rs`](src/saml/idp_metadata.rs))

This is the linchpin: every RD signing key used later comes from here, so the
metadata document is trusted by an *external* anchor, never by its own signature.

- `entityID` **equals** the pinned RD EntityID (a config constant per
  environment, not a value read from the document).
- The signing certificate carries the **expected RD OIN** in its
  `Subject.serialNumber` (§9.1).
- The signing certificate **chains to a pinned PKIoverheid root** via the
  embedded intermediates (§9.2). The signing keys themselves are *not* pinned
  (they rotate); trust derives from chain + OIN.
- The document's enveloped XML signature is then verified **against those
  pinned certs**.
- `validUntil`, if present, has to be in the future (§8.2/§8.5); endpoints are clean
  absolute **https** URLs with no characters that could break out of an HTML
  attribute / CSP header / request target.

### Front-channel binding, login-CSRF / forced login ([`flow.rs`](src/handlers/flow.rs))

- A one-shot `__Host-`-prefixed cookie set by `/login` binds the flow to the
  browser and its `User-Agent`; the ACS callback must present a cookie matching
  the assertion's `InResponseTo`. Cleared regardless of outcome.

### ArtifactResponse, §7.6.1 ([`artifact_response.rs`](src/saml/validation/artifact_response.rs))

- **Exactly one** top-level `ds:Signature` (the enveloped message signature),
  valid against an RD signing key selected from verified metadata (KeyInfo only
  *selects* the cert, §9.2).
- Signature / digest algorithm **allow-list** (RSA-SHA256+ / SHA-256+); an
  rsa-sha1 / sha1 downgrade is rejected (§9.1).
- **XSW defense**: every `ds:Reference` URI must target the consumed root element
  (empty or `#<root-id>`), so a signature whose digest matches a sibling/nested
  element cannot authenticate a forged wrapper.
- `@InResponseTo` equals our `ArtifactResolve` id; status is `Success`.

### Inner Response, §7.6.2 ([`response.rs`](src/saml/validation/response.rs))

- Status `Success` (else mapped to user-cancelled → `Cancelled`, or → `Error`).
- `@Destination` equals our ACS URL; `Issuer` equals the RD EntityID.
- No `EncryptedAssertion` present; an `Assertion` is present on success.

### Assertion, §7.6.3 / processing rules §7.6.3.5 ([`assertion.rs`](src/saml/validation/assertion.rs))

The Assertion is **not** verified by its own signature: its authenticity comes
from the enveloping RD signature on the ArtifactResponse (verified above), plus
binding its `Issuer` to the RD EntityID. Signatures inside an `Assertion`/`Advice`
are evidence-only (§9.1), and claims are read only from the outer assertion
(the `Advice` subtree is pruned).

- `Issuer` = RD EntityID (rule 1).
- `SubjectConfirmation` Method = `bearer`; `Recipient` = ACS URL (rule 2);
  `NotOnOrAfter` not passed (rule 3, ±30 s clock skew).
- `Conditions` `NotBefore`/`NotOnOrAfter` window valid (rule 6).
- `AudienceRestriction` contains the DV EntityID (rule 5).
- `AuthnContextClassRef` LoA ≥ the DV minimum (`Low`); equal-or-higher accepted,
  lower rejected (§7.6.3.2 / TVS T6); see [`loa.rs`](src/saml/validation/loa.rs).
- `InResponseTo` is extracted, then **matched-and-consumed atomically** against
  the outstanding-request store (rule 4 / replay §9.7): an absent, unknown,
  expired, or already-consumed value is rejected. The store is owned by the
  embedding app so the id survives `/login` and ACS landing on different instances
  ([`PendingRequests`](src/pending.rs) is the default in-memory implementation;
  TTL 15 min).

### SubjectID decryption, §7.6.3.4 / §9.3 ([`decryption.rs`](src/saml/decryption.rs))

- Encryption algorithm **allow-list** before decrypting: data cipher
  **AES-256-CBC**, key transport **RSA-OAEP** (RSA-1.5 and weaker ciphers
  rejected, blocking a Bleichenbacher-style downgrade).
- Decrypted with the DV's private encryption keys (each is tried, so a blob
  wrapped to a rotated key still decrypts).
- The decrypted `NameID` must use the `persistent` format, carry a
  `NameQualifier`, and must not carry `SPNameQualifier`/`SPProvidedID`.
- An assertion with no **acting** SubjectID is treated as an authentication
  failure (no usable identity).

> **PII:** decrypted SubjectIDs and the SAML `NameID` are wrapped in
> `SecretString` (zeroized on drop) and are never logged; only non-PII metadata
> (presence flags, lengths, LoA, entity URNs) appears in traces.

## Logout (SP-initiated only, §3.1.1.1)

1. `/logout` (the app's logout route): the app tears down its session and
   returns the recorded `NameID`; the DV builds a **signed `LogoutRequest`**
   (`saml:NameID`), registers its id, and auto-POSTs it to the RD SLO endpoint
   (HTTP-POST binding).
2. `POST /saml/sp/logout`: the RD's `LogoutResponse` must be a
   `samlp:LogoutResponse`, carry a **valid RD signature**, have an
   `@InResponseTo` matching a `LogoutRequest` this DV issued (consumed once, so a
   replay is rejected), and should report `Success`. The local session is already
   gone, so a failed/forged/replayed response is logged and dropped; the browser
   is always redirected to the post-logout page.

## Configuration

[`AuthConfig::from_env`](src/config.rs) reads four inputs; everything
environment-specific (RD endpoints, the Kiesraad DV EntityID / ServiceUUID, the
back-channel trust anchor, cert/key paths) derives from them:

| Variable | Meaning |
|---|---|
| `TVS_ENV` | `test` \| `preproduction` \| `production` |
| `CERTS_DIR` | Directory holding the DV certificate/key bundle |
| `BASE_URL` | Public origin (used to derive the SP ACS/SLO URLs) |
| `PRESELECTED_AD` | `Select` (default, RD shows its own picker) \| `DigiD` \| `eHerkenning` \| `eIDAS` |

The `tvs-mock` cargo feature targets the online shared TVS mock: it defaults
`TVS_ENV`/`BASE_URL` and embeds the committed test DV bundle from
[`fixtures/`](fixtures/). The embedded bundle is extracted and used whenever
`CERTS_DIR` is unset or points somewhere that holds no bundle, so a mock build
starts on a host with no certificates on disk. **Mock builds only; never enable
it for a real deployment** (it bakes in test private keys).

## Cryptography

XML-DSig signing/verification and XML-Enc decryption are delegated to the
pure-Rust `bergshamra-*` crates through the thin [`crypto`](src/saml/crypto.rs)
adapter; this crate owns the SAML-level policy around them (algorithm allow-lists,
key selection from verified metadata, the XSW root-coverage check).

## Tests

```
cargo test -p auth-service
```

Beyond the per-module unit tests, [`tests/`](tests/) covers message round-trips,
metadata validation, the full validation chain, and two XML-signature-wrapping
attack PoCs ([`xsw_sibling_poc.rs`](tests/xsw_sibling_poc.rs),
[`xsw_exploit_check.rs`](tests/xsw_exploit_check.rs)) that must stay rejected.
[`tvs_metadata.rs`](tests/tvs_metadata.rs) validates the real TVS mock metadata;
its tests are `#[ignore]`d because they need network access.

[`tvs_wire_samples.rs`](tests/tvs_wire_samples.rs) runs the validators over real
RD `ArtifactResponse` messages: the successful 4.4 flow, the §6.3 cluster
variant, the two §7.8 error paths, and a pre-4.4 DigiD message that must be
rejected. Samples and provenance in [`tests/fixtures/tvs/`](tests/fixtures/tvs/).
