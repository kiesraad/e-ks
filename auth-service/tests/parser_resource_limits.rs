//! The parser's fail-closed resource limits, exercised through the entry point
//! an unauthenticated caller actually reaches.
//!
//! `verify_xml_signature` is the first thing the SLS endpoint
//! (`POST /saml/sp/logout`, no session required) does with a posted
//! `SAMLResponse`, before any signature or trust check. Whatever it parses,
//! anyone on the internet chose.

use auth_service::saml::verification::{ExpectedRoot, verify_xml_signature};

const NS_SAMLP: &str = "urn:oasis:names:tc:SAML:2.0:protocol";

/// tokio's default worker-thread stack. The handler runs on one of these, so a
/// parser that spends stack per nesting level has this much rope.
const WORKER_STACK: usize = 2 * 1024 * 1024;

fn logout_response_nested(depth: usize, closed: bool) -> String {
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

/// Run `body` on a thread with a production-sized stack.
///
/// A stack overflow aborts the whole process rather than unwinding, so this
/// cannot be caught: if the limit regresses, the test binary dies outright and
/// the run fails. That abort *is* the assertion.
fn on_worker_sized_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(WORKER_STACK)
        .spawn(body)
        .expect("spawn")
        .join()
        .expect("thread must not die");
}

/// SECURITY (DoS): deeply nested XML must be refused by the parser rather than
/// recursed over. ~28 KB used to be enough to abort the process and take every
/// in-flight request with it; no key, no session and no valid signature needed.
#[test]
fn deeply_nested_logout_response_is_rejected_not_fatal() {
    on_worker_sized_stack(|| {
        let xml = logout_response_nested(50_000, true);
        let result = verify_xml_signature(
            &xml,
            &[],
            &ExpectedRoot {
                namespace: NS_SAMLP,
                local_name: "LogoutResponse",
                id: None,
            },
        );
        assert!(!result.is_valid());
        let errors = result.errors.join("; ");
        assert!(errors.contains("depth"), "rejected for the wrong reason: {errors}");
    });
}

/// The same attack without the closing tags: malformed, so it was always going
/// to be rejected, but at 3 bytes per level it is the cheapest version to send
/// and the rejection used to come only after the tokenizer had recursed to the
/// bottom.
#[test]
fn unclosed_deep_nesting_is_rejected_not_fatal() {
    on_worker_sized_stack(|| {
        let xml = logout_response_nested(50_000, false);
        assert!(xml.len() < 256 * 1024, "cheap to send: {} bytes", xml.len());
        let result = verify_xml_signature(
            &xml,
            &[],
            &ExpectedRoot {
                namespace: NS_SAMLP,
                local_name: "LogoutResponse",
                id: None,
            },
        );
        assert!(!result.is_valid());
    });
}
