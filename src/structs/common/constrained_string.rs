use crate::structs::common::{PotentialProblems, Problematic, Problems};

/// Define string types constrained in the FromStr implementation: trimmed, at
/// most `max` bytes, teletex characters only (which excludes all control
/// characters). With `multiline = true` (textarea fields) line breaks are
/// allowed on top of that, with `\r\n` (as submitted by a textarea) normalized
/// to `\n`.
macro_rules! constrained_strings {
    (@parse false, $value:ident, $max:expr, $name:ident) => {{
        let trimmed_value = $crate::form::validate_length($value, 1, $max)?;
        $crate::form::validate_teletex_chars(&trimmed_value)?;
        Ok($name(trimmed_value))
    }};
    (@parse true, $value:ident, $max:expr, $name:ident) => {{
        let normalized = $value.replace("\r\n", "\n");
        let trimmed_value = $crate::form::validate_length(&normalized, 1, $max)?;
        $crate::form::validate_multi_line_teletex_chars(&trimmed_value)?;
        Ok($name(trimmed_value))
    }};
    ($($(#[$meta:meta])* $vis:vis struct $name:ident(max = $max:expr, multiline = $multiline:tt);)*) => {
        $(
            $crate::transparent_string! {
                $(#[$meta])*
                $vis struct $name(String);
            }

            impl std::str::FromStr for $name {
                type Err = $crate::form::ValidationError;

                fn from_str(value: &str) -> Result<Self, Self::Err> {
                    constrained_strings!(@parse $multiline, value, $max, $name)
                }
            }
        )*
    };
}
pub(crate) use constrained_strings;

constrained_strings! {
    pub struct FirstName(max = 200, multiline = false);
    pub struct LegalName(max = 200, multiline = false);
    pub struct CommitteeMemberName(max = 200, multiline = false);
    pub struct StreetName(max = 200, multiline = false);
    pub struct StateOrProvince(max = 200, multiline = false);
    /// Free text the audit log entries are searched for.
    pub struct AuditLogSearch(max = 200, multiline = false);
}

impl Problematic<()> for LegalName {
    fn get_problems(&self, _: ()) -> Problems {
        let potential_problems = if self.to_string().is_empty() {
            vec![PotentialProblems::NoLegalName]
        } else {
            Vec::new()
        };

        Problems {
            potential_problems,
            info_problems: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::form::ValidationError;

    constrained_strings! {
        struct TestText(max = 30, multiline = true);
    }

    #[test]
    fn single_char_is_valid() {
        assert_eq!(Ok(FirstName("A".to_string())), FirstName::from_str("A"));
        assert_eq!(Ok(LegalName("A".to_string())), LegalName::from_str("A"));
    }

    #[test]
    fn empty_is_rejected() {
        assert_eq!(
            Err(ValidationError::ValueShouldNotBeEmpty),
            LegalName::from_str("   ")
        );
    }

    #[test]
    fn too_long() {
        let long = "a".repeat(201);
        assert_eq!(
            Err(ValidationError::ValueTooLong(201, 200)),
            LegalName::from_str(&long)
        );
    }

    #[test]
    fn line_breaks_are_rejected() {
        assert_eq!(
            Err(ValidationError::InvalidValue),
            LegalName::from_str("regel\néinde")
        );
    }

    #[test]
    fn multi_line_allows_and_normalizes_line_breaks() {
        assert_eq!(
            Ok(TestText("regel één\nregel twee".to_string())),
            TestText::from_str("regel één\r\nregel twee")
        );
    }

    #[test]
    fn multi_line_rejects_other_control_chars() {
        assert_eq!(
            Err(ValidationError::InvalidValue),
            TestText::from_str("tab\there")
        );
        assert_eq!(
            Err(ValidationError::InvalidValue),
            TestText::from_str("los\rreturn")
        );
    }

    #[test]
    fn multi_line_enforces_length_and_non_empty() {
        assert_eq!(
            Err(ValidationError::ValueShouldNotBeEmpty),
            TestText::from_str(" \r\n ")
        );
        assert_eq!(
            Err(ValidationError::ValueTooLong(31, 30)),
            TestText::from_str(&"a".repeat(31))
        );
    }
}
