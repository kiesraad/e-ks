use crate::{Locale, trans};

type ActualLength = usize;
type MaxLength = usize;
type MinLength = usize;
type ActualCount = usize;
type MaxCount = usize;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ValidationError {
    InvalidValue,
    InvalidEmail,
    ValueShouldNotBeEmpty,
    ChooseAtLeastOneOption,
    ValueTooLong(ActualLength, MaxLength),
    ValueTooShort(ActualLength, MinLength),
    InvalidChecksum,
    InvalidPlaceOfResidence,
    StartsWithLastNamePrefix,
    TooManyInitials(ActualCount, MaxCount),
    InvalidPostalCode,
    NameAlreadyExists,
    AppellationAlreadyExists,
    BsnAlreadyExists,
    DateInFuture,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message(Locale::default()))
    }
}

impl ValidationError {
    pub fn message(&self, locale: Locale) -> String {
        match self {
            ValidationError::InvalidValue => trans!("validation.invalid_value", locale),
            ValidationError::InvalidEmail => trans!("validation.invalid_email", locale),
            ValidationError::ValueShouldNotBeEmpty => {
                trans!("validation.value_should_not_be_empty", locale)
            }
            ValidationError::ChooseAtLeastOneOption => {
                trans!("validation.choose_at_least_one_option", locale)
            }
            ValidationError::ValueTooLong(actual, max) => {
                trans!("validation.value_too_long", locale, actual, max)
            }
            ValidationError::ValueTooShort(1, min) => {
                trans!("validation.value_too_short_single", locale, min)
            }
            ValidationError::ValueTooShort(actual, min) => {
                trans!("validation.value_too_short_plural", locale, actual, min)
            }
            ValidationError::InvalidChecksum => trans!("validation.invalid_bsn", locale),
            ValidationError::InvalidPlaceOfResidence => {
                trans!("validation.invalid_place_of_residence", locale)
            }
            ValidationError::StartsWithLastNamePrefix => {
                trans!("validation.starts_with_last_name_prefix", locale)
            }
            ValidationError::TooManyInitials(actual, max) => {
                trans!("validation.too_many_initials", locale, actual, max)
            }
            ValidationError::InvalidPostalCode => {
                trans!("validation.invalid_postal_code", locale)
            }
            ValidationError::NameAlreadyExists => {
                trans!("validation.name_already_exists", locale)
            }
            ValidationError::AppellationAlreadyExists => {
                trans!("validation.appellation_already_exists", locale)
            }
            ValidationError::BsnAlreadyExists => trans!("validation.bsn_already_exists", locale),
            ValidationError::DateInFuture => trans!("validation.date_of_birth_in_future", locale),
        }
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_messages_in_english() {
        assert_eq!(
            ValidationError::InvalidValue.message(Locale::En),
            "The provided value is not valid."
        );
        assert_eq!(
            ValidationError::InvalidEmail.message(Locale::En),
            "Invalid email address."
        );
        assert_eq!(
            ValidationError::ValueShouldNotBeEmpty.message(Locale::En),
            "This field must not be empty."
        );
        assert_eq!(
            ValidationError::ChooseAtLeastOneOption.message(Locale::En),
            "Please choose at least one option."
        );
        assert_eq!(
            ValidationError::ValueTooLong(10, 5).message(Locale::En),
            "The value is too long (10 characters), maximum 5 characters allowed."
        );
        assert_eq!(
            ValidationError::ValueTooShort(2, 5).message(Locale::En),
            "The value is too short (2 characters), minimum 5 characters required."
        );
        assert_eq!(
            ValidationError::InvalidChecksum.message(Locale::En),
            "Invalid BSN."
        );
        assert_eq!(
            ValidationError::TooManyInitials(21, 20).message(Locale::En),
            "There are too many initials (21), maximum 20 initials allowed."
        );
        assert_eq!(
            ValidationError::StartsWithLastNamePrefix.message(Locale::En),
            "Please put the prefix in the correct field."
        );
        assert_eq!(
            ValidationError::NameAlreadyExists.message(Locale::En),
            "A person with this name already exists."
        );
        assert_eq!(
            ValidationError::BsnAlreadyExists.message(Locale::En),
            "This BSN is already in use."
        );
        assert_eq!(
            ValidationError::DateInFuture.message(Locale::En),
            "Date of birth cannot be in the future."
        );
    }

    #[test]
    fn display_uses_default_locale() {
        let message = ValidationError::InvalidEmail.to_string();
        assert!(!message.is_empty());
    }
}
