//! LogoutResponse structural validation (eID §7.7.2).

use super::helpers::Validator;
use crate::{
    saml::{
        constants::{NS_SAMLP, STATUS_SUCCESS},
        xml_parser::{self, QName, find_descendant},
    },
    types::{EndpointUrl, EntityId, MessageId},
};

/// §7.7.2 correlation fields of a structurally valid LogoutResponse: the
/// `@InResponseTo` to consume and whether the status was `Success`.
#[derive(Debug)]
pub struct LogoutResponseFields {
    /// The root's `@ID`, so the signature verifier can bind the signed element
    /// to the one these fields were read from.
    pub id: Option<String>,
    pub in_response_to: MessageId,
    pub status_is_success: bool,
}

/// Parse the (not yet signature-verified) LogoutResponse XML and enforce the
/// §7.7.2 structural checks. Every element and attribute the §7.7.2 table gives
/// cardinality 1 is required: `samlp:LogoutResponse` root, `@Version` = 2.0, a
/// fresh `@IssueInstant`, `Issuer` = the RD, `@Destination` = our SLS endpoint,
/// and `@InResponseTo`. Returns the failure reason for the caller to log; every
/// failure resolves to the same "ignore and redirect" outcome, since the local
/// logout has already completed.
pub fn validate_logout_response(
    saml_response: &str,
    rd_entity_id: &EntityId,
    sls_url: &EndpointUrl,
) -> Result<LogoutResponseFields, String> {
    let doc = xml_parser::parse(saml_response)
        .map_err(|e| format!("could not parse LogoutResponse XML: {e}"))?;
    let root = doc.document_element();

    // Matched by (namespace, local name): a same-local-name element in another
    // namespace is not a samlp:LogoutResponse.
    let expected_root = QName {
        namespace: Some(NS_SAMLP),
        local_name: "LogoutResponse",
    };
    if doc.node_qname(root) != Some(expected_root) {
        return Err("response root is not samlp:LogoutResponse".to_string());
    }
    check_logout_response(&doc, root, rd_entity_id, sls_url)?;

    let Some(in_response_to) = doc.get_attribute(root, "InResponseTo") else {
        return Err("LogoutResponse has no InResponseTo".to_string());
    };
    // It is about to be looked up in the pending-request store, so require the
    // shape a LogoutRequest ID this DV issued actually has.
    let in_response_to = MessageId::parse(in_response_to)
        .map_err(|e| format!("LogoutResponse InResponseTo is not a message ID: {e}"))?;

    let status_is_success = find_descendant(&doc, root, NS_SAMLP, "StatusCode")
        .and_then(|n| doc.get_attribute(n, "Value"))
        == Some(STATUS_SUCCESS);

    Ok(LogoutResponseFields {
        id: doc.get_attribute(root, "ID").map(str::to_owned),
        in_response_to,
        status_is_success,
    })
}

