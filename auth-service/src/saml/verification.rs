//! XML-DSig signature verification (crypto delegated to the [`crypto`]
//! backend).
//!
//! eID §9.2: verification certs MUST come from verified metadata; the KeyInfo's
//! KeyName/X509Certificate only selects which trusted cert to use. So: find the
//! KeyInfo, match it against `trusted_keys`, then verify with that cert.
use crate::{
    keys::{CertificateBase64, KeyPair},
    saml::{
        constants::NS_DSIG,
        crypto::{self, SignatureVerification},
        xml_parser::{
            Document, NodeId, QName, all_elements, children_by_tag, descendants_by_tag,
            direct_text, find_child, find_descendant,
        },
    },
};
use tracing::debug;

// eID §9.1: SHA-1 is no longer supported (except as the RSA padding hash). The
// SignatureValue MUST use at least RSA-SHA256 and digests at least SHA-256, so we
// only accept the §9.1 algorithm set on incoming signatures and reject anything
// else (notably an rsa-sha1 / sha1 downgrade).
const ALLOWED_SIGNATURE_METHODS: &[&str] = &[
    "http://www.w3.org/2001/04/xmldsig-more#rsa-sha256",
    "http://www.w3.org/2001/04/xmldsig-more#rsa-sha384",
    "http://www.w3.org/2001/04/xmldsig-more#rsa-sha512",
];
// eID §9.1 lists one URI per digest, but its SHA-384/SHA-512 spellings differ
// from the W3C-registered ones (it gives `xmldsig-more#sha512`, the registered
// form is `xmlenc#sha512`). Both spellings of each are accepted so a conformant
// signer is not rejected over a URI alias; the algorithm strength is identical.
const ALLOWED_DIGEST_METHODS: &[&str] = &[
    "http://www.w3.org/2001/04/xmlenc#sha256",
    "http://www.w3.org/2001/04/xmldsig-more#sha384",
    "http://www.w3.org/2001/04/xmlenc#sha384",
    "http://www.w3.org/2001/04/xmldsig-more#sha512",
    "http://www.w3.org/2001/04/xmlenc#sha512",
];

// eID §9.1: "Canonicalization MUST be carried out according to the exclusive
// c14n method without comments". Pinning this matters for more than tidiness: a
// `WithComments` canonicalization would pull comment nodes into the digest, so
// the bytes the signature covers would stop matching the comment-free view the
// validators navigate (see `xml_parser`), which is exactly the gap the XSW
// comment-injection tests probe.
const EXCLUSIVE_C14N: &str = "http://www.w3.org/2001/10/xml-exc-c14n#";

// eID §9.1: the signature is embedded with the Enveloped Signature Transform, so
// every Reference must declare it. Without it the digest would cover the
// `<Signature>` element itself, which cannot be what a valid enveloped signature
// over our message root did.
const ENVELOPED_SIGNATURE_TRANSFORM: &str = "http://www.w3.org/2000/09/xmldsig#enveloped-signature";

// Attribute names that name an element for a `#id` reference. Must stay aligned
// with the backend's ID map so both resolve a reference the same way;
// `get_attribute` matches by local name, so `"id"` also covers `xml:id`.
const ID_ATTRIBUTES: &[&str] = &["ID", "Id", "id", "AssertionID"];

/// The element the caller is about to consume, read from the caller's own tree.
///
/// SECURITY (XSW): [`verify_xml_signature`] re-parses its input and the backend
/// parses it again with a different parser, so "the element I extracted" and "the
/// element that was verified" come from separate parses. This makes their equality
/// a check instead of an assumption.
pub struct ExpectedRoot<'a> {
    pub namespace: &'a str,
    pub local_name: &'a str,
    /// `None` leaves the ID unasserted, for a caller that has not parsed the
    /// document itself. `signature_covers_root` still binds every Reference.
    pub id: Option<&'a str>,
}

/// Outcome of signature verification.
pub struct VerifyResult {
    pub errors: Vec<String>,
}

impl VerifyResult {
    /// Valid exactly when no error was recorded.
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Verify the enveloping XML-DSig signature of `xml` against `trusted_keys`.
///
/// `expected_root` identifies the element the caller will consume and is
/// mandatory: binding it is the XSW defence, so there is no way to ask for a
/// verification that skips it (see [`ExpectedRoot`]).
pub fn verify_xml_signature(
    xml: &str,
    trusted_keys: &[KeyPair],
    expected_root: &ExpectedRoot<'_>,
) -> VerifyResult {
    debug!(
        "[verify] Verifying XML signature (xml_len={}, trusted_key_count={})",
        xml.len(),
        trusted_keys.len()
    );
    let mut errors = Vec::new();
    // Parse the XML once; the document is used for key matching, signature
    // enumeration, algorithm checks and the Reference-covers-root check.
    match crate::saml::xml_parser::parse(xml) {
        Ok(doc) => SignatureChecks::new(&doc, &mut errors).check_and_verify(
            xml,
            trusted_keys,
            expected_root,
        ),
        Err(e) => errors.push(format!("XML parse error: {e}")),
    }
    let result = VerifyResult { errors };
    debug!(
        "[verify] Signature verification done: valid={}, error_count={}",
        result.is_valid(),
        result.errors.len()
    );
    result
}

/// The structural signature checks over one parsed document, sharing the error
/// accumulator (mirrors the validation layer's `Validator` context).
struct SignatureChecks<'a, 'input> {
    doc: &'a Document<'input>,
    errors: &'a mut Vec<String>,
}

impl<'a, 'input> SignatureChecks<'a, 'input> {
    fn new(doc: &'a Document<'input>, errors: &'a mut Vec<String>) -> Self {
        Self { doc, errors }
    }

