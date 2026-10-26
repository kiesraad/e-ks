use crate::structs::common::{
    BsnOrNoneConfirmed, CountryCode, DateOfBirth, FullName, Gender, Initials, LastName,
    LastNamePrefix, PlaceOfResidence,
};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use validate::Validate;

use crate::{
    ElectionConfig, OptionStringExt, PgStore,
    common::FullNameForm,
    constants::DEFAULT_DATE_FORMAT,
    form::{FieldErrors, FormData, MergeErrors, ValidationError},
    structs::persons::{IdentityMatch, Person, PersonalData},
};

#[derive(Default, Serialize, Deserialize, Clone, Debug, Validate)]
#[validate(target = "PersonalData")]
#[serde(default)]
pub struct PersonalDataFieldsForm {
    #[validate(parse = "Gender", optional)]
    pub gender: String,
    #[validate(parse = "DateOfBirth", optional)]
    pub date_of_birth: String,
    #[validate(parse = "BsnOrNoneConfirmed", optional)]
    pub bsn: String,
    #[validate(parse = "PlaceOfResidence", optional)]
    pub place_of_residence: String,
    #[validate(parse = "CountryCode", optional)]
    pub country: String,
}

#[derive(Default, Serialize, Deserialize, Clone, Debug, Validate)]
#[validate(target = "Person")]
#[serde(default)]
pub struct PersonalDataForm {
    #[validate(flatten)]
    #[serde(flatten)]
    pub name: FullNameForm,
    #[validate(flatten)]
    #[serde(flatten)]
    pub personal_data: PersonalDataFieldsForm,
}

impl PersonalDataFieldsForm {
    pub fn is_date_of_birth_very_old(&self) -> bool {
        self.date_of_birth
            .parse::<DateOfBirth>()
            .is_ok_and(|d| d.is_very_old())
    }

    pub fn is_date_of_birth_too_young(&self, election: &ElectionConfig) -> bool {
        self.date_of_birth
            .parse::<DateOfBirth>()
            .is_ok_and(|d| d.is_too_young(election))
    }
}

impl From<PersonalData> for PersonalDataFieldsForm {
    fn from(personal_data: PersonalData) -> Self {
        PersonalDataFieldsForm {
            gender: personal_data
                .gender
                .map(|g| g.to_string())
                .unwrap_or_default(),
            date_of_birth: personal_data
                .date_of_birth
                .map(|d| d.format(DEFAULT_DATE_FORMAT).to_string())
                .unwrap_or_default(),
            bsn: personal_data
                .bsn
                .map(|s| s.to_exposed_string())
                .unwrap_or_default(),
            place_of_residence: personal_data.place_of_residence.to_string_or_default(),
            country: personal_data.country.to_string_or_default(),
        }
    }
}

impl From<Person> for PersonalDataForm {
    fn from(person: Person) -> Self {
        PersonalDataForm {
            name: FullNameForm::from(person.name),
            personal_data: PersonalDataFieldsForm::from(person.personal_data),
        }
    }
}

impl PersonalDataForm {
    /// Also checks person uniqueness
    pub fn validate_create_with_checks(
        self,
        store: &PgStore,
    ) -> Result<Person, Box<FormData<Self>>> {
        let data = store.snapshot();
        let existing = data.get_persons();
        let person_result = self.clone().validate_create();
        let errors = self.uniqueness_errors(existing);

        person_result.merge_errors(self, errors)
    }

    /// Also checks date of birth
    pub fn validate_update_with_checks(
        self,
        current: &Person,
        store: &PgStore,
    ) -> Result<Person, Box<FormData<Self>>> {
        let data = store.snapshot();
        let existing = data.get_persons();
        let existing_without_current: Vec<&Person> = existing
            .into_iter()
            .filter(|p| p.id != current.id)
            .collect();
        let person_result = self.clone().validate_update(current);
        let errors = self.uniqueness_errors(existing_without_current);

        person_result.merge_errors(self, errors)
    }

