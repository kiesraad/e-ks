//! Differential test of the two XML parsers that read the same signed bytes:
//! roxmltree (the app's view, via [`xml_parser::parse`]) and uppsala (what
//! `bergshamra-dsig` digests and resolves `#id` references against).
//!
//! Both trees are lowered to [`CNode`]s holding what either consumer can act
//! on, and must be equal whenever both parsers accept.

use crate::saml::{verification::ID_ATTRIBUTES, xml_parser};
use std::{collections::BTreeMap, sync::LazyLock};

/// The ID attribute names `bergshamra-dsig` registers (`build_verify_id_map` +
/// the explicit `xml:id` lookup in `build_id_map`, verify.rs), looked up with
/// uppsala's `Element::get_attribute`.
const BACKEND_ID_ATTRIBUTES: &[&str] = &["Id", "ID", "id", "AssertionID", "xml:id"];

const XML_PREFIX: &str = "xml";

/// One node of the canonical tree. Comments AND processing instructions are
/// dropped, and adjacent text and CDATA merged: the app navigates elements and
/// text only (`xml_parser` excludes comments/PIs from the element tree), so this
/// is exactly the view every app lookup (`direct_text`, `inner_text`) acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CNode {
    Element(CElement),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CElement {
    pub namespace: Option<String>,
    pub local_name: String,
    /// In source order: both consumers take the first match by local name.
    pub attributes: Vec<CAttr>,
    /// ID values as each consumer's own lookup finds them, sorted.
    pub ids: Vec<String>,
    /// In-scope prefix bindings (`None` = default namespace), `xml` excluded.
    pub namespaces: BTreeMap<Option<String>, String>,
    pub children: Vec<CNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CAttr {
    pub namespace: Option<String>,
    pub local_name: String,
    /// After XML 1.0 attribute-value normalisation (§3.3.3), applied to BOTH
    /// sides. SECURITY: this folds away literal TAB/LF/CR that only one parser
    /// collapses to a space. It cannot hide an XSW: exclusive c14n binds the
    /// signed bytes, so an attribute whose two parses differ only after this
    /// normalisation fails digest verification. ID attributes, which drive
    /// `#id` reference resolution, are compared raw via `ids` instead.
    pub value: String,
}

/// How the two parsers relate on one input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Both accepted and produced the same canonical tree.
    Agree,
    BothRejected,
    /// Only roxmltree accepted: production rejects at signature verification.
    OnlyRoxmltree,
    /// Only uppsala accepted: production rejects at the first parse.
    OnlyUppsala,
    /// Both accepted with different trees. Boxed: a `Difference` is much larger
    /// than the other variants.
    Diverge(Box<Difference>),
}

/// Parse `xml` with both parsers and compare the canonical trees.
pub(crate) fn compare(xml: &str) -> Verdict {
    let rox = xml_parser::parse(xml);
    let upp = uppsala::parse(xml);
    match (rox, upp) {
        (Err(_), Err(_)) => Verdict::BothRejected,
        (Ok(_), Err(_)) => Verdict::OnlyRoxmltree,
        (Err(_), Ok(_)) => Verdict::OnlyUppsala,
        (Ok(rox), Ok(upp)) => {
            let a = roxmltree_tree(rox.roxmltree());
            let b = uppsala_tree(&upp);
            match first_difference(&a, &b, "") {
                None => Verdict::Agree,
                Some(diff) => Verdict::Diverge(Box::new(diff)),
            }
        }
    }
}

/// Audit mode (`AUTH_SERVICE_FUZZ_STRICT=1`): panic on EVERY divergence, even
/// the known, analysed classes. Off by default, so an ongoing `bin/fuzz` run
/// hunts for NEW divergences instead of re-reporting the all- listed ones (whose
/// reproducers are checked in under `fuzz/findings/`).
static STRICT: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("AUTH_SERVICE_FUZZ_STRICT").is_some());

/// Whether a divergence must be reported (panic). A NEW (unclassified) one
/// always must; a known, analysed class only under `strict`.
pub(crate) fn must_report(diff: &Difference, strict: bool) -> bool {
    strict || known_finding(diff).is_none()
}

/// The harness: panics when both parsers accept but disagree, unless the first
/// difference is a known analysed class (see [`must_report`]).
pub(crate) fn run(data: &[u8]) {
    let Ok(xml) = std::str::from_utf8(data) else {
        return;
    };
    if let Verdict::Diverge(diff) = compare(xml)
        && must_report(&diff, *STRICT)
    {
        panic!(
            "roxmltree and uppsala disagree (class: {}) on {xml:?}: {diff}",
            known_finding(&diff).unwrap_or("NEW")
        );
    }
}

