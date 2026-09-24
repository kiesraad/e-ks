use crate::structs::common::{
    BsnOrNoneConfirmed, CountryCode, DateOfBirth, Gender, Initials, LastName, LastNamePrefix,
    PlaceOfResidence,
};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use validate::Validate;

use crate::{
    ElectionConfig, OptionStringExt, PgStore,
    common::FullNameForm,
    constants::DEFAULT_DATE_FORMAT,
    form::{FieldErrors, FormData, MergeErrors, ValidationError},
    structs::persons::{Person, PersonalData},
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
        let existing = store.get_persons();
        let existing_ref = existing.iter().collect();
        let person_result = self.clone().validate_create();
        let errors = self.uniqueness_errors(existing_ref);

        person_result.merge_errors(self, errors)
    }

    /// Also checks date of birth
    pub fn validate_update_with_checks(
        self,
        current: &Person,
        store: &PgStore,
    ) -> Result<Person, Box<FormData<Self>>> {
        let existing = store.get_persons();
        let existing_without_current: Vec<&Person> =
            existing.iter().filter(|p| *p != current).collect();
        let person_result = self.clone().validate_update(current);
        let errors = self.uniqueness_errors(existing_without_current);

        person_result.merge_errors(self, errors)
    }

    /// Validate that the BSN is unique OR the name (initials, prefix, lastname) is unique
    fn uniqueness_errors(&self, existing: Vec<&Person>) -> FieldErrors {
        let mut errors = Vec::new();

        // If a BSN is set, validate BSN uniqueness and skip name uniqueness checks
        if let Ok(BsnOrNoneConfirmed::Bsn(bsn)) =
            BsnOrNoneConfirmed::from_str(&self.personal_data.bsn)
        {
            if existing.iter().any(|existing_person| {
                existing_person.personal_data.bsn == Some(BsnOrNoneConfirmed::Bsn(bsn.clone()))
            }) {
                errors.push((
                    "personal_data.bsn".to_string(),
                    ValidationError::BsnAlreadyExists,
                ));
            }

            return errors;
        }

        if let Ok(last_name) = LastName::from_str(&self.name.last_name) {
            let initials = if self.name.initials.is_empty() {
                None
            } else {
                match Initials::from_str(&self.name.initials) {
                    Ok(initials) => Some(initials),
                    // Reported by the outer validation, like the prefix.
                    Err(_) => return Vec::new(),
                }
            };
            let last_name_prefix = if self.name.last_name_prefix.is_empty() {
                None
            } else {
                match LastNamePrefix::from_str(&self.name.last_name_prefix) {
                    Ok(prefix) => Some(prefix),
                    // Prefix validation will be handled by the outer validation
                    // we can ignore parsing errors here for the purpose of uniqueness checks
                    Err(_) => return Vec::new(),
                }
            };

            let has_duplicate_name = existing.iter().any(|p| {
                p.name.initials == initials
                    && p.name.last_name_prefix == last_name_prefix
                    && p.name.last_name == last_name
            });

            if has_duplicate_name {
                errors.push((
                    "name.initials".to_string(),
                    ValidationError::NameAlreadyExists,
                ));
                errors.push((
                    "name.last_name".to_string(),
                    ValidationError::NameAlreadyExists,
                ));
            }
        }

        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        OptionAsStrExt,
        form::ValidationError,
        structs::{
            common::{DutchAddress, PotentialProblems, Problematic, Severity, UtcDateTime},
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

    /// The BRP allows a person without first names, so a form without
    /// initials is valid; the missing initials are a warning on the person.
    #[test]
    fn personal_data_form_accepts_empty_initials() {
        let current = current_person();
        let mut form = valid_update_form();
        form.name.initials = "  ".to_string();

        let updated = form.validate_update(&current).unwrap();

        assert_eq!(updated.name.initials, None);
        let problems = updated.name.get_problems(Severity::Error);
        assert_eq!(
            problems.potential_problems,
            vec![PotentialProblems::NoInitials(Severity::Warn)]
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

    #[test]
    fn uniqueness_errors_for_duplicate_name_without_bsn() {
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
                bsn: "".to_string(),
                place_of_residence: "Amsterdam".to_string(),
                country: "NL".to_string(),
            },
        };

        let errors = form.uniqueness_errors(vec![&existing]);

        assert!(errors.contains(&(
            "name.initials".to_string(),
            ValidationError::NameAlreadyExists
        )));
        assert!(errors.contains(&(
            "name.last_name".to_string(),
            ValidationError::NameAlreadyExists
        )));
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
