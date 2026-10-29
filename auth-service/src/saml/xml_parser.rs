//! Read-only, namespace-aware XML DOM over [`roxmltree`], plus scoped traversal
//! helpers used by the SAML validators.
//!
//! [`roxmltree`] is a vetted, fuzzed, read-only DOM that resolves XML namespaces
//! and exposes each node's byte range in the source. We wrap it so the rest of
//! the crate keeps an index-based (`Document` + [`NodeId`]) surface: `parse`,
//! `document_element`, `get_attribute` (by local name), `local_name`,
//! `first_element_child`, [`inner_text`] (recursive, unescaped), and
//! [`Document::node_source`] (the raw source bytes of a node).
//!
//! Element lookups ([`find_child`], [`find_descendant`], …) match by
//! `(namespace-URI, local-name)`, so a `<saml:Issuer>` is only found when `saml`
//! resolves to the SAML assertion namespace, never by bare local name. This
//! avoids namespace-confusion attacks.
//!
//! SECURITY (XML Signature Wrapping): roxmltree excludes comments and processing
//! instructions from the element tree, and exclusive-c14n (used by the signature
//! backend) excludes them from the digest. The whole signed document is parsed
//! exactly once and the validators navigate that single tree, so an element
//! forged inside a comment is invisible to both extraction and the signature.

use std::ops::Range;

/// Index of a node within a [`Document`].
pub type NodeId = roxmltree::NodeId;

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

/// A namespace declaration in scope on an element; `prefix` is `None` for the
/// default namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamespaceDecl<'a> {
    pub prefix: Option<&'a str>,
    pub uri: &'a str,
}

/// A parsed, namespace-resolved XML document borrowing its source.
pub struct Document<'a> {
    inner: roxmltree::Document<'a>,
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

/// Cap on parsed node count, bounding memory from an oversized document; a
/// legitimate SAML message has at most a few thousand nodes. (DTDs are already
/// rejected: `ParsingOptions::allow_dtd` defaults to `false`.)
const NODE_LIMIT: u32 = 100_000;

/// Parse an XML string into a namespace-resolved [`Document`]. Errors on
/// malformed XML, an undeclared namespace prefix, empty input, or more than
/// `NODE_LIMIT` nodes.
pub fn parse(xml: &str) -> Result<Document<'_>, XmlError> {
    let opts = roxmltree::ParsingOptions {
        nodes_limit: NODE_LIMIT,
        ..Default::default()
    };
    let inner =
        roxmltree::Document::parse_with_options(xml, opts).map_err(|e| XmlError(e.to_string()))?;
    if inner.descendants().any(has_leading_colon_name) {
        return Err(XmlError("name starts with a colon".to_string()));
    }
    Ok(Document { inner })
}

/// roxmltree accepts `<:a>` and `<e :a="">` (an empty prefix), which is not
/// well-formed XML; reject it.
fn has_leading_colon_name(node: roxmltree::Node<'_, '_>) -> bool {
    if !node.is_element() {
        return false;
    }
    let input = node.document().input_text();
    let Some(source) = input.get(node.range()) else {
        return false;
    };
    let end_tag = source
        .rsplit_once("</")
        .is_some_and(|(_, end)| end.starts_with(':'));
    source.as_bytes().get(1) == Some(&b':')
        || end_tag
        || node
            .attributes()
            .any(|a| input.as_bytes().get(a.range().start) == Some(&b':'))
}

fn node_matches(node: roxmltree::Node<'_, '_>, ns: &str, local: &str) -> bool {
    node.is_element() && node.tag_name().name() == local && node.tag_name().namespace() == Some(ns)
}

impl<'a> Document<'a> {
    fn node(&self, id: NodeId) -> Option<roxmltree::Node<'_, 'a>> {
        self.inner.get_node(id)
    }

    /// The root (document) element. Guaranteed to exist: [`parse`] rejects a
    /// document without one.
    pub fn document_element(&self) -> NodeId {
        self.inner.root_element().id()
    }