/// The §7.7.2 mandatory-field checks: `@Version`, `@IssueInstant` freshness,
/// `Issuer` = the RD, and `@Destination` = our SLS endpoint.
fn check_logout_response(
    doc: &xml_parser::Document,
    root: xml_parser::NodeId,
    rd_entity_id: &EntityId,
    sls_url: &EndpointUrl,
) -> Result<(), String> {
    let mut errors = Vec::new();
    let mut v = Validator::new(doc, &mut errors);
    // eID §7.7.2 (cardinality 1): @Version MUST be 2.0.
    v.check_version(root, "LogoutResponse");
    // eID §7.7.2 (cardinality 1): @IssueInstant MUST be present. A LogoutResponse
    // carries no Conditions, so bound it the same way the other envelopes are
    // bounded, otherwise a captured response stays structurally valid forever
    // (the InResponseTo consume-once check is the only other replay bound).
    v.check_freshness(
        doc.get_attribute(root, "IssueInstant"),
        "LogoutResponse @IssueInstant",
    );
    // Bind to the RD, mirroring the ACS path.
    v.check_issuer(root, Some(rd_entity_id), "LogoutResponse");
    // eID §7.7.2 (cardinality 1): @Destination MUST be present and MUST be our
    // SLS endpoint, so a response minted for another SP is not accepted here.
    match doc.get_attribute(root, "Destination") {
        Some(d) if d == sls_url.as_str() => {}
        Some(_) => v.error("LogoutResponse @Destination is not our SLS endpoint".to_string()),
        None => v.error("LogoutResponse is missing the required @Destination".to_string()),
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::saml::constants::NS_SAML;

    const RD: &str = "urn:test:rd";
    const SLS: &str = "https://dv.example.com/saml/sp/logout";

    fn rd() -> EntityId {
        EntityId::from_static(RD)
    }

    fn sls() -> EndpointUrl {
        EndpointUrl::from_base_url(SLS, "SLS").expect("test SLS URL")
    }

    fn logout_response_xml(
        root: &str,
        issuer: &str,
        in_response_to: Option<&str>,
        destination: Option<&str>,
        status: &str,
    ) -> String {
        logout_response_with_instant(
            root,
            issuer,
            in_response_to,
            destination,
            status,
            Some(&now_offset(chrono::Duration::zero())),
        )
    }

    fn now_offset(offset: chrono::Duration) -> String {
        (chrono::Utc::now() + offset)
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string()
    }

    fn logout_response_with_instant(
        root: &str,
        issuer: &str,
        in_response_to: Option<&str>,
        destination: Option<&str>,
        status: &str,
        issue_instant: Option<&str>,
    ) -> String {
        let irt = in_response_to
            .map(|v| format!(r#" InResponseTo="{v}""#))
            .unwrap_or_default();
        let dest = destination
            .map(|v| format!(r#" Destination="{v}""#))
            .unwrap_or_default();
        let instant = issue_instant
            .map(|v| format!(r#" IssueInstant="{v}""#))
            .unwrap_or_default();
        format!(
            r#"<samlp:{root} xmlns:samlp="{NS_SAMLP}" xmlns:saml="{NS_SAML}" Version="2.0"{instant}{irt}{dest}>
                 <saml:Issuer>{issuer}</saml:Issuer>
                 <samlp:Status><samlp:StatusCode Value="{status}"/></samlp:Status>
               </samlp:{root}>"#
        )
    }

    #[test]
    fn extracts_fields_from_a_well_formed_logout_response() {
        let xml = logout_response_xml(
            "LogoutResponse",
            RD,
            Some("_req123"),
            Some(SLS),
            STATUS_SUCCESS,
        );
        let f = validate_logout_response(&xml, &rd(), &sls()).unwrap();
        assert!(f.status_is_success);
        assert_eq!(f.in_response_to.as_str(), "_req123");
    }

    #[test]
    fn non_success_status_is_extracted_not_rejected() {
        // A non-Success status is reported, not a structural failure: the local
        // logout already completed either way.
        let xml = logout_response_xml(
            "LogoutResponse",
            RD,
            Some("_req123"),
            Some(SLS),
            "urn:oasis:names:tc:SAML:2.0:status:Responder",
        );
        let f = validate_logout_response(&xml, &rd(), &sls()).unwrap();
        assert!(!f.status_is_success);
    }

    #[test]
    fn rejects_wrong_root_issuer_destination_and_missing_in_response_to() {
        // Wrong root element.
        let xml = logout_response_xml("Response", RD, Some("_r"), Some(SLS), STATUS_SUCCESS);
        let err = validate_logout_response(&xml, &rd(), &sls()).unwrap_err();
        assert!(err.contains("not samlp:LogoutResponse"), "{err}");

        // Wrong issuer.
        let xml = logout_response_xml(
            "LogoutResponse",
            "urn:evil:idp",
            Some("_r"),
            Some(SLS),
            STATUS_SUCCESS,
        );
        let err = validate_logout_response(&xml, &rd(), &sls()).unwrap_err();
        assert!(err.contains("Issuer"), "{err}");

        // Mismatched Destination.
        let xml = logout_response_xml(
            "LogoutResponse",
            RD,
            Some("_r"),
            Some("https://attacker.example/sls"),
            STATUS_SUCCESS,
        );
        let err = validate_logout_response(&xml, &rd(), &sls()).unwrap_err();
        assert!(err.contains("Destination"), "{err}");

        // Missing InResponseTo.
        let xml = logout_response_xml("LogoutResponse", RD, None, Some(SLS), STATUS_SUCCESS);
        let err = validate_logout_response(&xml, &rd(), &sls()).unwrap_err();
        assert!(err.contains("InResponseTo"), "{err}");
    }

    #[test]
    fn absent_destination_is_rejected() {
        // eID §7.7.2 gives @Destination cardinality 1, so its absence is a
        // protocol violation rather than "nothing to compare".
        let xml = logout_response_xml("LogoutResponse", RD, Some("_r"), None, STATUS_SUCCESS);
        let err = validate_logout_response(&xml, &rd(), &sls()).unwrap_err();
        assert!(err.contains("missing the required @Destination"), "{err}");
    }

    #[test]
    fn rejects_missing_version_and_issue_instant() {
        // Both are cardinality 1 in the §7.7.2 table.
        let no_version =
            logout_response_xml("LogoutResponse", RD, Some("_r"), Some(SLS), STATUS_SUCCESS)
                .replace(r#" Version="2.0""#, "");
        let err = validate_logout_response(&no_version, &rd(), &sls()).unwrap_err();
        assert!(err.contains("missing the required @Version"), "{err}");

        let no_instant = logout_response_with_instant(
            "LogoutResponse",
            RD,
            Some("_r"),
            Some(SLS),
            STATUS_SUCCESS,
            None,
        );
        let err = validate_logout_response(&no_instant, &rd(), &sls()).unwrap_err();
        assert!(err.contains("@IssueInstant is missing"), "{err}");
    }

    #[test]
    fn rejects_stale_and_future_issue_instant() {
        // Without a freshness bound a captured LogoutResponse would stay
        // structurally valid indefinitely.
        for (offset, expected) in [
            (-chrono::Duration::hours(1), "stale"),
            (chrono::Duration::hours(1), "in the future"),
        ] {
            let xml = logout_response_with_instant(
                "LogoutResponse",
                RD,
                Some("_r"),
                Some(SLS),
                STATUS_SUCCESS,
                Some(&now_offset(offset)),
            );
            let err = validate_logout_response(&xml, &rd(), &sls()).unwrap_err();
            assert!(err.contains(expected), "expected {expected}, got {err}");
        }
    }

    #[test]
    fn unparseable_xml_is_rejected() {
        let err = validate_logout_response("not xml <<<", &rd(), &sls()).unwrap_err();
        assert!(err.contains("could not parse"), "{err}");
    }
}