    /// A person with the same BSN, or without a BSN the same name and date
    /// of birth, may not be entered twice.
    fn uniqueness_errors(&self, existing: Vec<&Person>) -> FieldErrors {
        let bsn = BsnOrNoneConfirmed::from_str(&self.personal_data.bsn).ok();
        let has_bsn = matches!(bsn, Some(BsnOrNoneConfirmed::Bsn(_)));

        // The outer validation reports an invalid name; a BSN identifies the
        // person without it.
        let Some(name) = self
            .identity_name()
            .or_else(|| has_bsn.then(FullName::default))
        else {
            return Vec::new();
        };

        let candidate = Person {
            name,
            personal_data: PersonalData {
                bsn,
                date_of_birth: DateOfBirth::from_str(&self.personal_data.date_of_birth).ok(),
                ..Default::default()
            },
            ..Default::default()
        };

        if !existing
            .iter()
            .any(|p| candidate.identity_match(p) == IdentityMatch::Same)
        {
            return Vec::new();
        }

        if has_bsn {
            vec![(
                "personal_data.bsn".to_string(),
                ValidationError::BsnAlreadyExists,
            )]
        } else {
            [
                "name.initials",
                "name.last_name",
                "personal_data.date_of_birth",
            ]
            .into_iter()
            .map(|field| (field.to_string(), ValidationError::NameAlreadyExists))
            .collect()
        }
    }