    /// The local (unprefixed) name of element `id`, or `None` for a non-element.
    pub fn local_name(&self, id: NodeId) -> Option<&str> {
        let n = self.node(id)?;
        n.is_element().then(|| n.tag_name().name())
    }

    /// The expanded name of element `id`, or `None` for a non-element.
    pub fn node_qname(&self, id: NodeId) -> Option<QName<'_>> {
        let n = self.node(id)?;
        n.is_element().then(|| QName {
            namespace: n.tag_name().namespace(),
            local_name: n.tag_name().name(),
        })
    }

    /// The value of attribute `name` (matched by local name) on element `id`.
    pub fn get_attribute(&self, id: NodeId, name: &str) -> Option<&str> {
        self.node(id)?
            .attributes()
            .find(|a| a.name() == name)
            .map(|a| a.value())
    }

    /// The raw source bytes of node `id` (opening `<` through closing `>`),
    /// exactly as they appear in the parsed input.
    pub fn node_source(&self, id: NodeId) -> Option<&str> {
        let range: Range<usize> = self.node(id)?.range();
        self.inner.input_text().get(range)
    }

    /// The namespace declarations `id` inherits from its ancestors. A
    /// declaration counts as inherited when in scope at both `id` and its
    /// parent, so a prefix `id` redeclares itself (already in its start tag) is
    /// excluded.
    fn inherited_namespaces(&self, id: NodeId) -> Vec<NamespaceDecl<'_>> {
        let Some(node) = self.node(id) else {
            return Vec::new();
        };
        let parent_scope: Vec<NamespaceDecl<'_>> = node
            .parent()
            .map(|p| {
                p.namespaces()
                    .map(|ns| NamespaceDecl {
                        prefix: ns.name(),
                        uri: ns.uri(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        node.namespaces()
            .map(|ns| NamespaceDecl {
                prefix: ns.name(),
                uri: ns.uri(),
            })
            .filter(|decl| parent_scope.contains(decl))
            .collect()
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
        // `get`, not `[..]`: `insert_at` is derived from a `find` on this same
        // string so it is always a character boundary, but this function's
        // contract is to fail closed rather than panic on a surprise.
        Some(format!(
            "{}{declarations}{}",
            source.get(..insert_at)?,
            source.get(insert_at..)?
        ))
    }

    /// The first child element of `id` (skipping text/comment nodes), if any.
    pub fn first_element_child(&self, id: NodeId) -> Option<NodeId> {
        self.node(id)?
            .children()
            .find(roxmltree::Node::is_element)
            .map(|c| c.id())
    }

    /// All child elements of `id` (skipping text/comment nodes), in document
    /// order; empty for a non-element.
    pub fn element_children(&self, id: NodeId) -> Vec<NodeId> {
        self.node(id)
            .map(|n| {
                n.children()
                    .filter(roxmltree::Node::is_element)
                    .map(|c| c.id())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// All text under `id`, concatenated depth-first and unescaped, or `None` if
/// `id` is not a node of this document.
///
/// `None` rather than an empty string, which an element with no text also
/// yields: a caller must not read "element absent" as "element present but
/// empty".
pub fn inner_text(doc: &Document, id: NodeId) -> Option<String> {
    Some(
        doc.node(id)?
            .descendants()
            .filter_map(|d| d.is_text().then(|| d.text()).flatten())
            .collect(),
    )
}

/// The direct text children of `id`, unescaped, or `None` if `id` has any element
/// child.
///
/// SECURITY: use this rather than [`inner_text`] for values a trust decision is
/// made on (`Issuer`, `KeyName`, `NameID`, `Audience`, `AuthnContextClassRef`).
/// [`inner_text`] folds in descendant text, so `<saml:Issuer><x>urn:rd</x></saml:Issuer>`
/// would read as `urn:rd`.
pub fn direct_text(doc: &Document, id: NodeId) -> Option<String> {
    let node = doc.node(id)?;
    if node.children().any(|c| c.is_element()) {
        return None;
    }
    Some(
        node.children()
            .filter_map(|c| c.is_text().then(|| c.text()).flatten())
            .collect(),
    )
}

/// Every element in the document, in document order. Used for the document-wide
/// ID uniqueness check, which must look outside the referenced subtree.
pub fn all_elements(doc: &Document) -> Vec<NodeId> {
    doc.inner
        .root()
        .descendants()
        .filter(roxmltree::Node::is_element)
        .map(|n| n.id())
        .collect()
}

/// Find the first direct child element matching `(ns, local_name)`.
pub fn find_child(doc: &Document, id: NodeId, ns: &str, local_name: &str) -> Option<NodeId> {
    doc.node(id)?
        .children()
        .find(|c| node_matches(*c, ns, local_name))
        .map(|n| n.id())
}

/// Collect all direct child elements matching `(ns, local_name)`, in document order.
pub fn children_by_tag(doc: &Document, id: NodeId, ns: &str, local_name: &str) -> Vec<NodeId> {
    match doc.node(id) {
        Some(n) => n
            .children()
            .filter(|c| node_matches(*c, ns, local_name))
            .map(|c| c.id())
            .collect(),
        None => Vec::new(),
    }
}

/// Find the first descendant element (excluding `id` itself) matching
/// `(ns, local_name)`, in document order.
pub fn find_descendant(doc: &Document, id: NodeId, ns: &str, local_name: &str) -> Option<NodeId> {
    doc.node(id)?
        .descendants()
        .skip(1)
        .find(|d| node_matches(*d, ns, local_name))
        .map(|n| n.id())
}

/// All descendant elements of `id` (excluding `id` itself) in document order,
/// whatever their namespace.
pub fn element_descendants(doc: &Document, id: NodeId) -> Vec<NodeId> {
    match doc.node(id) {
        Some(n) => n
            .descendants()
            .skip(1)
            .filter(roxmltree::Node::is_element)
            .map(|d| d.id())
            .collect(),
        None => Vec::new(),
    }
}

/// Find all descendant elements (excluding `id` itself) matching `(ns, local_name)`.
pub fn descendants_by_tag(doc: &Document, id: NodeId, ns: &str, local_name: &str) -> Vec<NodeId> {
    match doc.node(id) {
        Some(n) => n
            .descendants()
            .skip(1)
            .filter(|d| node_matches(*d, ns, local_name))
            .map(|d| d.id())
            .collect(),
        None => Vec::new(),
    }
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
        .filter(|&n| {
            doc.node(n)
                .is_some_and(|node| node_matches(node, tag.0, tag.1))
        })
        .collect()
}

/// Pre-order list of descendant element ids under `id` (excluding `id` itself),
/// skipping any subtree rooted at a `prune` element.
fn walk_pruned(doc: &Document, id: NodeId, prune: Tag) -> Vec<NodeId> {
    let mut out = Vec::new();
    let Some(root) = doc.node(id) else {
        return out;
    };
    for child in root.children() {
        collect_pruned(child, prune, &mut out);
    }
    out
}

fn collect_pruned(node: roxmltree::Node<'_, '_>, prune: Tag, out: &mut Vec<NodeId>) {
    if !node.is_element() {
        return;
    }
    if node_matches(node, prune.0, prune.1) {
        return; // prune this subtree entirely
    }
    out.push(node.id());
    for child in node.children() {
        collect_pruned(child, prune, out);
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
        // roxmltree is namespace-strict: a fragment using an undeclared prefix is
        // an error (the validators always navigate a single, fully-declared tree
        // rather than re-parsing namespace-incomplete subtrees).
        assert!(parse(r#"<saml:Assertion>x</saml:Assertion>"#).is_err());
    }

    #[test]
    fn leading_colon_names_are_rejected() {
        for xml in ["<:a/>", "<a></:a>", r#"<a :b=""/>"#, "<a><:b/></a>"] {
            assert!(parse(xml).is_err(), "{xml}");
        }
        assert!(parse("<a><b c=\"\"/></a>").is_ok());
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