// ---------------------------------------------------------------------------
// roxmltree, read through the accessors the app uses
// ---------------------------------------------------------------------------

/// The document's top-level nodes (root element plus prolog/epilog PIs).
pub(crate) fn roxmltree_tree(doc: &roxmltree::Document<'_>) -> Vec<CNode> {
    roxmltree_children(doc.root())
}

fn roxmltree_children(node: roxmltree::Node<'_, '_>) -> Vec<CNode> {
    let mut out = Vec::new();
    for child in node.children() {
        match child.node_type() {
            roxmltree::NodeType::Element => out.push(CNode::Element(roxmltree_element(child))),
            roxmltree::NodeType::Text => push_text(&mut out, child.text().unwrap_or_default()),
            roxmltree::NodeType::Comment | roxmltree::NodeType::PI | roxmltree::NodeType::Root => {}
        }
    }
    out
}

fn roxmltree_element(node: roxmltree::Node<'_, '_>) -> CElement {
    let attributes = node
        .attributes()
        .map(|a| CAttr {
            namespace: namespace_uri(a.namespace()),
            local_name: a.name().to_owned(),
            value: normalize_attr_value(a.value()),
        })
        .collect();
    // `verification::root_id`/`check_id_is_unique`: `get_attribute` per name,
    // which is the first attribute with that local name.
    let mut ids: Vec<String> = ID_ATTRIBUTES
        .iter()
        .filter_map(|name| node.attributes().find(|a| a.name() == *name))
        .map(|a| a.value().to_owned())
        .collect();
    ids.sort();
    // First binding per prefix wins, as in roxmltree's own prefix lookup.
    // A namespace declaration is an attribute, so its URI gets the same
    // attribute-value normalisation (see `normalize_attr_value`).
    let mut namespaces = BTreeMap::new();
    for ns in node.namespaces() {
        // `xmlns=""` is roxmltree's (None, "") binding, uppsala's absent one.
        let undeclared_default = ns.name().is_none() && ns.uri().is_empty();
        if ns.name() != Some(XML_PREFIX) && !undeclared_default {
            namespaces
                .entry(ns.name().map(str::to_owned))
                .or_insert_with(|| normalize_attr_value(ns.uri()));
        }
    }
    CElement {
        namespace: namespace_uri(node.tag_name().namespace()),
        local_name: node.tag_name().name().to_owned(),
        attributes,
        ids,
        namespaces,
        children: roxmltree_children(node),
    }
}

// ---------------------------------------------------------------------------
// uppsala, parsed exactly as bergshamra-dsig parses (`uppsala::parse`)
// ---------------------------------------------------------------------------

pub(crate) fn uppsala_tree(doc: &uppsala::Document<'_>) -> Vec<CNode> {
    uppsala_children(doc, doc.root(), &BTreeMap::new())
}

fn uppsala_children(
    doc: &uppsala::Document<'_>,
    id: uppsala::NodeId,
    scope: &BTreeMap<Option<String>, String>,
) -> Vec<CNode> {
    let mut out = Vec::new();
    for child in doc.children(id) {
        match doc.node_kind(child) {
            Some(uppsala::NodeKind::Element(e)) => {
                out.push(CNode::Element(uppsala_element(doc, child, e, scope)));
            }
            Some(uppsala::NodeKind::Text(t) | uppsala::NodeKind::CData(t)) => {
                push_text(&mut out, t)
            }
            Some(
                uppsala::NodeKind::ProcessingInstruction(_)
                | uppsala::NodeKind::Comment(_)
                | uppsala::NodeKind::Document
                | uppsala::NodeKind::Attribute(..),
            )
            | None => {}
        }
    }
    out
}