    /// The identifying name fields, or `None` when one does not parse.
    fn identity_name(&self) -> Option<FullName> {
        let last_name_prefix = if self.name.last_name_prefix.is_empty() {
            None
        } else {
            Some(LastNamePrefix::from_str(&self.name.last_name_prefix).ok()?)
        };

        Some(FullName {
            first_name: None,
            last_name: LastName::from_str(&self.name.last_name).ok()?,
            last_name_prefix,
            initials: Some(Initials::from_str(&self.name.initials).ok()?),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        OptionAsStrExt,
        form::ValidationError,
        structs::{
            common::{DutchAddress, UtcDateTime},
            persons::PersonId,
        },
        test_utils::{self, display_opt, parse_country_code, sample_person_with},
    };

    /// A person with known personal data and a Dutch address.
    fn current_person() -> Person {
        let mut current =
            sample_person_with(PersonId::new(), Some("Evert"), "Klaas Smit", None, "E.D.");
        current.personal_data.gender = Some(Gender::Female);
        current.personal_data.place_of_residence =
            Some(PlaceOfResidence::Known("Waterdam".to_string()));
        current.personal_data.country = Some(parse_country_code("NL"));
        current.address = DutchAddress {
            locality: Some("Heemdamseburg".parse().expect("locality")),
            postal_code: Some("1234AB".parse().expect("postal code")),
            house_number: Some("10".parse().expect("house number")),
            house_number_addition: Some("B".parse().expect("house number addition")),
            street_name: Some("Spoorstraat".parse().expect("street name")),
            known_in_bag: Some(true),
        };
        current.updated_at = UtcDateTime::default();
        current
    }

    /// A valid form for [`current_person`], with whitespace to be trimmed and
    /// a name prefix and male gender to be applied.
    fn valid_update_form() -> PersonalDataForm {
        PersonalDataForm {
            name: FullNameForm {
                first_name: " Evert ".to_string(),
                last_name: "  Klaas Smit ".to_string(),
                last_name_prefix: "  van de ".to_string(),
                initials: "E.D.".to_string(),
            },
            personal_data: PersonalDataFieldsForm {
                gender: "male".to_string(),
                date_of_birth: "01-02-2020".to_string(),
                bsn: "none-confirmed".to_string(),
                place_of_residence: "Waterdam".to_string(),
                country: " nl ".to_string(),
            },
        }
    }

    #[test]
    fn personal_data_form_updates_existing_person_when_valid() {
        let current = current_person();
        let form = valid_update_form();

        let updated = form.validate_update(&current).unwrap();

        assert_eq!(updated.id, current.id);
        assert_eq!(updated.personal_data.gender, Some(Gender::Male));
        assert_eq!(updated.name.last_name.to_string(), "Klaas Smit");
        assert_eq!(
            display_opt(&updated.name.last_name_prefix).as_deref(),
            Some("van de")
        );
        assert_eq!(
            display_opt(&updated.name.first_name).as_deref(),
            Some("Evert")
        );
        assert_eq!(updated.name.initials.clone().to_string_or_default(), "E.D.");
        assert_eq!(
            updated
                .personal_data
                .date_of_birth
                .map(|d| d.format(DEFAULT_DATE_FORMAT).to_string()),
            Some("01-02-2020".to_string())
        );
        assert_eq!(
            display_opt(&updated.personal_data.place_of_residence).as_deref(),
            Some("Waterdam")
        );
        assert_eq!(
            display_opt(&updated.personal_data.country).as_deref(),
            Some("NL")
        );
        assert_eq!(
            display_opt(&updated.address.locality).as_deref(),
            Some("Heemdamseburg")
        );
        assert_eq!(
            updated.address.postal_code.unwrap(),
            "1234AB".parse().unwrap()
        );
        assert_eq!(
            display_opt(&updated.address.house_number).as_deref(),
            Some("10")
        );
        assert_eq!(
            display_opt(&updated.address.house_number_addition).as_deref(),
            Some("B")
        );
        assert_eq!(
            display_opt(&updated.address.street_name).as_deref(),
            Some("Spoorstraat")
        );
        assert!(updated.updated_at >= current.updated_at);
    }

    /// The BRP allows a person without first names and thus without initials,
    /// but a political group has to hand in initials for every person.
    #[test]
    fn personal_data_form_rejects_empty_initials() {
        let current = current_person();
        let mut form = valid_update_form();
        form.name.initials = "  ".to_string();

        let Err(data) = form.validate_update(&current) else {
            panic!("expected validation errors");
        };

        assert_eq!(
            data.errors(),
            vec![(
                "name.initials".to_string(),
                ValidationError::ValueShouldNotBeEmpty
            )]
        );
    }

    #[test]
    fn personal_data_form_collects_validation_errors() {
        let form = PersonalDataForm {
            name: FullNameForm {
                first_name: "🤔".to_string(),
                last_name: "de Bakker".to_string(),
                last_name_prefix: "Boris".to_string(),
                initials: "jd".to_string(),
            },
            personal_data: PersonalDataFieldsForm {
                gender: "invalid".to_string(),
                date_of_birth: "2020/01/01".to_string(),
                bsn: "".to_string(),
                place_of_residence: "".to_string(),
                country: "xx".to_string(),
            },
        };

        let Err(data) = form.validate_create() else {
            panic!("expected validation errors");
        };

        let errors = data.errors();
        assert_eq!(errors.len(), 7);
        assert!(errors.contains(&(
            "personal_data.gender".to_string(),
            ValidationError::InvalidValue
        )));
        assert!(errors.contains(&(
            "name.last_name".to_string(),
            ValidationError::StartsWithLastNamePrefix
        )));
        assert!(errors.contains(&(
            "name.last_name_prefix".to_string(),
            ValidationError::InvalidValue
        )));
        assert!(errors.contains(&("name.first_name".to_string(), ValidationError::InvalidValue)));
        assert!(errors.contains(&("name.initials".to_string(), ValidationError::InvalidValue)));
        assert!(errors.contains(&(
            "personal_data.date_of_birth".to_string(),
            ValidationError::InvalidDateFormat
        )));
        assert!(errors.contains(&(
            "personal_data.country".to_string(),
            ValidationError::InvalidValue
        )));
    }

    #[test]
    fn display_helpers_behave_correctly() {
        let mut person =
            sample_person_with(PersonId::new(), Some("Evert"), "Klaas Smit", None, "E.D.");
        person.personal_data.gender = Some(Gender::Male);

        assert_eq!(person.name.display(), "Klaas Smit, E.D. (Evert)");
        assert_eq!(person.gender_key(), "common.gender.male");

        person.name.first_name = None;
        assert_eq!(person.name.first_name.as_str_or_empty(), "");
        assert_eq!(person.name.display(), "Klaas Smit, E.D.");
    }

    /// A form with the same name and date of birth as the sample person.
    fn namesake_form(bsn: &str, date_of_birth: &str) -> PersonalDataForm {
        PersonalDataForm {
            name: FullNameForm {
                first_name: "".to_string(),
                last_name: "Klaas Smit".to_string(),
                last_name_prefix: "".to_string(),
                initials: "E.D.".to_string(),
            },
            personal_data: PersonalDataFieldsForm {
                gender: "m".to_string(),
                date_of_birth: date_of_birth.to_string(),
                bsn: bsn.to_string(),
                place_of_residence: "Amsterdam".to_string(),
                country: "NL".to_string(),
            },
        }
    }

    #[test]
    fn uniqueness_errors_for_duplicate_name_and_date_of_birth_without_bsn() {
        let existing = sample_person_with(PersonId::new(), None, "Klaas Smit", None, "E.D.");

        let errors = namesake_form("", "01-02-1990").uniqueness_errors(vec![&existing]);

        assert_eq!(
            errors,
            vec![
                (
                    "name.initials".to_string(),
                    ValidationError::NameAlreadyExists
                ),
                (
                    "name.last_name".to_string(),
                    ValidationError::NameAlreadyExists
                ),
                (
                    "personal_data.date_of_birth".to_string(),
                    ValidationError::NameAlreadyExists
                ),
            ]
        );
    }

    #[test]
    fn uniqueness_allows_duplicate_name_with_other_date_of_birth() {
        let existing = sample_person_with(PersonId::new(), None, "Klaas Smit", None, "E.D.");

        let errors = namesake_form("", "12-12-1970").uniqueness_errors(vec![&existing]);

        assert!(errors.is_empty());
    }

    #[test]
    fn uniqueness_allows_duplicate_name_without_date_of_birth() {
        let existing = sample_person_with(PersonId::new(), None, "Klaas Smit", None, "E.D.");

        let errors = namesake_form("", "").uniqueness_errors(vec![&existing]);

        assert!(errors.is_empty());
    }

    #[test]
    fn uniqueness_allows_duplicate_name_when_only_existing_has_bsn() {
        let mut existing = sample_person_with(PersonId::new(), None, "Klaas Smit", None, "E.D.");
        existing.personal_data.bsn =
            Some(BsnOrNoneConfirmed::Bsn("123456782".parse().expect("bsn")));

        let errors = namesake_form("", "01-02-1990").uniqueness_errors(vec![&existing]);

        assert!(errors.is_empty());
    }

    #[test]
    fn uniqueness_errors_for_duplicate_bsn() {
        let mut existing = sample_person_with(PersonId::new(), None, "Klaas Smit", None, "E.D.");
        existing.personal_data.bsn =
            Some(BsnOrNoneConfirmed::Bsn("123456782".parse().expect("bsn")));

        let form = PersonalDataForm {
            name: FullNameForm {
                first_name: "".to_string(),
                last_name: "Other".to_string(),
                last_name_prefix: "".to_string(),
                initials: "E.D.".to_string(),
            },
            personal_data: PersonalDataFieldsForm {
                gender: "m".to_string(),
                date_of_birth: "12-12-1970".to_string(),
                bsn: "123456782".to_string(),
                place_of_residence: "Amsterdam".to_string(),
                country: "NL".to_string(),
            },
        };

        let errors = form.uniqueness_errors(vec![&existing]);

        assert_eq!(
            errors,
            vec![(
                "personal_data.bsn".to_string(),
                ValidationError::BsnAlreadyExists
            )]
        );
    }

    #[test]
    fn uniqueness_allows_duplicate_name_with_unique_bsn() {
        let mut existing = sample_person_with(PersonId::new(), None, "Klaas Smit", None, "E.D.");
        existing.personal_data.bsn =
            Some(BsnOrNoneConfirmed::Bsn("123456782".parse().expect("bsn")));

        let form = PersonalDataForm {
            name: FullNameForm {
                first_name: "".to_string(),
                last_name: "Klaas Smit".to_string(),
                last_name_prefix: "".to_string(),
                initials: "E.D.".to_string(),
            },
            personal_data: PersonalDataFieldsForm {
                gender: "m".to_string(),
                date_of_birth: "12-12-1970".to_string(),
                bsn: "111222333".to_string(),
                place_of_residence: "Amsterdam".to_string(),
                country: "NL".to_string(),
            },
        };

        let errors = form.uniqueness_errors(vec![&existing]);

        assert!(errors.is_empty());
    }

    #[tokio::test]
    async fn validate_create_with_checks_combines_errors() {
        let store = PgStore::new_for_test();

        let mut sample_person = test_utils::sample_person(PersonId::new());
        sample_person.personal_data.bsn = Some("111222333".parse().unwrap());
        sample_person.create(&store).await.unwrap();

        let mut form = test_utils::sample_person_form();
        form.personal_data.bsn = "111222333".parse().unwrap();
        form.name.last_name_prefix = "invalid".to_string();

        let form_data = form
            .validate_create_with_checks(&store)
            .expect_err("form shouldn't validate");

        let errors = form_data.errors();

        assert_eq!(errors.len(), 2);
        assert!(errors.contains(&(
            "personal_data.bsn".to_string(),
            ValidationError::BsnAlreadyExists
        )));
        assert!(errors.contains(&(
            "name.last_name_prefix".to_string(),
            ValidationError::InvalidValue
        )));
    }
}
