//! Shared validation context and XML/SOAP helpers for the SAML validators.

use crate::{
    saml::{
        constants::{
            CLOCK_SKEW_SECONDS, MESSAGE_FRESHNESS_SECONDS, NS_SAML, NS_SAMLP, STATUS_SUCCESS,
        },
        xml_parser::{Document, NodeId, direct_text, find_child, find_descendant, inner_text},
    },
    types::EntityId,
};
use jiff::{SignedDuration, Timestamp};

/// Context threaded through every validation step: the parsed document, one
/// wall-clock reference with the eID §9.5 clock-skew allowance, and the error
/// accumulator the checks push their findings onto.
///
/// Mirrors the validators' contract: checks never abort, they record errors;
/// the caller treats the result as valid only when no error was recorded.
pub(super) struct Validator<'a, 'input> {
    pub doc: &'a Document<'input>,
    pub now: Timestamp,
    pub skew: SignedDuration,
    pub errors: &'a mut Vec<String>,
}

impl<'a, 'input> Validator<'a, 'input> {
    pub fn new(doc: &'a Document<'input>, errors: &'a mut Vec<String>) -> Self {
        Self {
            doc,
            now: Timestamp::now(),
            skew: SignedDuration::from_secs(CLOCK_SKEW_SECONDS),
            errors,
        }
    }

    pub fn error(&mut self, message: String) {
        self.errors.push(message);
    }

    /// eID §7.6.1/§7.6.2/§7.6.3 (and SAML core): `@Version` MUST be exactly `2.0`.
    /// A missing or differing version fails closed rather than being ignored.
    pub fn check_version(&mut self, node: NodeId, label: &str) {
        match self.doc.get_attribute(node, "Version") {
            Some("2.0") => {}
            Some(v) => self.error(format!(
                "{label} has unsupported @Version {v:?} (MUST be 2.0)"
            )),
            None => self.error(format!("{label} is missing the required @Version")),
        }
    }

    /// eID §7.6.1/§7.6.2/§7.6.3.5 rule 1: the `<saml:Issuer>` of `node` MUST be the
    /// pinned RD EntityID, so an RD-signed envelope naming a different entity is
    /// rejected. `None` skips the check (tests).
    pub fn check_issuer(&mut self, node: NodeId, expected_issuer: Option<&EntityId>, label: &str) {
        let Some(expected) = expected_issuer else {
            return;
        };
        // `direct_text`: an Issuer with element children is not an identity.
        let issuer = find_child(self.doc, node, NS_SAML, "Issuer");
        match issuer.map(|n| direct_text(self.doc, n)) {
            Some(Some(ref i)) if expected == i.trim() => {}
            Some(Some(i)) => self.error(format!(
                "{label} Issuer mismatch: expected {expected}, got {}",
                i.trim()
            )),
            Some(None) => self.error(format!("{label} Issuer contains child elements")),
            None => self.error(format!("{label} has no Issuer")),
        }
    }

    /// eID §7.6.1/§7.6.2: require a `Success` StatusCode on `node`, composing the
    /// second-level StatusCode and StatusMessage (§7.8) into the error so the actual
    /// reason is visible in logs. Returns the top-level status code for callers that
    /// branch on it.
    pub fn check_status_success(&mut self, node: NodeId, label: &str) -> Option<String> {
        let status_code = find_status_code(self.doc, node);
        if status_code.as_deref() != Some(STATUS_SUCCESS) {
            let second = find_nested_status_code(self.doc, node);
            let message = find_samlp_text(self.doc, node, "StatusMessage");
            self.error(format!(
                "{label} status: {} ({}) - {}",
                status_code.as_deref().unwrap_or("unknown"),
                second.map(|s| s.to_string()).unwrap_or_default(),
                message.map(|m| m.to_string()).unwrap_or_default()
            ));
        }
        status_code
    }

    /// Bound an `@IssueInstant`/`@AuthnInstant` on both sides: reject a value older
    /// than the message-freshness window (plus skew) and one further in the future
    /// than skew allows.
    ///
    /// eID §7.6.1/§7.6.2/§7.6.3 give `@IssueInstant` cardinality 1 on every message
    /// we receive and §7.6.3 the same for `@AuthnInstant`, so an absent or
    /// unparseable value fails closed rather than skipping the freshness bound.
    ///
    /// The spec sets no explicit freshness window (it bounds the Assertion only via
    /// `Conditions`), so [MESSAGE_FRESHNESS_SECONDS] is our own ceiling: the
    /// envelope types carry no `Conditions` at all, which would otherwise leave
    /// them unbounded in time. The future bound matters for the same reason:
    /// without it a far-future instant would never expire.
    pub fn check_freshness(&mut self, val: Option<&str>, label: &str) {
        let Some(s) = val else {
            self.error(format!("{label} is missing (required, cardinality 1)"));
            return;
        };
        let Ok(t) = s.parse::<Timestamp>() else {
            self.error(format!("{label} has an invalid timestamp: {s}"));
            return;
        };
        let max_age = SignedDuration::from_secs(MESSAGE_FRESHNESS_SECONDS);
        let Some(stale_after) = self.shifted(t, max_age + self.skew, label, s) else {
            return;
        };
        let Some(not_before) = self.shifted(t, -self.skew, label, s) else {
            return;
        };
        if stale_after < self.now {
            self.error(format!("{label} is stale: issued at {s}"));
        } else if not_before > self.now {
            self.error(format!("{label} is in the future: issued at {s}"));
        }
    }