    fn error(&mut self, message: String) {
        self.errors.push(message);
    }

    // eID §9.1: reject weak (e.g. SHA-1) signature/digest algorithms before
    // trusting the signature, regardless of what the backend would accept.
    //
    // SECURITY (XSW): `signature_covers_root` binds "signature valid" to "this
    // root element was signed". The backend only checks that each Reference's
    // digest matches its resolved target; in a detached layout that target may
    // be a sibling of the Signature while we consume the root. eID uses one
    // enveloped signature over the message root, so every Reference must target
    // this root (empty URI = whole document, or "#<root-id>"), and
    // `check_id_is_unique` makes that ID name the root unambiguously.
    fn check_and_verify(
        &mut self,
        xml: &str,
        trusted_keys: &[KeyPair],
        expected_root: &ExpectedRoot<'_>,
    ) {
        let root = self.doc.document_element();
        if !self.check_expected_root(root, expected_root) {
            return;
        }
        let sig_node = match self.enveloping_signature(root) {
            Ok(n) => n,
            Err(e) => {
                self.error(e);
                return;
            }
        };
        let Some(signed_info) = self.signed_info(sig_node) else {
            return;
        };
        if let Some(key) = self.find_matching_key(sig_node, trusted_keys)
            && self.check_signature_algorithms(signed_info)
            && self.signature_covers_root(root, signed_info)
        {
            self.verify_with_cert(xml, key);
        }
    }

    /// The signature's own `<SignedInfo>`, which is the only one the backend
    /// reads (`find_child_element(sig, SignedInfo)`).
    ///
    /// SECURITY (XSW): every algorithm and Reference check below must inspect
    /// the element the backend actually verifies. Searching the whole
    /// `<Signature>` subtree instead would let a decoy (say a `<ds:Object>`
    /// carrying conformant-looking declarations, placed before the real
    /// `SignedInfo`) answer the §9.1 checks while the backend signs and
    /// verifies under the weak algorithms in the real one.
    fn signed_info(&mut self, sig: NodeId) -> Option<NodeId> {
        match children_by_tag(self.doc, sig, NS_DSIG, "SignedInfo")[..] {
            [only] => Some(only),
            [] => {
                self.error("Signature has no SignedInfo".to_string());
                None
            }
            [..] => {
                self.error(
                    "Signature has more than one SignedInfo, so the signed algorithm \
                     declarations are ambiguous (possible XML signature wrapping)"
                        .to_string(),
                );
                None
            }
        }
    }

    /// SECURITY (XSW): require this parse's root to be the same element the caller
    /// extracted from its own tree. Runs first, so a mismatch never reaches the
    /// crypto backend. See [`ExpectedRoot`].
    fn check_expected_root(&mut self, root: NodeId, expected: &ExpectedRoot<'_>) -> bool {
        let Some(actual) = self.doc.node_qname(root) else {
            self.error("Signed document has no root element".to_string());
            return false;
        };
        let wanted = QName {
            namespace: Some(expected.namespace),
            local_name: expected.local_name,
        };
        if actual != wanted {
            self.error(format!(
                "Signed root element is {actual}, but the caller consumes {wanted} \
                 (possible XML signature wrapping)"
            ));
            return false;
        }
        if let Some(expected_id) = expected.id {
            let root_id = self.root_id(root);
            if root_id.as_deref() != Some(expected_id) {
                self.error(format!(
                    "Signed root element ID {root_id:?} does not match the ID {expected_id:?} the \
                     caller consumes (possible XML signature wrapping)"
                ));
                return false;
            }
        }
        true
    }

    /// Run the crypto backend over `xml` with the matched trusted cert,
    /// recording a failed or errored verification.
    fn verify_with_cert(&mut self, xml: &str, cert: &KeyPair) {
        match crypto::verify_signature(xml, &cert.cert_pem) {
            Ok(SignatureVerification::Valid) => {
                debug!("[verify] Signature OK");
            }
            Ok(SignatureVerification::Invalid(reason)) => {
                self.error(format!("Signature verification failed: {reason}"));
            }
            Err(e) => {
                self.error(e.to_string());
            }
        }
    }

