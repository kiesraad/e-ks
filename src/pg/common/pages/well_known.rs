use axum_extra::routing::TypedPath;
use jiff::civil::{Date, date};
use serde::Deserialize;

#[derive(TypedPath, Deserialize)]
#[typed_path("/security.txt")]
pub struct SecurityTxt {}

/// Expires at midnight UTC at the start of this date.
const SECURITY_EXPIRATION: Date = date(2027, 2, 1);

/// Emit a security.txt file, for background information see <https://github.com/securitytxt/security-txt>
pub(super) async fn security_txt(_: SecurityTxt) -> String {
    format!(
        "Expires: {SECURITY_EXPIRATION}T00:00:00Z
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
        let now = Timestamp::now().to_zoned_datetime(TimeZone::UTC);
        let expiration = SECURITY_EXPIRATION
            .to_zoned_datetime(TimeZone::UTC)
            .unwrap();
        assert!(expiration > &now + 7.days());
        assert!(expiration < &now + 12.months());

        // Nice idea from Michiel: make the unit test deliberately flaky
        // if the expiration date is soon to expire.
        if option_env!("EKS_CHECK_EXPIRATION") == Some("1") && expiration < &now + 3.months() {
            panic!("SECURITY_EXPIRATION will expire within three months, please update it");
        }
    }
}