    /// `t` shifted by `delta`, or `None` (with an error recorded) when the shift
    /// is not representable.
    ///
    /// jiff's `+`/`-` on a `Timestamp` **panic** on overflow, and every instant
    /// the validators shift is a wire value: a timestamp near the edge of the
    /// representable range (jiff parses years up to 9999) would take the
    /// process down. An instant that cannot absorb a 30-second skew allowance is
    /// nonsense anyway, so it is rejected rather than reasoned about.
    fn shifted(
        &mut self,
        t: Timestamp,
        delta: SignedDuration,
        label: &str,
        raw: &str,
    ) -> Option<Timestamp> {
        let shifted = t.checked_add(delta).ok();
        if shifted.is_none() {
            self.error(format!(
                "{label} timestamp is outside the usable range: {raw}"
            ));
        }
        shifted
    }

    /// eID §7.6.3 (cardinality 1): `Conditions/@NotBefore` is mandatory (stricter
    /// than SAML core, where it is optional). Reject a missing, unparseable, or
    /// not-yet-valid value.
    pub fn check_not_before(&mut self, val: Option<&str>, label: &str) {
        if let Some((t, s)) = self.parse_required_instant(val, "NotBefore", label)
            && let Some(not_before) = self.shifted(t, -self.skew, label, s)
            && not_before > self.now
        {
            self.error(format!("{label} not yet valid: NotBefore {s}"));
        }
    }

    /// eID §7.6.3 (cardinality 1) / §9.5: @NotOnOrAfter is mandatory, so a missing
    /// or unparseable value fails closed rather than silently skipping the
    /// freshness check (an assertion with no/garbage expiry must not be accepted).
    pub fn check_not_on_or_after(&mut self, val: Option<&str>, label: &str) {
        if let Some((t, s)) = self.parse_required_instant(val, "NotOnOrAfter", label)
            && let Some(expires_at) = self.shifted(t, self.skew, label, s)
            && expires_at < self.now
        {
            self.error(format!("{label} expired: {s}"));
        }
    }

    /// Parse a mandatory timestamp attribute, failing closed: a missing or
    /// unparseable value records an error and yields `None`. On success the parsed
    /// instant is returned together with the raw string (for error messages).
    fn parse_required_instant<'v>(
        &mut self,
        val: Option<&'v str>,
        attr: &str,
        label: &str,
    ) -> Option<(Timestamp, &'v str)> {
        let Some(s) = val else {
            self.error(format!("{label} is missing the required {attr}"));
            return None;
        };
        match s.parse::<Timestamp>() {
            Ok(t) => Some((t, s)),
            Err(_) => {
                self.error(format!("{label} has an invalid {attr} timestamp: {s}"));
                None
            }
        }
    }
}

fn find_status_code(doc: &Document, root: NodeId) -> Option<String> {
    find_descendant(doc, root, NS_SAMLP, "StatusCode")
        .and_then(|n| doc.get_attribute(n, "Value"))
        .map(String::from)
}

fn find_nested_status_code(doc: &Document, root: NodeId) -> Option<String> {
    let sc = find_descendant(doc, root, NS_SAMLP, "StatusCode")?;
    find_child(doc, sc, NS_SAMLP, "StatusCode")
        .and_then(|n| doc.get_attribute(n, "Value"))
        .map(String::from)
}

/// Find a `samlp:`-namespaced descendant element's text (e.g. `StatusMessage`).
fn find_samlp_text(doc: &Document, root: NodeId, local_name: &str) -> Option<String> {
    find_descendant(doc, root, NS_SAMLP, local_name).and_then(|n| inner_text(doc, n))
}