    /// Locate the single *enveloping* `<Signature>` (a direct child of `root`)
    /// and require it to be the first `<Signature>` in the whole document; a
    /// structural violation is returned as the error message.
    ///
    /// eID §7.6.1/§7.6.3: verify only the *enveloping* signature. Nested
    /// signatures belong to nested elements signed by a different party (e.g.
    /// the DigiD AD inside an ArtifactResponse), so checking them against the
    /// RD keys would wrongly reject the response. The backend verifies the
    /// first signature it finds, so more than one enveloping signature is
    /// ambiguous. eID messages carry exactly one.
    ///
    /// SECURITY (XSW): the crypto backend verifies the *first* `<Signature>` in
    /// document order, which is not necessarily the enveloping one. A nested
    /// signature placed earlier in the document (e.g. a genuine RD signature
    /// wrapped inside a forged root) would be the signature actually verified,
    /// while the structural checks only inspect the enveloping signature, so a
    /// bogus never-verified enveloping signature could authenticate a forged
    /// root. Require the enveloping signature to be the first `<Signature>` in
    /// the whole document so the backend verifies exactly the signature we
    /// authorize. Genuine eID messages always place it first (Issuer ->
    /// Signature -> Status/Response), so this only rejects wrapped documents.
    fn enveloping_signature(&self, root: NodeId) -> Result<NodeId, String> {
        let sig_nodes = children_by_tag(self.doc, root, NS_DSIG, "Signature");
        debug!(
            "[verify] Found {} enveloping Signature element(s) on <{}>",
            sig_nodes.len(),
            self.doc.local_name(root).unwrap_or_default()
        );
        let [sig] = sig_nodes[..] else {
            return Err(match sig_nodes.len() {
                0 => "No ds:Signature element found".to_string(),
                n => format!("Expected exactly one enveloping ds:Signature, found {n}"),
            });
        };

        let all_sigs = descendants_by_tag(self.doc, root, NS_DSIG, "Signature");
        if all_sigs.first() != Some(&sig) {
            return Err(
                "A nested ds:Signature precedes the enveloping signature: the backend would \
                 verify a different signature than the enveloping one (possible XML signature \
                 wrapping)"
                    .to_string(),
            );
        }
        Ok(sig)
    }

    // eID §9.1: verify the Signature's declared SignatureMethod, every Reference
    // DigestMethod, the canonicalization method and the Reference transforms are
    // in the permitted set. Returns `false` (and records an error) on any
    // disallowed or missing algorithm so a weak signature is not trusted even if
    // the crypto backend could verify it.
    fn check_signature_algorithms(&mut self, signed_info: NodeId) -> bool {
        // Evaluate all four so every violation is reported, not just the first.
        let sig_method_ok = self.check_signature_method(signed_info);
        let digests_ok = self.check_digest_methods(signed_info);
        let c14n_ok = self.check_canonicalization_method(signed_info);
        let transforms_ok = self.check_reference_transforms(signed_info);
        sig_method_ok && digests_ok && c14n_ok && transforms_ok
    }

    // eID §9.1: the SignatureMethod MUST be RSA-SHA256 or stronger (no SHA-1).
    fn check_signature_method(&mut self, signed_info: NodeId) -> bool {
        match find_child(self.doc, signed_info, NS_DSIG, "SignatureMethod")
            .and_then(|n| self.doc.get_attribute(n, "Algorithm"))
        {
            Some(a) if ALLOWED_SIGNATURE_METHODS.contains(&a) => true,
            Some(a) => {
                self.error(format!(
                    "Disallowed SignatureMethod (eID §9.1 requires RSA-SHA256 or stronger): {a}"
                ));
                false
            }
            None => {
                self.error("Signature has no SignatureMethod algorithm".to_string());
                false
            }
        }
    }

    // eID §9.1: every Reference DigestMethod MUST be SHA-256 or stronger.
    fn check_digest_methods(&mut self, signed_info: NodeId) -> bool {
        let mut ok = true;
        let digests: Vec<NodeId> = self
            .references(signed_info)
            .into_iter()
            .flat_map(|r| children_by_tag(self.doc, r, NS_DSIG, "DigestMethod"))
            .collect();
        if digests.is_empty() {
            self.error("Signature has no DigestMethod".to_string());
            ok = false;
        }
        for d in digests {
            match self.doc.get_attribute(d, "Algorithm") {
                Some(a) if ALLOWED_DIGEST_METHODS.contains(&a) => {}
                Some(a) => {
                    self.error(format!(
                        "Disallowed DigestMethod (eID §9.1 requires SHA-256 or stronger): {a}"
                    ));
                    ok = false;
                }
                None => {
                    self.error("DigestMethod has no Algorithm".to_string());
                    ok = false;
                }
            }
        }
        ok
    }

    // eID §9.1: exclusive c14n without comments, on the SignedInfo itself.
    fn check_canonicalization_method(&mut self, signed_info: NodeId) -> bool {
        match find_child(self.doc, signed_info, NS_DSIG, "CanonicalizationMethod")
            .and_then(|n| self.doc.get_attribute(n, "Algorithm"))
        {
            Some(EXCLUSIVE_C14N) => true,
            Some(a) => {
                self.error(format!(
                    "Disallowed CanonicalizationMethod (eID §9.1 requires exclusive c14n \
                     without comments): {a}"
                ));
                false
            }
            None => {
                self.error("Signature has no CanonicalizationMethod algorithm".to_string());
                false
            }
        }
    }

