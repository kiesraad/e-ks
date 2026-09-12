//! Read-only, namespace-aware XML DOM over [`uppsala`], plus scoped traversal
//! helpers used by the SAML validators.
//!
//! SECURITY (parser divergence): deliberately the same parser the signature
//! backend uses (`bergshamra` parses every signed document with `uppsala`). A
//! construct two parsers read differently is a signature wrapping vector: the
//! digest covers one tree, the claims come from another. See
//! `tests/xsw_parser_divergence.rs`.
//!
//! Lookups match by `(namespace-URI, local-name)`, so a `<saml:Issuer>` is only
//! found when `saml` resolves to the SAML assertion namespace, never by bare
//! local name. This avoids namespace-confusion attacks.
//!
//! SECURITY (XSW): comments and processing instructions are not elements, and
//! exclusive-c14n excludes them from the digest. The signed document is parsed
//! once and the validators navigate that one tree, so an element forged inside a
//! comment is invisible to both extraction and the signature.

/// Index of a node within a [`Document`].
pub type NodeId = uppsala::NodeId;

/// An element's expanded name: namespace URI (`None` for an element in no
/// namespace) plus local, unprefixed name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QName<'a> {
    pub namespace: Option<&'a str>,
    pub local_name: &'a str,
}

impl std::fmt::Display for QName<'_> {
    /// James Clark notation (`{namespace}local`), as used in the error messages.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.namespace {
            Some(ns) => write!(f, "{{{ns}}}{}", self.local_name),
            None => f.write_str(self.local_name),
        }
    }
}

/// A namespace declaration; `prefix` is `None` for the default namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamespaceDecl<'a> {
    pub prefix: Option<&'a str>,
    pub uri: &'a str,
}

/// A parsed, namespace-resolved XML document borrowing its source.
pub struct Document<'a> {
    inner: uppsala::Document<'a>,
    /// Resolved in [`parse`], so [`Document::document_element`] cannot panic.
    root_element: NodeId,
}

/// Opaque XML parse error (`Display`), convertible into `AuthError`.
#[derive(Debug)]
pub struct XmlError(String);

impl std::fmt::Display for XmlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for XmlError {}

/// Cap on parsed node count; a legitimate SAML message has a few thousand.
const NODE_LIMIT: usize = 100_000;

/// Cap on element nesting depth.
///
/// SECURITY (DoS): a recursive-descent parser spends stack per level, and a Rust
/// stack overflow aborts instead of unwinding, so unbounded depth on the
/// unauthenticated SLS endpoint (parsed before any signature check) is a remote
/// kill switch. `NODE_LIMIT` does not bound it: 4000 levels is 4000 nodes.
/// uppsala enforces this while tokenizing, which also bounds [`collect_pruned`].
///
/// eID messages nest under 20 deep and uppsala's own default is 128, so nothing
/// deeper could verify against the backend anyway.
const DEPTH_LIMIT: u32 = 100;

/// Parse into a namespace-resolved [`Document`]. Errors on malformed XML, a DTD,
/// empty input, nesting past `DEPTH_LIMIT`, or more than `NODE_LIMIT` nodes.
pub fn parse(xml: &str) -> Result<Document<'_>, XmlError> {
    // Both are opt-in: a DTD is never legitimate here (XXE / entity expansion),
    // and the depth cap is the DoS bound above.
    let inner = uppsala::Parser::new()
        .with_forbid_dtd(true)
        .with_max_depth(DEPTH_LIMIT)
        .parse(xml)
        .map_err(|e| XmlError(e.to_string()))?;

    // Validators navigate from the root element, so require one up front.
    let root_element = inner
        .document_element()
        .ok_or_else(|| XmlError("document has no root element".to_string()))?;

    let node_count = inner.descendants(inner.root()).len();
    if node_count > NODE_LIMIT {
        return Err(XmlError(format!(
            "document has {node_count} nodes, more than the limit of {NODE_LIMIT}"
        )));
    }

    Ok(Document {
        inner,
        root_element,
    })
}

/// Whether element `id` has the expanded name `(ns, local)`.
fn node_matches(doc: &Document, id: NodeId, ns: &str, local: &str) -> bool {
    doc.inner
        .element(id)
        .is_some_and(|el| el.matches_name_ns(ns, local))
}

impl<'a> Document<'a> {
    /// The root element. Guaranteed to exist: [`parse`] rejects a document
    /// without one.
    pub fn document_element(&self) -> NodeId {
        self.root_element
    }