/// Find a direct child element `(ns, local_name)` as a node in the parsed tree.
///
/// SECURITY (XML Signature Wrapping): exclusive-c14n and roxmltree both exclude
/// comments, so a raw string scan could slice a forged element out of a comment
/// interior that the signature digest never covered. Navigating the single
/// parsed tree (instead of re-scanning bytes) reads exactly the element the
/// signed, comment-excluded view sees.
pub(super) fn child_element(
    doc: &Document,
    parent: NodeId,
    ns: &str,
    local_name: &str,
) -> Option<NodeId> {
    find_child(doc, parent, ns, local_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::saml::xml_parser::parse;

    fn root_of(doc: &Document) -> NodeId {
        doc.document_element()
    }

    /// Run `check` with a [`Validator`] over a dummy document and return the
    /// recorded errors (the time checks never read the document).
    fn time_check_errors(check: impl FnOnce(&mut Validator)) -> Vec<String> {
        let doc = parse(r#"<x xmlns="urn:x"/>"#).unwrap();
        let mut errors = Vec::new();
        check(&mut Validator::new(&doc, &mut errors));
        errors
    }

    // -- child_element (parser-based, anti-XSW) --

    #[test]
    fn child_element_returns_direct_child() {
        let xml = format!(
            r#"<samlp:ArtifactResponse xmlns:samlp="{NS_SAMLP}"><samlp:Response ID="_r1">inner</samlp:Response></samlp:ArtifactResponse>"#
        );
        let doc = parse(&xml).unwrap();
        let child = child_element(&doc, root_of(&doc), NS_SAMLP, "Response").unwrap();
        assert_eq!(doc.get_attribute(child, "ID"), Some("_r1"));
    }

    #[test]
    fn child_element_none_when_missing() {
        let xml = format!(
            r#"<samlp:ArtifactResponse xmlns:samlp="{NS_SAMLP}">nothing</samlp:ArtifactResponse>"#
        );
        let doc = parse(&xml).unwrap();
        assert!(child_element(&doc, root_of(&doc), NS_SAMLP, "Response").is_none());
    }

    #[test]
    fn child_element_ignores_element_hidden_in_comment() {
        // SECURITY (XSW): a forged <Response> hidden in a comment must NOT be
        // found; the parser never materializes comment content, so the node
        // lookup returns the genuine direct child only.
        let xml = format!(
            r#"<samlp:ArtifactResponse xmlns:samlp="{NS_SAMLP}"><!--<samlp:Response ID="_forged">EVIL</samlp:Response>--><samlp:Response ID="_genuine">GOOD</samlp:Response></samlp:ArtifactResponse>"#
        );
        let doc = parse(&xml).unwrap();
        let child = child_element(&doc, root_of(&doc), NS_SAMLP, "Response").unwrap();
        assert_eq!(doc.get_attribute(child, "ID"), Some("_genuine"));
        assert_eq!(inner_text(&doc, child).as_deref(), Some("GOOD"));
    }

    // -- find_status_code --

    #[test]
    fn find_status_code_success() {
        let xml = format!(
            r#"<Response xmlns="{NS_SAMLP}"><Status><StatusCode Value="{STATUS_SUCCESS}"/></Status></Response>"#
        );
        let doc = parse(&xml).unwrap();
        assert_eq!(
            find_status_code(&doc, root_of(&doc)).unwrap(),
            STATUS_SUCCESS
        );
    }

    #[test]
    fn find_status_code_missing() {
        let xml = format!(r#"<Response xmlns="{NS_SAMLP}"/>"#);
        let doc = parse(&xml).unwrap();
        assert!(find_status_code(&doc, root_of(&doc)).is_none());
    }

    // -- find_nested_status_code --

    #[test]
    fn find_nested_status_code_extracts_second_level() {
        let xml = format!(
            r#"<Response xmlns="{NS_SAMLP}"><Status><StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:Responder"><StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:AuthnFailed"/></StatusCode></Status></Response>"#
        );
        let doc = parse(&xml).unwrap();
        let nested = find_nested_status_code(&doc, root_of(&doc)).unwrap();
        assert!(nested.contains("AuthnFailed"));
    }

    // -- check_not_on_or_after --

    #[test]
    fn check_not_on_or_after_valid() {
        let future = (Timestamp::now() + SignedDuration::from_hours(1))
            .strftime("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let errors = time_check_errors(|v| v.check_not_on_or_after(Some(&future), "Test"));
        assert!(errors.is_empty());
    }

    #[test]
    fn check_not_on_or_after_expired() {
        let past = (Timestamp::now() - SignedDuration::from_hours(1))
            .strftime("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let errors = time_check_errors(|v| v.check_not_on_or_after(Some(&past), "Test"));
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expired"));
    }

    #[test]
    fn check_freshness_bounds_both_directions_and_requires_presence() {
        let at = |offset: SignedDuration| {
            (Timestamp::now() + offset)
                .strftime("%Y-%m-%dT%H:%M:%SZ")
                .to_string()
        };

        // A recent instant is fresh.
        let errors = time_check_errors(|v| {
            v.check_freshness(Some(&at(-SignedDuration::from_secs(10))), "Msg")
        });
        assert!(errors.is_empty(), "fresh instant: {errors:?}");

        // eID cardinality 1: an absent instant fails closed.
        let errors = time_check_errors(|v| v.check_freshness(None, "Msg"));
        assert!(
            errors.iter().any(|e| e.contains("is missing")),
            "{errors:?}"
        );

        // Older than the freshness window plus skew is stale.
        let errors = time_check_errors(|v| {
            v.check_freshness(Some(&at(-SignedDuration::from_secs(400))), "Msg")
        });
        assert!(errors.iter().any(|e| e.contains("stale")), "{errors:?}");

        // Further in the future than skew allows is rejected too: without this a
        // far-future instant would never age out.
        let errors = time_check_errors(|v| {
            v.check_freshness(Some(&at(SignedDuration::from_hours(1))), "Msg")
        });
        assert!(
            errors.iter().any(|e| e.contains("in the future")),
            "{errors:?}"
        );

        // Inside the skew allowance a slightly-future instant is still accepted.
        let errors = time_check_errors(|v| {
            v.check_freshness(Some(&at(SignedDuration::from_secs(5))), "Msg")
        });
        assert!(errors.is_empty(), "within skew: {errors:?}");

        // An unparseable instant fails closed.
        let errors = time_check_errors(|v| v.check_freshness(Some("garbage"), "Msg"));
        assert!(errors.iter().any(|e| e.contains("invalid timestamp")));
    }

    #[test]
    fn check_not_before_requires_presence_and_rejects_future() {
        let at = |offset: SignedDuration| {
            (Timestamp::now() + offset)
                .strftime("%Y-%m-%dT%H:%M:%SZ")
                .to_string()
        };

        // eID §7.6.3 makes Conditions/@NotBefore mandatory.
        let errors = time_check_errors(|v| v.check_not_before(None, "Test"));
        assert!(
            errors.iter().any(|e| e.contains("missing the required")),
            "{errors:?}"
        );

        let errors = time_check_errors(|v| {
            v.check_not_before(Some(&at(SignedDuration::from_hours(1))), "Test")
        });
        assert!(
            errors.iter().any(|e| e.contains("not yet valid")),
            "{errors:?}"
        );

        let errors = time_check_errors(|v| v.check_not_before(Some("nonsense"), "Test"));
        assert!(errors.iter().any(|e| e.contains("invalid NotBefore")));

        let errors = time_check_errors(|v| {
            v.check_not_before(Some(&at(-SignedDuration::from_mins(5))), "Test")
        });
        assert!(errors.is_empty(), "past NotBefore: {errors:?}");
    }

    #[test]
    fn check_not_on_or_after_missing_or_malformed_fails_closed() {
        // Absent @NotOnOrAfter is a mandatory-element violation, not "no check".
        let errors = time_check_errors(|v| v.check_not_on_or_after(None, "Test"));
        assert!(errors.iter().any(|e| e.contains("missing the required")));

        // An unparseable timestamp must also be rejected, not silently accepted.
        let errors =
            time_check_errors(|v| v.check_not_on_or_after(Some("not-a-timestamp"), "Test"));
        assert!(errors.iter().any(|e| e.contains("invalid NotOnOrAfter")));
    }

    #[test]
    fn timestamps_at_the_edge_of_the_range_are_rejected_not_panicked_on() {
        // A hostile `@IssueInstant` / `@NotBefore` / `@NotOnOrAfter` at the edge
        // of jiff's range must be refused, never take the process down: shifting
        // it by the skew allowance overflows, and jiff's `+` panics on overflow,
        // so every timestamp check must reject it instead of doing arithmetic
        // on it.
        for raw in [Timestamp::MAX.to_string(), Timestamp::MIN.to_string()] {
            let raw = raw.as_str();
            assert!(
                raw.parse::<Timestamp>().is_ok(),
                "{raw} must actually parse, or this test proves nothing"
            );

            let errors = time_check_errors(|v| v.check_freshness(Some(raw), "Test"));
            assert!(!errors.is_empty(), "freshness accepted {raw}");

            let errors = time_check_errors(|v| v.check_not_before(Some(raw), "Test"));
            assert!(!errors.is_empty(), "NotBefore accepted {raw}");

            let errors = time_check_errors(|v| v.check_not_on_or_after(Some(raw), "Test"));
            assert!(!errors.is_empty(), "NotOnOrAfter accepted {raw}");
        }
    }
}
