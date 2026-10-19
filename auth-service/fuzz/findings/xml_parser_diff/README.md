# xml_parser_diff findings

## leading-colon-name (analysed, no bypass)

A name beginning with `:` is not a valid XML QName. The two parsers read it
differently:

- roxmltree (the app, `saml::xml_parser`) splits on the first `:`, finds an
  empty prefix, and reads what follows: `:ArtifactResponse` becomes local name
  `ArtifactResponse`; `:attr=` becomes attribute `attr`; and `:xmlns=` is even
  taken as a default-namespace declaration (so the element gains a namespace).
- uppsala (what `bergshamra-dsig` verifies against) keeps `:ArtifactResponse`,
  `:attr`, `:xmlns` as literal names.

Reproducers: `leading_colon_root`, `leading_colon_signature`,
`leading_colon_xmlns`.

So the app can match an element by `(namespace, local)` that the signature
backend sees under a different local name. This is an element/attribute
*identity* divergence, which is the XSW dimension, so `xml_parser_diff` reports
it rather than normalising it away.

It is not exploitable in the ACS pipeline:
- An RD never signs an invalid QName, so a genuine message never contains one.
- The XSW defences are ID-based (reference URI -> ID map, unique ID, "signed
  root ID == consumed root ID") and name-agnostic, and the enveloping signature
  must be the first `<Signature>` in document order. A leading-colon element the
  app matches is one the backend does *not* match, so it fails closed (the app
  sees a signature/element the backend ignores, not the reverse that would
  authenticate forged content).
- `saml_xsw` sweeps leading-colon root renames and `:xmlns`/`:name` forgeries
  combined with wrapping, duplicate IDs and extra signatures (the
  `LeadingColonForgery`/`ChangeRoot(LeadingColon)` mutations, see
  `fuzzing_tests::xsw_catalogue_never_bypasses`) and never accepts a
  non-genuine identity.

## id-attr-whitespace (analysed, no bypass)

An ID attribute value (`ID`/`Id`/`id`/`AssertionID`/`xml:id`) that contains a
literal TAB/LF/CR: roxmltree applies XML attribute-value normalisation (the
whitespace becomes a space) while uppsala, which builds the backend `#id` map,
keeps it raw. ID values drive `#id` reference resolution so they are compared
raw (not folded). It is not exploitable: an RD never mints an ID with embedded
whitespace, the reference URI is itself whitespace-normalised, and an element
whose canonicalised bytes carry such an ID fails digest verification (uppsala's
raw value is non-conformant against the signer's normalised one), so the
message is rejected. Allowlisted by default (this reproducer class is checked
in); `AUTH_SERVICE_FUZZ_STRICT=1` makes the harness panic on it again.

`xml_parser_diff` allowlists this class by default so an ongoing run hunts for
NEW divergences; `AUTH_SERVICE_FUZZ_STRICT=1` makes it panic on this class too,
for a fresh audit.
