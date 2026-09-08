# TVS wire samples

Real `ArtifactResponse` messages from the TVS *Routeringsdienst*, used by
[`tests/tvs_wire_samples.rs`](../../tvs_wire_samples.rs).

Copied verbatim from [`minvws/nl-rdo-max`](https://github.com/minvws/nl-rdo-max)
(the TVS reference SP) at commit `70d1e46907cb1a9af666891f9b87882bd0f00c0b`,
`tests/test-saml-art.tvs.xml` and `tests/resources/sample_messages/`. Upstream
is EUPL-1.2, as is this repository. The identifiers are test-environment values and the
`EncryptedID` payloads are wrapped to DV keys we do not hold.

| File | Shape |
|---|---|
| `artifact_response_success.xml` | 4.4 success: `Advice` AD assertion, `EncryptedID` ActingSubjectID, LoA substantial |
| `artifact_response_cluster.xml` | 4.4 §6.3 cluster connection (LC + DV audiences), LoA high |
| `artifact_response_login_cancelled.xml` | §7.8 `Responder` / `AuthnFailed` on the inner Response |
| `artifact_response_request_denied.xml` | §7.8 `Requester` / `RequestDenied` at the artifact layer, no inner Response |
| `artifact_response_digid_pre44.xml` | pre-4.4 DigiD: sector-coded plaintext NameID, no eID attributes |