fn uppsala_element(
    doc: &uppsala::Document<'_>,
    id: uppsala::NodeId,
    e: &uppsala::Element<'_>,
    parent_scope: &BTreeMap<Option<String>, String>,
) -> CElement {
    let attributes = e
        .attributes
        .iter()
        .map(|a| CAttr {
            namespace: namespace_uri(a.name.namespace_uri.as_deref()),
            local_name: a.name.local_name.to_string(),
            value: normalize_attr_value(&a.value),
        })
        .collect();
    let mut ids: Vec<String> = BACKEND_ID_ATTRIBUTES
        .iter()
        .filter_map(|name| e.get_attribute(name))
        .map(str::to_owned)
        .collect();
    ids.sort();
    // Mirrors uppsala's `NamespaceResolver::declare` (an empty default URI
    // undeclares it).
    let mut namespaces = parent_scope.clone();
    for (prefix, uri) in &e.namespace_declarations {
        if prefix == XML_PREFIX {
            continue;
        }
        let key = (!prefix.is_empty()).then(|| prefix.to_string());
        if key.is_none() && uri.is_empty() {
            namespaces.remove(&None);
        } else {
            // Same attribute-value normalisation as roxmltree applies.
            namespaces.insert(key, normalize_attr_value(uri));
        }
    }
    let children = uppsala_children(doc, id, &namespaces);
    CElement {
        namespace: namespace_uri(e.name.namespace_uri.as_deref()),
        local_name: e.name.local_name.to_string(),
        attributes,
        ids,
        namespaces,
        children,
    }
}

// ---------------------------------------------------------------------------
// Shared lowering and comparison
// ---------------------------------------------------------------------------

/// Append text, merging with a preceding text node (see [`CNode`]).
///
/// Line ends are normalised to LF on BOTH sides. SECURITY: XML 1.0 §2.11
/// mandates this before parsing, and roxmltree applies it in CDATA while
/// uppsala does not. Like attribute normalisation it is digest-bound, and the
/// app trims text claims (Issuer, NameID, Audience, LoA) before comparing, so a
/// CR vs LF residue cannot select a different value that still verifies.
fn push_text(out: &mut Vec<CNode>, text: &str) {
    let text = normalize_text_line_ends(text);
    if let Some(CNode::Text(prev)) = out.last_mut() {
        prev.push_str(&text);
    } else {
        out.push(CNode::Text(text));
    }
}

/// XML 1.0 §3.3.3 attribute-value normalisation of literal whitespace: line
/// ends to a space, then TAB/LF to a space. Character references (`&#9;`) are
/// preserved by both parsers and so are not touched here.
fn normalize_attr_value(value: &str) -> String {
    normalize_text_line_ends(value).replace(['\t', '\n'], " ")
}

/// XML 1.0 §2.11 line-end normalisation: CRLF and lone CR to LF.
fn normalize_text_line_ends(s: &str) -> String {
    if s.contains('\r') {
        s.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        s.to_owned()
    }
}

/// A resolved namespace URI, normalised for comparison. A namespace URI derives
/// from a declaration's attribute value, so it gets the same attribute-value
/// normalisation; and an empty URI is `Some("")` in roxmltree (`xmlns=""`) but
/// `None` in uppsala, folded to `None`. SECURITY: every app element/attribute
/// lookup matches an exact, whitespace-free namespace constant, so a URI that
/// differs only by whitespace (or is empty) matches nothing on either side.
fn namespace_uri(uri: Option<&str>) -> Option<String> {
    uri.map(normalize_attr_value).filter(|u| !u.is_empty())
}

/// Where and how the two trees first differ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Difference {
    pub path: String,
    pub kind: DiffKind,
}

/// One side per field: roxmltree first, uppsala second.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DiffKind {
    Name(CElement, CElement),
    Attributes(Vec<CAttr>, Vec<CAttr>),
    Ids(Vec<String>, Vec<String>),
    Namespaces(
        BTreeMap<Option<String>, String>,
        BTreeMap<Option<String>, String>,
    ),
    Node(CNode, CNode),
    ChildCount(Vec<CNode>, Vec<CNode>),
}

impl std::fmt::Display for Difference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let path = &self.path;
        match &self.kind {
            DiffKind::Name(x, y) => write!(
                f,
                "{path}: name roxmltree {{{:?}}}{} vs uppsala {{{:?}}}{}",
                x.namespace, x.local_name, y.namespace, y.local_name
            ),
            DiffKind::Attributes(x, y) => {
                write!(f, "{path}: attributes roxmltree {x:?} vs uppsala {y:?}")
            }
            DiffKind::Ids(x, y) => write!(f, "{path}: IDs roxmltree {x:?} vs uppsala {y:?}"),
            DiffKind::Namespaces(x, y) => write!(
                f,
                "{path}: in-scope namespaces roxmltree {x:?} vs uppsala {y:?}"
            ),
            DiffKind::Node(x, y) => write!(f, "{path}: roxmltree {x:?} vs uppsala {y:?}"),
            DiffKind::ChildCount(x, y) => {
                write!(f, "{path}: children roxmltree {x:?} vs uppsala {y:?}")
            }
        }
    }
}