    /// The local (unprefixed) name of element `id`, or `None` for a non-element.
    pub fn local_name(&self, id: NodeId) -> Option<&str> {
        Some(&self.inner.element(id)?.name.local_name)
    }

    /// The expanded name of element `id`, or `None` for a non-element.
    pub fn node_qname(&self, id: NodeId) -> Option<QName<'_>> {
        let name = &self.inner.element(id)?.name;
        Some(QName {
            namespace: name.namespace_uri.as_deref(),
            local_name: &name.local_name,
        })
    }

    /// The value of attribute `name` (matched by local name) on element `id`.
    pub fn get_attribute(&self, id: NodeId, name: &str) -> Option<&str> {
        self.inner.get_attribute(id, name)
    }

    /// The raw source bytes of node `id`, opening `<` through closing `>`.
    pub fn node_source(&self, id: NodeId) -> Option<&'a str> {
        self.inner.node_source(id)
    }

    /// Namespace declarations `id` inherits: every prefix declared on an
    /// ancestor that `id` does not redeclare, nearest declaration winning.
    fn inherited_namespaces(&self, id: NodeId) -> Vec<NamespaceDecl<'_>> {
        let Some(el) = self.inner.element(id) else {
            return Vec::new();
        };
        // uppsala spells the default namespace as the empty prefix, ours as `None`.
        let redeclared = |p: &str| el.namespace_declarations.iter().any(|(o, _)| **o == *p);

        let mut inherited: Vec<NamespaceDecl<'_>> = Vec::new();
        // Nearest-first, so the first binding seen for a prefix is the one in scope.
        for ancestor in self.inner.ancestors(id) {
            let Some(ancestor) = self.inner.element(ancestor) else {
                continue;
            };
            for (prefix, uri) in &ancestor.namespace_declarations {
                let shadowed = redeclared(prefix)
                    || inherited.iter().any(|d| d.prefix.unwrap_or("") == &**prefix);
                if !shadowed {
                    inherited.push(NamespaceDecl {
                        prefix: (!prefix.is_empty()).then_some(&**prefix),
                        uri,
                    });
                }
            }
        }
        inherited
    }

    /// [`Document::node_source`] with the inherited namespace declarations
    /// restored onto the start tag, for a subtree whose declarations live on an
    /// ancestor (e.g. a `soap:Envelope`) and so does not parse standalone.
    ///
    /// Digest-preserving only because exclusive c14n is pinned: it emits a
    /// declaration only where the prefix is visibly utilized, so restoring the
    /// scope the signer canonicalized in gives the same canonical bytes.
    ///
    /// `None` if the start tag cannot be delimited, or a URI would need attribute
    /// escaping (fail closed rather than escape).
    pub fn node_source_with_inherited_namespaces(&self, id: NodeId) -> Option<String> {
        let source = self.node_source(id)?;
        let inherited = self.inherited_namespaces(id);
        if inherited.is_empty() {
            return Some(source.to_owned());
        }
        if inherited
            .iter()
            .any(|decl| decl.uri.contains(['"', '&', '<']))
        {
            return None;
        }

        // Insert after the element name, which ends at the first whitespace, `/`
        // or `>`: a fixed position in a known start tag, not a content search.
        let rest = source.strip_prefix('<')?;
        let insert_at = 1 + rest.find(|c: char| c.is_whitespace() || c == '/' || c == '>')?;

        let declarations: String = inherited
            .iter()
            .map(|NamespaceDecl { prefix, uri }| match prefix {
                Some(p) => format!(r#" xmlns:{p}="{uri}""#),
                None => format!(r#" xmlns="{uri}""#),
            })
            .collect();
        // `get`, not `[..]`: `insert_at` comes from a `find` on this same string
        // so it is always a char boundary, but fail closed rather than panic.
        Some(format!(
            "{}{declarations}{}",
            source.get(..insert_at)?,
            source.get(insert_at..)?
        ))
    }

    /// Node `id` as a standalone document: its raw bytes when those parse on
    /// their own, else with the inherited namespace declarations restored.
    ///
    /// `None` when neither parses, so a caller never hands the crypto backend a
    /// fragment we could not re-read ourselves.
    pub fn self_contained_source(&self, id: NodeId) -> Option<String> {
        let raw = self.node_source(id)?;
        if parse(raw).is_ok() {
            return Some(raw.to_owned());
        }
        let reconstructed = self.node_source_with_inherited_namespaces(id)?;
        parse(&reconstructed).ok()?;
        Some(reconstructed)
    }

    /// The first child element of `id` (skipping text/comment nodes), if any.
    pub fn first_element_child(&self, id: NodeId) -> Option<NodeId> {
        self.inner
            .children_iter(id)
            .find(|&c| self.inner.element(c).is_some())
    }
}

/// All text under `id`, concatenated depth-first and unescaped, or `None` if
/// `id` is not a node of this document.
///
/// `None` rather than an empty string, which an element with no text also
/// yields: a caller must not read "absent" as "present but empty".
pub fn inner_text(doc: &Document, id: NodeId) -> Option<String> {
    doc.inner.node_kind(id)?;
    Some(doc.inner.text_content_deep(id))
}

/// The direct text children of `id`, unescaped, or `None` if `id` has any element
/// child.
///
/// SECURITY: use this rather than [`inner_text`] for values a trust decision is
/// made on (`Issuer`, `KeyName`, `NameID`, `Audience`, `AuthnContextClassRef`).
/// [`inner_text`] folds in descendant text, so `<saml:Issuer><x>urn:rd</x></saml:Issuer>`
/// would read as `urn:rd`.
///
/// Not uppsala's `element_text`, which returns only the *first* text child and
/// tolerates element children.
pub fn direct_text(doc: &Document, id: NodeId) -> Option<String> {
    doc.inner.node_kind(id)?;
    let mut text = String::new();
    for child in doc.inner.children_iter(id) {
        if doc.inner.element(child).is_some() {
            return None;
        }
        // CDATA counts as text, so it cannot hide an identifier.
        if let Some(t) = doc.inner.text_content(child) {
            text.push_str(t);
        }
    }
    Some(text)
}

/// Every element in the document, in document order. Used for the document-wide
/// ID uniqueness check, which must look outside the referenced subtree.
pub fn all_elements(doc: &Document) -> Vec<NodeId> {
    doc.inner
        .descendants(doc.inner.root())
        .into_iter()
        .filter(|&n| doc.inner.element(n).is_some())
        .collect()
}

/// Find the first direct child element matching `(ns, local_name)`.
pub fn find_child(doc: &Document, id: NodeId, ns: &str, local_name: &str) -> Option<NodeId> {
    doc.inner.first_child_element_by_name_ns(id, ns, local_name)
}

/// Collect all direct child elements matching `(ns, local_name)`, in document order.
pub fn children_by_tag(doc: &Document, id: NodeId, ns: &str, local_name: &str) -> Vec<NodeId> {
    doc.inner.child_elements_by_name_ns(id, ns, local_name)
}

/// Find the first descendant element (excluding `id` itself) matching
/// `(ns, local_name)`, in document order.
pub fn find_descendant(doc: &Document, id: NodeId, ns: &str, local_name: &str) -> Option<NodeId> {
    doc.inner
        .descendants(id)
        .into_iter()
        .find(|&d| node_matches(doc, d, ns, local_name))
}

/// Find all descendant elements (excluding `id` itself) matching `(ns, local_name)`.
pub fn descendants_by_tag(doc: &Document, id: NodeId, ns: &str, local_name: &str) -> Vec<NodeId> {
    doc.inner
        .descendants(id)
        .into_iter()
        .filter(|&d| node_matches(doc, d, ns, local_name))
        .collect()
}

/// A `(namespace-URI, local-name)` element tag, for the pruned lookups below.
pub type Tag<'a> = (&'a str, &'a str);

/// Like [`find_descendant`], but never descends into a subtree whose root
/// element matches `prune`.
///
/// Used by assertion validation to read claims from the outer RD Assertion while
/// skipping the `<saml:Advice>` evidence subtree (the AD assertions), which
/// carries its own Recipient / InResponseTo / scheme-specific LoA (eID §7.6.3).
pub fn find_descendant_pruned(doc: &Document, id: NodeId, tag: Tag, prune: Tag) -> Option<NodeId> {
    descendants_by_tag_pruned(doc, id, tag, prune)
        .into_iter()
        .next()
}

/// Like [`descendants_by_tag`], but skips any subtree rooted at an element
/// matching `prune`. See [`find_descendant_pruned`].
pub fn descendants_by_tag_pruned(doc: &Document, id: NodeId, tag: Tag, prune: Tag) -> Vec<NodeId> {
    walk_pruned(doc, id, prune)
        .into_iter()
        .filter(|&n| node_matches(doc, n, tag.0, tag.1))
        .collect()
}

/// Pre-order list of descendant element ids under `id` (excluding `id` itself),
/// skipping any subtree rooted at a `prune` element.
fn walk_pruned(doc: &Document, id: NodeId, prune: Tag) -> Vec<NodeId> {
    let mut out = Vec::new();
    for child in doc.inner.children_iter(id) {
        collect_pruned(doc, child, prune, &mut out);
    }
    out
}

/// Recursion depth is bounded by `DEPTH_LIMIT`, enforced at parse time.
fn collect_pruned(doc: &Document, id: NodeId, prune: Tag, out: &mut Vec<NodeId>) {
    if doc.inner.element(id).is_none() {
        return;
    }
    if node_matches(doc, id, prune.0, prune.1) {
        return; // prune this subtree entirely
    }
    out.push(id);
    for child in doc.inner.children_iter(id) {
        collect_pruned(doc, child, prune, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::saml::constants::{NS_DSIG, NS_SAML, NS_SAMLP, NS_SOAP};

    #[test]
    fn inner_text_concatenates_recursively() {
        let doc = parse(r#"<r xmlns="urn:x">hello <b>world</b></r>"#).unwrap();
        let root = doc.document_element();
        assert_eq!(inner_text(&doc, root).as_deref(), Some("hello world"));
    }

    #[test]
    fn find_descendant_matches_by_namespace_not_just_local_name() {
        // Two elements share the local name "Issuer" but live in different
        // namespaces; the lookup must only match the requested namespace.
        let xml = format!(
            r#"<samlp:Response xmlns:samlp="{NS_SAMLP}" xmlns:saml="{NS_SAML}"><other:Issuer xmlns:other="urn:other">WRONG</other:Issuer><saml:Issuer>RIGHT</saml:Issuer></samlp:Response>"#
        );
        let doc = parse(&xml).unwrap();
        let root = doc.document_element();
        let issuer = find_descendant(&doc, root, NS_SAML, "Issuer").unwrap();
        assert_eq!(inner_text(&doc, issuer).as_deref(), Some("RIGHT"));
        assert!(find_descendant(&doc, root, "urn:other", "Issuer").is_some());
        assert!(find_descendant(&doc, root, NS_SAMLP, "Issuer").is_none());
    }

    #[test]
    fn children_by_tag_only_returns_direct_children() {
        // A Signature directly on the root plus one nested in a child: only the
        // direct child is returned (the scoping that keeps ArtifactResponse
        // verification from choking on the nested, differently-signed Assertion).
        let xml = format!(
            r#"<Root xmlns="{NS_DSIG}"><Signature>outer</Signature><Child><Signature>inner</Signature></Child></Root>"#
        );
        let doc = parse(&xml).unwrap();
        let root = doc.document_element();
        let sigs = children_by_tag(&doc, root, NS_DSIG, "Signature");
        assert_eq!(sigs.len(), 1);
        assert_eq!(inner_text(&doc, sigs[0]).as_deref(), Some("outer"));
        // descendants_by_tag, by contrast, finds both.
        assert_eq!(
            descendants_by_tag(&doc, root, NS_DSIG, "Signature").len(),
            2
        );
    }

    #[test]
    fn attribute_access_by_local_name() {
        let doc = parse(r#"<el xmlns="urn:x" foo="bar" baz="qux"/>"#).unwrap();
        let root = doc.document_element();
        assert_eq!(doc.get_attribute(root, "foo"), Some("bar"));
        assert_eq!(doc.get_attribute(root, "baz"), Some("qux"));
        assert_eq!(doc.get_attribute(root, "missing"), None);
    }

    #[test]
    fn node_source_returns_exact_node_bytes() {
        // `node_source` must return the raw source slice of a node, from its
        // opening `<` to the end of its closing tag, bytes intact. The
        // EncryptedID decryption / signature paths depend on this.
        let xml =
            r#"<root xmlns="urn:r"><a>x</a><enc xmlns="urn:e"><data>cipher</data></enc></root>"#;
        let doc = parse(xml).unwrap();
        let root = doc.document_element();
        let enc = find_descendant(&doc, root, "urn:e", "enc").unwrap();
        assert_eq!(
            doc.node_source(enc),
            Some(r#"<enc xmlns="urn:e"><data>cipher</data></enc>"#)
        );
    }

    #[test]
    fn inner_text_unescapes_entities() {
        let doc = parse(r#"<r xmlns="urn:x">a &amp; b &lt;c&gt;</r>"#).unwrap();
        let root = doc.document_element();
        assert_eq!(inner_text(&doc, root).as_deref(), Some("a & b <c>"));
    }

    #[test]
    fn empty_document_is_error() {
        assert!(parse("").is_err());
    }

    /// Nest `depth` elements inside a SAML-shaped root, closing them or not.
    fn nested(depth: u32, closed: bool) -> String {
        let mut xml = format!(r#"<samlp:LogoutResponse xmlns:samlp="{NS_SAMLP}">"#);
        for _ in 0..depth {
            xml.push_str("<a>");
        }
        if closed {
            for _ in 0..depth {
                xml.push_str("</a>");
            }
        }
        xml.push_str("</samlp:LogoutResponse>");
        xml
    }

    /// The rejection must be the depth cap, not incidental malformedness.
    fn assert_rejected_for_depth(xml: &str) {
        let err = parse(xml).err().expect("must be rejected");
        let message = err.to_string();
        assert!(message.contains("depth"), "rejected for the wrong reason: {message}");
    }

    #[test]
    fn nesting_up_to_the_limit_is_accepted() {
        // The root element counts as one level, so DEPTH_LIMIT - 1 children fit.
        assert!(parse(&nested(DEPTH_LIMIT - 1, true)).is_ok());
    }

    #[test]
    fn nesting_past_the_limit_is_rejected() {
        assert_rejected_for_depth(&nested(DEPTH_LIMIT, true));
    }

    /// SECURITY (DoS): the payload that used to abort the process outright.
    ///
    /// A recursive-descent parser spends stack per nesting level and a Rust
    /// stack overflow aborts rather than unwinding, so this reached the parser
    /// through the unauthenticated SLS endpoint before any signature check. The
    /// missing close tags make the document malformed, but that verdict used to
    /// arrive only after the tokenizer had already recursed all the way down, and
    /// they cost the attacker just 3 bytes per level.
    #[test]
    fn unclosed_deep_nesting_is_rejected_before_anything_recurses() {
        let payload = nested(20_000, false);
        assert!(payload.len() < 64 * 1024, "cheap to send: {}", payload.len());
        assert_rejected_for_depth(&payload);
    }

    /// The depth cap counts elements, so markup that merely *contains* tag-like
    /// text must not inflate it, and a self-closing tag opens no level.
    #[test]
    fn depth_accounting_ignores_markup_that_only_looks_nested() {
        let deep = "<a>".repeat(DEPTH_LIMIT as usize + 50);
        let xml = format!(
            r#"<r xmlns="urn:x"><!--{deep}--><![CDATA[{deep}]]><?pi {deep}?><c t="a>b>c"/><d/></r>"#
        );
        let doc = parse(&xml).expect("must parse");
        // `>` is legal inside an attribute value; the value survived intact.
        let c = find_child(&doc, doc.document_element(), "urn:x", "c").expect("c");
        assert_eq!(doc.get_attribute(c, "t"), Some("a>b>c"));
    }

    /// `forbid_dtd` is opt-in on the parser, so guard the flag: a DTD is the
    /// classic XXE / entity-expansion vector and never legitimate here.
    #[test]
    fn doctype_is_rejected() {
        let xml = format!(
            r#"<!DOCTYPE r [<!ENTITY x "bsn">]><samlp:LogoutResponse xmlns:samlp="{NS_SAMLP}">&x;</samlp:LogoutResponse>"#
        );
        assert!(parse(&xml).is_err());
    }

    #[test]
    fn direct_text_excludes_element_children() {
        // Own text is returned, with entities unescaped.
        let doc = parse(r#"<r xmlns="urn:x">a &amp; b</r>"#).unwrap();
        assert_eq!(
            direct_text(&doc, doc.document_element()),
            Some("a & b".to_string())
        );

        // A child element yields None, where `inner_text` would fold in its text.
        let doc = parse(r#"<r xmlns="urn:x"><x>urn:rd</x></r>"#).unwrap();
        let root = doc.document_element();
        assert_eq!(direct_text(&doc, root), None);
        assert_eq!(inner_text(&doc, root).as_deref(), Some("urn:rd"));

        // Comments are not element children, so they do not suppress the text,
        // and their content is never part of it.
        let doc = parse(r#"<r xmlns="urn:x">ab<!--EVIL-->cd</r>"#).unwrap();
        assert_eq!(
            direct_text(&doc, doc.document_element()),
            Some("abcd".to_string())
        );
    }

    #[test]
    fn inherited_namespaces_make_a_sliced_element_parse_standalone() {
        // samlp:/saml: are declared on the envelope, not on the sliced element.
        let xml = format!(
            r#"<soap:Envelope xmlns:soap="{NS_SOAP}" xmlns:samlp="{NS_SAMLP}" xmlns:saml="{NS_SAML}"><soap:Body><samlp:ArtifactResponse ID="_a1"><saml:Issuer>urn:rd</saml:Issuer></samlp:ArtifactResponse></soap:Body></soap:Envelope>"#
        );
        let doc = parse(&xml).unwrap();
        let art =
            find_descendant(&doc, doc.document_element(), NS_SAMLP, "ArtifactResponse").unwrap();

        // The raw slice has undeclared prefixes.
        let raw = doc.node_source(art).unwrap();
        assert!(parse(raw).is_err(), "raw slice must not parse: {raw}");

        // With the inherited declarations restored it parses, and is the same
        // element with the same content.
        let restored = doc.node_source_with_inherited_namespaces(art).unwrap();
        let restored_doc = parse(&restored).expect("restored slice must parse");
        let root = restored_doc.document_element();
        assert_eq!(
            restored_doc.node_qname(root),
            Some(QName {
                namespace: Some(NS_SAMLP),
                local_name: "ArtifactResponse",
            })
        );
        assert_eq!(restored_doc.get_attribute(root, "ID"), Some("_a1"));
        let issuer = find_child(&restored_doc, root, NS_SAML, "Issuer").unwrap();
        assert_eq!(inner_text(&restored_doc, issuer).as_deref(), Some("urn:rd"));
    }

    #[test]
    fn self_contained_element_source_is_returned_unchanged() {
        // Nothing is inherited, so the bytes must be byte-identical to the slice.
        let xml = format!(r#"<samlp:Response xmlns:samlp="{NS_SAMLP}" ID="_r1"/>"#);
        let doc = parse(&xml).unwrap();
        let root = doc.document_element();
        assert_eq!(
            doc.node_source_with_inherited_namespaces(root).as_deref(),
            doc.node_source(root)
        );
    }

    #[test]
    fn all_elements_lists_the_whole_document() {
        let xml = format!(r#"<r xmlns="{NS_SAMLP}"><a/><b><c/></b></r>"#);
        let doc = parse(&xml).unwrap();
        let names: Vec<&str> = all_elements(&doc)
            .into_iter()
            .filter_map(|n| doc.local_name(n))
            .collect();
        assert_eq!(names, vec!["r", "a", "b", "c"]);
    }

    #[test]
    fn undeclared_namespace_prefix_is_rejected() {
        // The parser is namespace-strict: a fragment using an undeclared prefix is
        // an error (the validators always navigate a single, fully-declared tree
        // rather than re-parsing namespace-incomplete subtrees).
        assert!(parse(r#"<saml:Assertion>x</saml:Assertion>"#).is_err());
    }

    #[test]
    fn pruned_descendant_skips_advice_subtree() {
        // The Advice subtree carries an inner Assertion with its own Issuer; the
        // pruned search must read only the outer Issuer.
        let xml = format!(
            r#"<saml:Assertion xmlns:saml="{NS_SAML}"><saml:Advice><saml:Assertion><saml:Issuer>INNER</saml:Issuer></saml:Assertion></saml:Advice><saml:Issuer>OUTER</saml:Issuer></saml:Assertion>"#
        );
        let doc = parse(&xml).unwrap();
        let root = doc.document_element();
        let issuer =
            find_descendant_pruned(&doc, root, (NS_SAML, "Issuer"), (NS_SAML, "Advice")).unwrap();
        assert_eq!(inner_text(&doc, issuer).as_deref(), Some("OUTER"));
        // Only the outer Issuer is visible to the pruned descendant search.
        assert_eq!(
            descendants_by_tag_pruned(&doc, root, (NS_SAML, "Issuer"), (NS_SAML, "Advice")).len(),
            1
        );
        // Without pruning, both are found.
        assert_eq!(descendants_by_tag(&doc, root, NS_SAML, "Issuer").len(), 2);
    }
}
