use axum_extra::routing::TypedPath;
use jiff::Timestamp;
use serde::Deserialize;

#[derive(TypedPath, Deserialize)]
#[typed_path("/security.txt")]
pub struct SecurityTxt {}

/// Midnight UTC at the start of 1 February 2027.
const SECURITY_EXPIRATION: Timestamp = Timestamp::constant(1_801_440_000, 0);

/// Emit a security.txt file, for background information see <https://github.com/securitytxt/security-txt>
pub(super) async fn security_txt(_: SecurityTxt) -> String {
    let date_str = SECURITY_EXPIRATION.strftime("%Y-%m-%dT%H:%M:%SZ");

    format!(
        "Expires: {date_str}
Canonical: https://kandidaatstellen.kiesraad.nl/.well-known/security.txt

Policy: https://code.overheid.nl/Kiesraad/e-KS/src/branch/main/SECURITY.md

Contact: mailto:security@kiesraad.nl
Preferred-Languages: en, nl

Hiring: https://www.werkenvoornederland.nl
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_utils::response_body_string;
    use axum::response::IntoResponse;
    use jiff::{Timestamp, ToSpan, tz::TimeZone};

    #[tokio::test]
    async fn security_renders_text() {
        // This test is almost useless, except that it forces (at compile time, most likely)
        // that security.txt is a text/plain.
        let text = security_txt(SecurityTxt {}).await;
        let body = response_body_string(text.clone().into_response()).await;
        assert_eq!(body, text);
    }

    #[test]
    fn security_is_not_stale() {
        // This tests that EXPIRATION is in the near future, and,
        // per RFC9116, is not more than one year in the future
        assert_eq!(SECURITY_EXPIRATION.to_string(), "2027-02-01T00:00:00Z");

        let now = Timestamp::now().to_zoned_datetime(TimeZone::UTC);
        let expiration = SECURITY_EXPIRATION.to_zoned_datetime(TimeZone::UTC);
        assert!(expiration > &now + 7.days());
        assert!(expiration < &now + 12.months());

        // Nice idea from Michiel: make the unit test deliberately flaky
        // if the expiration date is soon to expire.
        if option_env!("EKS_CHECK_EXPIRATION") == Some("1") && expiration < &now + 3.months() {
            panic!("SECURITY_EXPIRATION will expire within three months, please update it");
        }
    }
}