    // eID §9.1: a Reference may only apply the enveloped-signature transform
    // (required) and exclusive c14n (optional, last).
    //
    // SECURITY (XSW): an allow-list, not a known-bad list. The backend implements
    // XPath, XPath Filter 2.0, XPointer, XSLT, base64 and Relationship transforms,
    // all of which select an arbitrary node-set to digest. Any of those would let
    // a Reference name `#<root-id>`, passing `signature_covers_root`, while
    // digesting something narrower.
    fn check_reference_transforms(&mut self, signed_info: NodeId) -> bool {
        let mut ok = true;
        for r in self.references(signed_info) {
            let nodes: Vec<NodeId> = children_by_tag(self.doc, r, NS_DSIG, "Transforms")
                .into_iter()
                .flat_map(|t| children_by_tag(self.doc, t, NS_DSIG, "Transform"))
                .collect();
            let mut transforms = Vec::with_capacity(nodes.len());
            for t in nodes {
                match self.doc.get_attribute(t, "Algorithm") {
                    Some(a) => transforms.push(a),
                    None => {
                        self.error("Signature Reference Transform has no Algorithm".to_string());
                        ok = false;
                    }
                }
            }
            ok &= self.check_transform_list(&transforms);
        }
        ok
    }

    /// The eID §9.1 transform allow-list, applied to one Reference in order.
    fn check_transform_list(&mut self, transforms: &[&str]) -> bool {
        let mut ok = true;

        for unexpected in transforms
            .iter()
            .filter(|t| ![ENVELOPED_SIGNATURE_TRANSFORM, EXCLUSIVE_C14N].contains(*t))
        {
            self.error(format!(
                "Signature Reference uses a disallowed transform (eID §9.1 permits only the \
                 enveloped-signature transform and exclusive c14n without comments): {unexpected}"
            ));
            ok = false;
        }

        let enveloped = transforms
            .iter()
            .filter(|t| **t == ENVELOPED_SIGNATURE_TRANSFORM)
            .count();
        if enveloped == 0 {
            self.error(
                "Signature Reference does not apply the enveloped-signature transform (eID §9.1)"
                    .to_string(),
            );
            ok = false;
        } else if enveloped > 1 {
            self.error(format!(
                "Signature Reference applies the enveloped-signature transform {enveloped} times \
                 (eID §9.1 permits one)"
            ));
            ok = false;
        }

        let c14n = transforms.iter().filter(|t| **t == EXCLUSIVE_C14N).count();
        if c14n > 1 {
            self.error(format!(
                "Signature Reference applies {c14n} canonicalization transforms (eID §9.1 \
                 permits at most one)"
            ));
            ok = false;
        }
        // c14n yields octets, so a later transform would get the wrong input kind.
        if c14n == 1 && transforms.last() != Some(&EXCLUSIVE_C14N) {
            self.error(
                "Signature Reference applies a canonicalization transform before another \
                 transform (eID §9.1: c14n comes last)"
                    .to_string(),
            );
            ok = false;
        }

        ok
    }

    // SECURITY (XSW): every `<Reference>` of the enveloping signature MUST
    // target the root element `root` (the element whose data the caller
    // consumes). Accepts an empty URI (whole document) or `#<id>` where `<id>`
    // is the root's ID attribute. Returns `false` (and records an error) on a
    // missing or off-root reference, so a signature whose digest matches a
    // sibling/nested element cannot authenticate a forged root wrapped around
    // it.
    fn signature_covers_root(&mut self, root: NodeId, signed_info: NodeId) -> bool {
        let root_id = self.root_id(root);

        let refs = self.references(signed_info);
        if refs.is_empty() {
            self.error("Signature has no Reference".to_string());
            return false;
        }
        for r in refs {
            let uri = self.doc.get_attribute(r, "URI").unwrap_or("");
            if uri.is_empty() {
                continue; // whole document, which is rooted at `root`
            }
            let targets_root = root_id
                .as_deref()
                .filter(|id| uri.strip_prefix('#') == Some(id));
            let Some(id) = targets_root else {
                self.error(format!(
                    "Signature Reference URI {uri:?} does not target the signed root element \
                     (possible XML signature wrapping)"
                ));
                return false;
            };
            // Names the root's ID, but uniquely only if nothing else carries it.
            if !self.check_id_is_unique(root, id) {
                return false;
            }
        }
        true
    }

    /// The `<Reference>` elements the backend processes: the direct children of
    /// `<SignedInfo>` (`find_child_elements(signed_info, Reference)`).
    fn references(&self, signed_info: NodeId) -> Vec<NodeId> {
        children_by_tag(self.doc, signed_info, NS_DSIG, "Reference")
    }

    /// The root's ID under any [`ID_ATTRIBUTES`] name. Owned so callers can hold
    /// it across an `error()` call.
    fn root_id(&self, root: NodeId) -> Option<String> {
        ID_ATTRIBUTES
            .iter()
            .find_map(|a| self.doc.get_attribute(root, a))
            .map(str::to_owned)
    }