/// The first difference between two canonical trees, or `None` if equal.
pub(crate) fn first_difference(a: &[CNode], b: &[CNode], path: &str) -> Option<Difference> {
    for (i, pair) in a.iter().zip(b).enumerate() {
        let here = format!("{path}/{i}");
        let diff = match pair {
            (CNode::Element(x), CNode::Element(y)) => element_difference(x, y, &here),
            (x, y) if x == y => None,
            (x, y) => Some(Difference {
                path: here,
                kind: DiffKind::Node(x.clone(), y.clone()),
            }),
        };
        if diff.is_some() {
            return diff;
        }
    }
    (a.len() != b.len()).then(|| Difference {
        path: path.to_owned(),
        kind: DiffKind::ChildCount(a.to_vec(), b.to_vec()),
    })
}

fn element_difference(x: &CElement, y: &CElement, path: &str) -> Option<Difference> {
    let path = format!("{path}({})", x.local_name);
    let kind = if x.namespace != y.namespace || x.local_name != y.local_name {
        DiffKind::Name(shallow(x), shallow(y))
    } else if x.attributes != y.attributes {
        DiffKind::Attributes(x.attributes.clone(), y.attributes.clone())
    } else if x.ids != y.ids {
        DiffKind::Ids(x.ids.clone(), y.ids.clone())
    } else if x.namespaces != y.namespaces {
        DiffKind::Namespaces(x.namespaces.clone(), y.namespaces.clone())
    } else {
        return first_difference(&x.children, &y.children, &path);
    };
    Some(Difference { path, kind })
}

/// `e` without its subtree, to keep a report readable.
fn shallow(e: &CElement) -> CElement {
    CElement {
        children: Vec::new(),
        ..e.clone()
    }
}

// ---------------------------------------------------------------------------
// Known, analysed divergences (reproducers in `fuzz/findings/xml_parser_diff/`)
// ---------------------------------------------------------------------------

/// A name beginning with `:` is not a valid QName, and the two parsers read it
/// differently. roxmltree drops the empty prefix and reads what follows the
/// colon: `<:x>` becomes local name `x`, `:attr=` becomes attribute `attr`, and
/// `:xmlns=` is even taken as a default-namespace declaration. uppsala keeps the
/// literal `:x`/`:attr`/`:xmlns` as an ordinary name. These are all the same
/// lenient-parsing cause and show up as an element/attribute *identity* or
/// *namespace* difference, so they are NOT normalised away: the harness flags
/// them. `saml_xsw` injects `:name`/`:xmlns` and confirms no bypass (an RD never
/// signs such a name, and the ID-based reference/root binding is name-agnostic),
/// but element identity is the XSW dimension, so it is reported, not assumed safe.
pub(crate) const LEADING_COLON: &str = "leading-colon-name";

/// An ID attribute value whose two reads differ only by XML attribute-value
/// whitespace normalisation (roxmltree collapses a literal TAB/LF/CR to a
/// space; uppsala, which drives the backend's `#id` map, keeps it raw). ID
/// *values* are compared raw (not normalised) because they resolve references,
/// so this surfaces here rather than being folded. It cannot matter: a
/// legitimate RD never mints an ID with embedded whitespace, the reference URI
/// is itself whitespace-normalised, and an element whose c14n'd bytes carry
/// such an ID fails digest verification (uppsala's raw value is non-conformant
/// against the signer's normalised one), so the message is rejected.
pub(crate) const ID_WHITESPACE: &str = "id-attr-whitespace";

/// The known divergence class that explains `diff`, if any.
pub(crate) fn known_finding(diff: &Difference) -> Option<&'static str> {
    match &diff.kind {
        DiffKind::Name(x, y) => {
            (leading_colon_element(x) || leading_colon_element(y)).then_some(LEADING_COLON)
        }
        DiffKind::Attributes(x, y) => {
            (x.iter().chain(y).any(leading_colon_attr)).then_some(LEADING_COLON)
        }
        DiffKind::Ids(x, y) if x.len() == y.len() => x
            .iter()
            .zip(y)
            .all(|(a, b)| normalize_attr_value(a) == normalize_attr_value(b))
            .then_some(ID_WHITESPACE),
        _ => None,
    }
}

/// Whether a diverging element involves a leading-colon name (its own local
/// name, or any attribute's): the signature of roxmltree's invalid-QName
/// leniency (see [`LEADING_COLON`]).
fn leading_colon_element(e: &CElement) -> bool {
    e.local_name.starts_with(':') || e.attributes.iter().any(leading_colon_attr)
}

fn leading_colon_attr(a: &CAttr) -> bool {
    a.local_name.starts_with(':')
}