    /// SECURITY (XSW): a `#id` reference names the root unambiguously only if
    /// exactly one element carries that ID, else the backend could resolve to one
    /// element while we consume another. The backend rejects duplicate IDs too;
    /// this is our own guarantee rather than an inherited one.
    fn check_id_is_unique(&mut self, root: NodeId, id: &str) -> bool {
        let carriers: Vec<NodeId> = all_elements(self.doc)
            .into_iter()
            .filter(|&n| {
                ID_ATTRIBUTES
                    .iter()
                    .any(|a| self.doc.get_attribute(n, a) == Some(id))
            })
            .collect();
        match carriers.as_slice() {
            [only] if *only == root => true,
            [_] => {
                self.error(format!(
                    "Signature Reference URI {id:?} resolves to an element other than the signed \
                     root (possible XML signature wrapping)"
                ));
                false
            }
            others => {
                self.error(format!(
                    "{} elements carry the ID {id:?} referenced by the signature, so it does not \
                     uniquely name the signed root (possible XML signature wrapping)",
                    others.len()
                ));
                false
            }
        }
    }

    // eID §9.2: the KeyInfo only *selects* which trusted cert verifies; a
    // KeyName or certificate that matches nothing from verified metadata is
    // rejected. Returns the matched key.
    fn find_matching_key<'k>(
        &mut self,
        sig: NodeId,
        trusted: &'k [KeyPair],
    ) -> Option<&'k KeyPair> {
        let found = if let Some(key_name_node) = find_descendant(self.doc, sig, NS_DSIG, "KeyName")
        {
            // `direct_text`: the key selector is the element's own text only.
            match direct_text(self.doc, key_name_node) {
                Some(name) => self.key_by_name(&name, trusted),
                None => {
                    self.error("Signature KeyInfo KeyName contains child elements".to_string());
                    None
                }
            }
        } else if let Some(x509_node) = find_descendant(self.doc, sig, NS_DSIG, "X509Certificate") {
            match direct_text(self.doc, x509_node) {
                Some(cert) => self.key_by_cert(&cert, trusted),
                None => {
                    self.error(
                        "Signature KeyInfo X509Certificate contains child elements".to_string(),
                    );
                    None
                }
            }
        } else {
            self.error(
                "Signature KeyInfo contains neither KeyName nor X509Certificate".to_string(),
            );
            None
        };
        if let Some(key) = found {
            debug!(
                "[verify] Signature matched trusted key key_name={}",
                key.key_name
            );
        }
        found
    }

    fn key_by_name<'k>(&mut self, key_name: &str, trusted: &'k [KeyPair]) -> Option<&'k KeyPair> {
        let key_name = key_name.trim();
        if let Some(found) = trusted.iter().find(|kp| kp.matches_key_name(key_name)) {
            return Some(found);
        }
        let trusted_names: Vec<&str> = trusted.iter().map(|kp| kp.key_name.as_str()).collect();
        self.error(format!(
            "Unknown KeyName in signature: {key_name} (trusted: {trusted_names:?})"
        ));
        None
    }

    fn key_by_cert<'k>(&mut self, cert_text: &str, trusted: &'k [KeyPair]) -> Option<&'k KeyPair> {
        // A `<ds:X509Certificate>` that is not a certificate at all matches
        // nothing: fail closed rather than treat the raw text as a candidate.
        let Ok(sig_cert) = CertificateBase64::parse(cert_text) else {
            self.error("X509Certificate in signature is not a base64 certificate".to_string());
            return None;
        };
        if let Some(found) = trusted.iter().find(|kp| kp.cert_base64 == sig_cert) {
            return Some(found);
        }
        self.error("X509Certificate in signature does not match any trusted key".to_string());
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::saml::constants::{NS_SAML, NS_SAMLP};

    fn key_pair(cert_pem: &str) -> KeyPair {
        KeyPair::from_pem(
            crate::keys::CertificatePem::parse(cert_pem).expect("test PEM parses"),
            crate::keys::PrivateKeyPem::absent(),
        )
    }

    // A self-contained test certificate (never used for real signing).
    const TEST_PEM: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/rd-signing-1.pem"
    ));

    #[test]
    fn only_the_root_signature_is_considered() {
        // A Signature nested in a child but none enveloping the root: the
        // nested one (e.g. an Assertion's, signed by a different party) must be
        // ignored here so it doesn't fail ArtifactResponse verification.
        let xml = format!(
            r##"<samlp:ArtifactResponse xmlns:samlp="{NS_SAMLP}" ID="_r"><samlp:Status/><samlp:Response><dsig:Signature xmlns:dsig="{NS_DSIG}"><dsig:KeyInfo><dsig:KeyName>x</dsig:KeyName></dsig:KeyInfo></dsig:Signature></samlp:Response></samlp:ArtifactResponse>"##
        );
        let expected = ExpectedRoot {
            namespace: NS_SAMLP,
            local_name: "ArtifactResponse",
            id: Some("_r"),
        };
        let result = verify_xml_signature(&xml, &[key_pair(TEST_PEM)], &expected);
        assert!(!result.is_valid());
        assert_eq!(
            result.errors,
            vec!["No ds:Signature element found".to_string()]
        );
    }

    #[test]
    fn find_matching_key_matches_sha256_key_name() {
        let kp = key_pair(TEST_PEM);
        let sha256 = kp.cert_pem.key_names()[1].clone();
        let xml = format!(
            r#"<Signature xmlns="http://www.w3.org/2000/09/xmldsig#"><KeyInfo><KeyName>{sha256}</KeyName></KeyInfo></Signature>"#
        );
        let doc = crate::saml::xml_parser::parse(&xml).unwrap();
        let sig = doc.document_element();
        let mut errors = Vec::new();
        let found = SignatureChecks::new(&doc, &mut errors)
            .find_matching_key(sig, std::slice::from_ref(&kp));
        assert!(found.is_some(), "SHA-256 KeyName should match: {errors:?}");
    }

    /// A standalone `<Signature>` carrying just the algorithm declarations
    /// `check_signature_algorithms` inspects.
    fn signed_info(sig_alg: &str, digest_alg: &str, c14n: &str, transforms: &[&str]) -> String {
        let transforms: String = transforms
            .iter()
            .map(|t| format!(r#"<Transform Algorithm="{t}"/>"#))
            .collect();
        format!(
            r#"<Signature xmlns="{NS_DSIG}"><SignedInfo><CanonicalizationMethod Algorithm="{c14n}"/><SignatureMethod Algorithm="{sig_alg}"/><Reference><Transforms>{transforms}</Transforms><DigestMethod Algorithm="{digest_alg}"/></Reference></SignedInfo></Signature>"#
        )
    }

    /// The `SignedInfo` of the enveloping signature on `root`, which is what the
    /// Reference/algorithm checks take.
    fn signed_info_of(doc: &Document<'_>, root: NodeId) -> NodeId {
        let sig = crate::saml::xml_parser::find_child(doc, root, NS_DSIG, "Signature")
            .expect("test signature present");
        crate::saml::xml_parser::find_child(doc, sig, NS_DSIG, "SignedInfo")
            .expect("test SignedInfo present")
    }

    fn algorithm_errors(xml: &str) -> (bool, Vec<String>) {
        let doc = crate::saml::xml_parser::parse(xml).unwrap();
        let sig = doc.document_element();
        let mut errors = Vec::new();
        let mut checks = SignatureChecks::new(&doc, &mut errors);
        // The checks read the signature's own SignedInfo, as the backend does.
        let ok = checks
            .signed_info(sig)
            .is_some_and(|si| checks.check_signature_algorithms(si));
        (ok, errors)
    }

    #[test]
    fn check_signature_algorithms_requires_exclusive_c14n() {
        // eID §9.1: exclusive c14n WITHOUT comments. The WithComments variant
        // would pull comments into the digest, breaking the equivalence between
        // what is signed and the comment-free tree the validators read.
        let (ok, errors) = algorithm_errors(&signed_info(
            "http://www.w3.org/2001/04/xmldsig-more#rsa-sha256",
            "http://www.w3.org/2001/04/xmlenc#sha256",
            "http://www.w3.org/2001/10/xml-exc-c14n#WithComments",
            &[ENVELOPED_SIGNATURE_TRANSFORM, EXCLUSIVE_C14N],
        ));
        assert!(!ok);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("Disallowed CanonicalizationMethod")),
            "{errors:?}"
        );

        // Inclusive c14n as a Reference transform is rejected by the allow-list,
        // which names the offending URI.
        let (ok, errors) = algorithm_errors(&signed_info(
            "http://www.w3.org/2001/04/xmldsig-more#rsa-sha256",
            "http://www.w3.org/2001/04/xmlenc#sha256",
            EXCLUSIVE_C14N,
            &[
                ENVELOPED_SIGNATURE_TRANSFORM,
                "http://www.w3.org/TR/2001/REC-xml-c14n-20010315",
            ],
        ));
        assert!(!ok);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("disallowed transform") && e.contains("REC-xml-c14n-20010315")),
            "{errors:?}"
        );
    }

    /// The backend implements these transforms and each can select an arbitrary
    /// node-set to digest, so the allow-list must reject them all.
    #[test]
    fn check_reference_transforms_rejects_node_set_selecting_transforms() {
        for rejected in [
            "http://www.w3.org/TR/1999/REC-xpath-19991116",
            "http://www.w3.org/2002/06/xmldsig-filter2",
            "http://www.w3.org/2001/04/xmldsig-more/xptr",
            "http://www.w3.org/TR/1999/REC-xslt-19991116",
            "http://www.w3.org/2000/09/xmldsig#base64",
            "http://schemas.openxmlformats.org/package/2006/RelationshipTransform",
        ] {
            let (ok, errors) = algorithm_errors(&signed_info(
                "http://www.w3.org/2001/04/xmldsig-more#rsa-sha256",
                "http://www.w3.org/2001/04/xmlenc#sha256",
                EXCLUSIVE_C14N,
                &[ENVELOPED_SIGNATURE_TRANSFORM, rejected],
            ));
            assert!(!ok, "{rejected} must be rejected");
            assert!(
                errors
                    .iter()
                    .any(|e| e.contains("disallowed transform") && e.contains(rejected)),
                "{rejected}: {errors:?}"
            );
        }
    }

    #[test]
    fn check_reference_transforms_accepts_only_the_two_permitted_shapes() {
        let algorithms = |transforms: &[&str]| {
            algorithm_errors(&signed_info(
                "http://www.w3.org/2001/04/xmldsig-more#rsa-sha256",
                "http://www.w3.org/2001/04/xmlenc#sha256",
                EXCLUSIVE_C14N,
                transforms,
            ))
        };

        // The two shapes eID §9.1 allows.
        for accepted in [
            vec![ENVELOPED_SIGNATURE_TRANSFORM],
            vec![ENVELOPED_SIGNATURE_TRANSFORM, EXCLUSIVE_C14N],
        ] {
            let (ok, errors) = algorithms(&accepted);
            assert!(ok, "{accepted:?} must be accepted: {errors:?}");
        }

        // c14n yields octets, so nothing may follow it.
        let (ok, errors) = algorithms(&[EXCLUSIVE_C14N, ENVELOPED_SIGNATURE_TRANSFORM]);
        assert!(!ok);
        assert!(
            errors.iter().any(|e| e.contains("c14n comes last")),
            "{errors:?}"
        );

        // Repeats of either permitted transform are rejected rather than ignored.
        let (ok, errors) = algorithms(&[
            ENVELOPED_SIGNATURE_TRANSFORM,
            ENVELOPED_SIGNATURE_TRANSFORM,
            EXCLUSIVE_C14N,
        ]);
        assert!(!ok);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("enveloped-signature transform 2 times")),
            "{errors:?}"
        );

        let (ok, errors) = algorithms(&[
            ENVELOPED_SIGNATURE_TRANSFORM,
            EXCLUSIVE_C14N,
            EXCLUSIVE_C14N,
        ]);
        assert!(!ok);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("2 canonicalization transforms")),
            "{errors:?}"
        );
    }

    #[test]
    fn check_signature_algorithms_requires_enveloped_transform() {
        // eID §9.1: the signature is embedded with the enveloped-signature
        // transform; a Reference without it did not digest our message root.
        let (ok, errors) = algorithm_errors(&signed_info(
            "http://www.w3.org/2001/04/xmldsig-more#rsa-sha256",
            "http://www.w3.org/2001/04/xmlenc#sha256",
            EXCLUSIVE_C14N,
            &[EXCLUSIVE_C14N],
        ));
        assert!(!ok);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("enveloped-signature transform")),
            "{errors:?}"
        );
    }

    #[test]
    fn check_signature_algorithms_accepts_sha256_and_rejects_sha1() {
        let ok_xml = signed_info(
            "http://www.w3.org/2001/04/xmldsig-more#rsa-sha256",
            "http://www.w3.org/2001/04/xmlenc#sha256",
            EXCLUSIVE_C14N,
            &[ENVELOPED_SIGNATURE_TRANSFORM, EXCLUSIVE_C14N],
        );
        let (ok, errors) = algorithm_errors(&ok_xml);
        assert!(ok, "{errors:?}");

        // rsa-sha1 SignatureMethod + sha1 DigestMethod must both be rejected.
        let sha1_xml = signed_info(
            "http://www.w3.org/2000/09/xmldsig#rsa-sha1",
            "http://www.w3.org/2000/09/xmldsig#sha1",
            EXCLUSIVE_C14N,
            &[ENVELOPED_SIGNATURE_TRANSFORM, EXCLUSIVE_C14N],
        );
        let (ok, errors) = algorithm_errors(&sha1_xml);
        assert!(!ok);
        assert!(errors.iter().any(|e| e.contains("SignatureMethod")));
        assert!(errors.iter().any(|e| e.contains("DigestMethod")));
    }

    #[test]
    fn signature_covers_root_binds_reference_to_root() {
        // Reference "#_root" with a matching root ID, and empty URI, both pass.
        for uri in ["#_root", ""] {
            let xml = format!(
                r##"<Root xmlns="x" ID="_root"><Signature xmlns="{NS_DSIG}"><SignedInfo><Reference URI="{uri}"/></SignedInfo></Signature></Root>"##
            );
            let doc = crate::saml::xml_parser::parse(&xml).unwrap();
            let root = doc.document_element();
            let si = signed_info_of(&doc, root);
            let mut errors = Vec::new();
            assert!(
                SignatureChecks::new(&doc, &mut errors).signature_covers_root(root, si),
                "{errors:?}"
            );
        }

        // A reference to a sibling/other id must be rejected (XSW wrapping).
        let xml = format!(
            r##"<Root xmlns="x" ID="_root"><Signature xmlns="{NS_DSIG}"><SignedInfo><Reference URI="#_sibling"/></SignedInfo></Signature></Root>"##
        );
        let doc = crate::saml::xml_parser::parse(&xml).unwrap();
        let root = doc.document_element();
        let si = signed_info_of(&doc, root);
        let mut errors = Vec::new();
        assert!(!SignatureChecks::new(&doc, &mut errors).signature_covers_root(root, si));
        assert!(errors.iter().any(|e| e.contains("wrapping")));
    }

    #[test]
    fn nested_signature_before_enveloping_is_rejected() {
        // A <Signature> nested in an earlier subtree precedes the enveloping
        // (direct-child) signature in document order. The backend would verify
        // that nested one, so verify_xml_signature must reject the document as a
        // possible signature-wrapping attack before any crypto runs.
        let xml = format!(
            r##"<Root xmlns="urn:x" ID="_root"><Wrapper><Signature xmlns="{NS_DSIG}"><SignedInfo><Reference URI="#_inner"/></SignedInfo><KeyInfo><KeyName>x</KeyName></KeyInfo></Signature></Wrapper><Signature xmlns="{NS_DSIG}"><SignedInfo><Reference URI="#_root"/></SignedInfo><KeyInfo><KeyName>x</KeyName></KeyInfo></Signature></Root>"##
        );
        let expected = ExpectedRoot {
            namespace: "urn:x",
            local_name: "Root",
            id: Some("_root"),
        };
        let result = verify_xml_signature(&xml, &[key_pair(TEST_PEM)], &expected);
        assert!(!result.is_valid());
        assert!(
            result.errors.iter().any(|e| e.contains("wrapping")),
            "{:?}",
            result.errors
        );
    }

    /// SECURITY (XSW): the bytes are parsed again here, so `ExpectedRoot` must
    /// reject any disagreement about which element they are.
    #[test]
    fn expected_root_must_match_the_element_the_caller_consumes() {
        let xml = format!(
            r##"<samlp:ArtifactResponse xmlns:samlp="{NS_SAMLP}" ID="_genuine"><Signature xmlns="{NS_DSIG}"><SignedInfo><Reference URI="#_genuine"/></SignedInfo><KeyInfo><KeyName>x</KeyName></KeyInfo></Signature></samlp:ArtifactResponse>"##
        );

        // A matching name and ID gets past the binding, on to the algorithm
        // checks that this stub signature fails.
        let matching = ExpectedRoot {
            namespace: NS_SAMLP,
            local_name: "ArtifactResponse",
            id: Some("_genuine"),
        };
        let errors = verify_xml_signature(&xml, &[key_pair(TEST_PEM)], &matching).errors;
        assert!(
            !errors
                .iter()
                .any(|e| e.contains("possible XML signature wrapping")),
            "the binding must accept the matching root: {errors:?}"
        );

        // Wrong local name, wrong namespace and wrong ID are each rejected.
        let mismatches = [
            ExpectedRoot {
                namespace: NS_SAMLP,
                local_name: "Response",
                id: Some("_genuine"),
            },
            ExpectedRoot {
                namespace: NS_SAML,
                local_name: "ArtifactResponse",
                id: Some("_genuine"),
            },
            ExpectedRoot {
                namespace: NS_SAMLP,
                local_name: "ArtifactResponse",
                id: Some("_other"),
            },
        ];
        for expected in &mismatches {
            let result = verify_xml_signature(&xml, &[key_pair(TEST_PEM)], expected);
            assert!(!result.is_valid());
            assert!(
                result
                    .errors
                    .iter()
                    .any(|e| e.contains("possible XML signature wrapping")),
                "{:?} should be rejected as wrapping: {:?}",
                expected.local_name,
                result.errors
            );
        }
    }

    /// SECURITY (XSW): `#id` names the root only if nothing else carries that ID.
    #[test]
    fn signature_covers_root_requires_the_referenced_id_to_be_unique() {
        // A second element carries the root's ID value under a different ID-ish
        // attribute name, so `#_root` no longer names the root unambiguously.
        let xml = format!(
            r##"<Root xmlns="urn:x" ID="_root"><Forged id="_root"/><Signature xmlns="{NS_DSIG}"><SignedInfo><Reference URI="#_root"/></SignedInfo></Signature></Root>"##
        );
        let doc = crate::saml::xml_parser::parse(&xml).unwrap();
        let root = doc.document_element();
        let si = signed_info_of(&doc, root);
        let mut errors = Vec::new();
        assert!(!SignatureChecks::new(&doc, &mut errors).signature_covers_root(root, si));
        assert!(
            errors
                .iter()
                .any(|e| e.contains("2 elements carry the ID") && e.contains("wrapping")),
            "{errors:?}"
        );
    }

    /// A key name is the element's own text, not text folded up from children.
    #[test]
    fn key_name_with_child_elements_is_rejected() {
        let kp = key_pair(TEST_PEM);
        let sha256 = kp.cert_pem.key_names()[1].clone();
        let xml = format!(
            r#"<Signature xmlns="{NS_DSIG}"><KeyInfo><KeyName><x>{sha256}</x></KeyName></KeyInfo></Signature>"#
        );
        let doc = crate::saml::xml_parser::parse(&xml).unwrap();
        let sig = doc.document_element();
        let mut errors = Vec::new();
        assert!(
            SignatureChecks::new(&doc, &mut errors)
                .find_matching_key(sig, std::slice::from_ref(&kp))
                .is_none(),
            "a KeyName whose text comes from a child element must not select a key"
        );
        assert!(
            errors.iter().any(|e| e.contains("contains child elements")),
            "{errors:?}"
        );
    }

    #[test]
    fn find_matching_key_reports_unknown_key_name() {
        let kp = key_pair(TEST_PEM);
        let xml = r#"<Signature xmlns="http://www.w3.org/2000/09/xmldsig#"><KeyInfo><KeyName>deadbeef</KeyName></KeyInfo></Signature>"#;
        let doc = crate::saml::xml_parser::parse(xml).unwrap();
        let sig = doc.document_element();
        let mut errors = Vec::new();
        assert!(
            SignatureChecks::new(&doc, &mut errors)
                .find_matching_key(sig, std::slice::from_ref(&kp))
                .is_none()
        );
        assert!(errors[0].contains("Unknown KeyName"));
    }
}
