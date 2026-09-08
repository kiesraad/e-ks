use serde::{Deserialize, Serialize};

use crate::{
    ElectionConfig,
    core::{AnyLocale, Locale},
    id_newtype,
    structs::{
        common::{
            DutchAddress, FullName, Gender, HasSeverity, PotentialProblems, Problematic, Problems,
            Severity, UtcDateTime, WithProblems,
        },
        persons::PersonalData,
    },
    trans,
};

id_newtype!(pub struct PersonId);

#[derive(Default, Debug, Serialize, Eq, PartialEq, Deserialize, Clone)]
pub struct Person {
    pub id: PersonId,
    pub name: FullName,
    pub personal_data: PersonalData,
    pub address: DutchAddress,
    pub representative: Option<Representative>,
    pub updated_at: UtcDateTime,
}

pub type PersonWithProblems = WithProblems<Person>;

impl Problematic<ElectionConfig> for Person {
    fn get_problems(&self, election: ElectionConfig) -> Problems {
        Problems::merge(vec![
            self.name.get_problems(Severity::Error),
            self.personal_data.get_problems(election),
            if self.needs_representative() {
                self.representative.get_problems(self)
            } else {
                self.address.get_problems(Severity::Warn)
            },
        ])
    }
}

#[derive(Default, Debug, Eq, PartialEq, Serialize, Deserialize, Clone)]
pub struct Representative {
    pub name: FullName,
    pub address: DutchAddress,
}

impl Problematic<&Person> for Option<Representative> {
    fn get_problems(&self, associated_person: &Person) -> Problems {
        if !associated_person.needs_representative() {
            return Problems {
                potential_problems: Vec::new(),
                info_problems: Vec::new(),
            };
        }
        let Some(representative) = self else {
            return Problems {
                potential_problems: vec![PotentialProblems::NoRepresentative],
                info_problems: Vec::new(),
            };
        };

        let problems = Problems::merge(vec![
            representative.name.get_problems(Severity::Warn),
            representative.address.get_problems(Severity::Warn),
        ]);
        let potential_problems = problems
            .potential_problems
            .into_iter()
            .map(|p| PotentialProblems::RepresentativeProblem(Box::new(p)))
            .collect();

        Problems {
            potential_problems,
            info_problems: problems.info_problems,
        }
    }
}

impl Person {
    /// Returns the initials as printed on the candidate list,
    /// i.e., optionally with the first name and gender.
    ///
    /// **Examples:**
    /// - H. (Hubertus) (m)
    /// - H. (m)
    /// - H. (Hubertus)
    /// - H.
    pub fn initials_as_printed_on_list(&self, locale: AnyLocale) -> String {
        let mut initials = self.name.initials_with_first_name();
        if let Some(gender) = &self.personal_data.gender {
            initials.push_str(&format!(" ({})", gender.abbreviation(locale)));
        }
        initials
    }

    /// Returns the full name as printed on the candidate list.
    ///
    /// **Example:** van Dijk, A.B. (Anne) (v)
    pub fn name_as_printed_on_list(&self, locale: AnyLocale) -> String {
        format!(
            "{}, {}",
            self.name.last_name_with_prefix(),
            self.initials_as_printed_on_list(locale)
        )
    }

    pub fn lives_in_nl(&self) -> bool {
        self.personal_data.lives_in_nl()
    }

    /// See [`PersonalData::needs_representative`].
    pub fn needs_representative(&self) -> bool {
        self.personal_data.needs_representative()
    }

    pub fn gender_key(&self) -> &'static str {
        self.personal_data
            .gender
            .map(|g| match g {
                Gender::Male => "common.gender.male",
                Gender::Female => "common.gender.female",
            })
            .unwrap_or("")
    }

    pub fn gender_label(&self, locale: Locale) -> String {
        self.personal_data
            .gender
            .map(|g| match g {
                Gender::Male => trans!("common.gender.male", locale),
                Gender::Female => trans!("common.gender.female", locale),
            })
            .unwrap_or_default()
    }

    pub fn personal_info_class(&self, election: &ElectionConfig) -> &'static str {
        Problems::merge(vec![
            self.name.get_problems(Severity::Error),
            self.personal_data.get_problems(*election),
        ])
        .highest_severity_class()
    }

    pub fn is_representative_complete(&self) -> bool {
        self.representative.get_problems(self).is_all_good()
    }

    /// Whether this person's representative has an address not found in the BAG.
    pub fn representative_address_unknown(&self) -> bool {
        self.representative
            .as_ref()
            .is_some_and(|representative| representative.address.is_unknown())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::{
        AppError, PgStore,
        pagination::SortDirection,
        structs::{
            common::{BsnOrNoneConfirmed, CountryCode, EmptyAddressProblems, PlaceOfResidence},
            persons::PersonSort,
        },
        test_utils::{
            parse_country_code, parse_last_name_prefix, sample_person, sample_person_with,
            sample_person_with_last_name,
        },
    };

    #[tokio::test]
    async fn create_and_get_person() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let id = PersonId::new();
        let person = sample_person(id);

        person.create(&store).await?;

        let loaded = store.get_person(id)?;
        assert_eq!(loaded.id, id);
        assert_eq!(loaded.name.last_name.to_string(), "Jansen");

        Ok(())
    }

    #[tokio::test]
    async fn update_person_overwrites_fields() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let id = PersonId::new();
        let mut person = sample_person(id);

        person.create(&store).await?;

        person.name.last_name = "Updated".parse().expect("last name");
        person.update(&store).await?;

        let updated = store.get_person(id)?;
        assert_eq!(updated.name.last_name.to_string(), "Updated");

        Ok(())
    }

    #[tokio::test]
    async fn remove_person_deletes_record() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let id = PersonId::new();
        let person = sample_person(id);

        person.create(&store).await?;
        person.delete(&store).await?;

        let missing = store.get_person(id);
        assert!(missing.is_err());

        Ok(())
    }

    #[tokio::test]
    async fn update_address_overwrites_fields() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let id = PersonId::new();
        let mut person = sample_person(id);

        person.create(&store).await?;

        person.address.locality = Some("Nieuwegein".parse().expect("locality"));
        person.address.postal_code = Some("9999 ZZ".parse().expect("postal code"));
        person.address.house_number = Some("99".parse().expect("house number"));
        person.address.house_number_addition = None;
        person.address.street_name = Some("Nieuweweg".parse().expect("street name"));

        person
            .update_address(&store, person.address.clone())
            .await?;

        let updated = store.get_person(id)?;
        assert_eq!(
            updated.address.locality.as_deref().map(ToString::to_string),
            Some("Nieuwegein".to_string())
        );
        assert_eq!(
            updated.address.postal_code.unwrap(),
            "9999ZZ".parse().unwrap()
        );
        assert_eq!(
            updated
                .address
                .house_number
                .as_deref()
                .map(ToString::to_string),
            Some("99".to_string())
        );
        assert_eq!(updated.address.house_number_addition, None);
        assert_eq!(
            updated
                .address
                .street_name
                .as_deref()
                .map(ToString::to_string),
            Some("Nieuweweg".to_string())
        );

        Ok(())
    }

    #[tokio::test]
    async fn list_and_count_persons() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        sample_person_with_last_name(PersonId::new(), "Jansen")
            .create(&store)
            .await?;
        sample_person_with_last_name(PersonId::new(), "Bakker")
            .create(&store)
            .await?;

        let total = store.get_person_count();
        assert_eq!(total, 2);

        let persons = Person::list(&store, 10, 0, &PersonSort::LastName, &SortDirection::Asc)?;
        assert_eq!(persons.len(), 2);
        assert_eq!(persons[0].data.name.last_name.to_string(), "Bakker");

        Ok(())
    }

    #[test]
    fn last_name_formats_with_optional_prefix() {
        let mut person = sample_person_with(PersonId::new(), None, "Dijk", None, "A.B.");
        assert_eq!(person.name.last_name_with_prefix(), "Dijk");
        assert_eq!(person.name.last_name_with_prefix_appended(), "Dijk");

        person.name.last_name_prefix = Some(parse_last_name_prefix("van"));
        assert_eq!(person.name.last_name_with_prefix(), "van Dijk");
        assert_eq!(person.name.last_name_with_prefix_appended(), "Dijk, van");
    }

    #[test]
    fn appellation_shows_first_name_when_present() {
        let mut person =
            sample_person_with(PersonId::new(), Some("Anne"), "Dijk", Some("van"), "A.B.");
        assert_eq!(person.name.display(), "van Dijk, A.B. (Anne)");

        person.name.first_name = None;
        assert_eq!(person.name.display(), "van Dijk, A.B.");
    }

    #[test]
    fn lives_in_nl_defaults_to_true_and_accepts_variants() {
        let mut person = sample_person(PersonId::new());
        person.personal_data.country = None;
        assert!(person.lives_in_nl());

        person.personal_data.country = Some(parse_country_code("NL"));
        assert!(person.lives_in_nl());

        person.personal_data.country = Some(parse_country_code("BE"));
        assert!(!person.lives_in_nl());
    }

    #[test]
    fn gender_key_returns_translations_or_empty_keys() {
        let mut person = sample_person(PersonId::new());
        person.personal_data.gender = None;
        assert_eq!(person.gender_key(), "");

        person.personal_data.gender = Some(Gender::Male);
        assert_eq!(person.gender_key(), "common.gender.male");

        person.personal_data.gender = Some(Gender::Female);
        assert_eq!(person.gender_key(), "common.gender.female");
    }

    fn complete_address() -> DutchAddress {
        DutchAddress {
            locality: Some("Utrecht".parse().expect("locality")),
            postal_code: Some("1234 AB".parse().expect("postal code")),
            house_number: Some("10".parse().expect("house number")),
            house_number_addition: None,
            street_name: Some("Stationsstraat".parse().expect("street name")),
            known_in_bag: Some(true),
        }
    }

    fn complete_representative() -> Representative {
        Representative {
            name: FullName {
                first_name: Some("Anne".parse().expect("first name")),
                last_name: "Dijk".parse().expect("last name"),
                last_name_prefix: None,
                initials: "A.B.".parse().expect("initials"),
            },
            address: complete_address(),
        }
    }

    #[test]
    fn complete_person_has_no_problems() {
        let problems = sample_person(PersonId::new()).get_problems(ElectionConfig::EK27);
        assert!(problems.potential_problems.is_empty());
        assert!(problems.info_problems.is_empty());
    }

    #[test]
    fn missing_last_name_produces_error() {
        let mut person = sample_person(PersonId::new());
        person.name = FullName::default();
        assert!(
            person
                .get_problems(ElectionConfig::EK27)
                .potential_problems
                .contains(&PotentialProblems::NoLastName(Severity::Error))
        );
    }

    #[test]
    fn non_dutch_person_without_representative_produces_warning() {
        let mut person = sample_person(PersonId::new());
        person.personal_data.country = Some("BE".parse().expect("country code"));
        person.representative = None;
        assert!(
            person
                .get_problems(ElectionConfig::EK27)
                .potential_problems
                .contains(&PotentialProblems::NoRepresentative)
        );
    }

    #[test]
    fn caribbean_nl_person_without_representative_produces_warning() {
        let mut person = sample_person(PersonId::new());
        person.personal_data.place_of_residence =
            Some(PlaceOfResidence::Known("Kralendijk".to_string()));
        // the (empty) correspondence address must not be flagged: a
        // representative is required instead
        person.address = DutchAddress::default();

        let problems = person.get_problems(ElectionConfig::EK27);
        assert!(
            problems
                .potential_problems
                .contains(&PotentialProblems::NoRepresentative)
        );

        person.representative = Some(complete_representative());
        assert!(person.get_problems(ElectionConfig::EK27).is_all_good());
    }

    #[test]
    fn non_dutch_person_with_incomplete_representative_produces_wrapped_problems() {
        let mut person = sample_person(PersonId::new());
        person.personal_data.country = Some("BE".parse().expect("country code"));
        person.representative = Some(Representative::default());
        let problems = person.get_problems(ElectionConfig::EK27);
        assert!(
            problems
                .potential_problems
                .iter()
                .any(|p| matches!(p, PotentialProblems::RepresentativeProblem(_)))
        );
        assert!(
            !problems
                .potential_problems
                .contains(&PotentialProblems::NoRepresentative)
        );
    }

    #[test]
    fn non_dutch_person_with_representative_address_unknown_in_bag_produces_wrapped_problem() {
        let mut person = sample_person(PersonId::new());
        person.personal_data.country = Some("BE".parse().expect("country code"));
        let mut representative = complete_representative();
        representative.address.known_in_bag = Some(false);
        person.representative = Some(representative);
        let problems = person.get_problems(ElectionConfig::EK27);
        // the representative's unknown address is surfaced through the wrapper
        assert!(
            problems
                .potential_problems
                .contains(&PotentialProblems::RepresentativeProblem(Box::new(
                    PotentialProblems::UnknownAddress
                )))
        );
    }

    #[test]
    fn dutch_person_does_not_require_representative() {
        let mut person = sample_person(PersonId::new());
        person.representative = None;
        assert!(
            !person
                .get_problems(ElectionConfig::EK27)
                .potential_problems
                .contains(&PotentialProblems::NoRepresentative)
        );
    }

    #[test]
    fn dutch_person_with_incomplete_address_produces_warnings() {
        let mut person = sample_person(PersonId::new());
        person.address = DutchAddress::default();
        let problems = person.get_problems(ElectionConfig::EK27);
        assert!(problems.potential_problems.iter().any(|pp| match pp {
            PotentialProblems::IncompleteAddress {
                severity: Severity::Warn,
                problems,
            } => {
                problems.contains(&EmptyAddressProblems::StreetName)
            }
            _ => false,
        }));

        assert!(problems.potential_problems.iter().any(|pp| match pp {
            PotentialProblems::IncompleteAddress {
                severity: Severity::Warn,
                problems,
            } => {
                problems.contains(&EmptyAddressProblems::Locality)
            }
            _ => false,
        }));
    }

    #[test]
    fn non_dutch_person_with_representative_does_not_check_address() {
        let mut person = sample_person(PersonId::new());
        person.personal_data.country = Some("BE".parse().expect("country code"));
        person.address = DutchAddress::default();
        person.representative = Some(complete_representative());
        let problems = person.get_problems(ElectionConfig::EK27);
        assert!(
            !problems
                .potential_problems
                .contains(&PotentialProblems::IncompleteAddress {
                    severity: Severity::Warn,
                    problems: vec![EmptyAddressProblems::StreetName]
                })
        );
        assert!(
            !problems
                .potential_problems
                .contains(&PotentialProblems::NoRepresentative)
        );
    }

    #[test]
    fn representative_is_complete_requires_name_and_address() {
        let mut person = sample_person(PersonId::new());
        person.personal_data.country = CountryCode::from_str("NL").ok();
        let mut representative = complete_representative();

        assert!(
            Some(representative.clone())
                .get_problems(&person)
                .is_all_good()
        );

        representative.address = DutchAddress::default();
        person.personal_data.country = CountryCode::from_str("BE").ok();
        assert!(!Some(representative).get_problems(&person).is_all_good());
    }

    #[test]
    fn personal_info_complete_requires_core_fields() {
        let mut person = sample_person(PersonId::new());
        person.personal_data.bsn = None;
        assert_eq!(person.personal_info_class(&ElectionConfig::EK27), "warning");

        person.personal_data.bsn = Some(BsnOrNoneConfirmed::Bsn("999995972".parse().expect("bsn")));
        assert_eq!(person.personal_info_class(&ElectionConfig::EK27), "ok");

        person.personal_data.date_of_birth = None;
        assert_eq!(person.personal_info_class(&ElectionConfig::EK27), "error");
    }

    #[test]
    fn representative_complete_depends_on_country() {
        let mut person = sample_person(PersonId::new());
        assert!(person.is_representative_complete());

        person.personal_data.country = Some("BE".parse().expect("country code"));
        assert!(!person.is_representative_complete());

        person.representative = Some(complete_representative());
        assert!(person.is_representative_complete());
    }

    #[test]
    fn person_complete_handles_dutch_and_non_dutch_requirements() {
        let mut dutch_person = sample_person(PersonId::new());
        dutch_person.personal_data.bsn =
            Some(BsnOrNoneConfirmed::Bsn("999995972".parse().expect("bsn")));
        assert!(
            dutch_person
                .get_problems(ElectionConfig::EK27)
                .is_all_good()
        );

        let mut non_dutch_person = sample_person(PersonId::new());
        non_dutch_person.personal_data.bsn =
            Some(BsnOrNoneConfirmed::Bsn("999995972".parse().expect("bsn")));
        non_dutch_person.personal_data.country = Some("BE".parse().expect("country code"));
        non_dutch_person.address = DutchAddress::default();
        assert!(
            !non_dutch_person
                .get_problems(ElectionConfig::EK27)
                .is_all_good()
        );

        non_dutch_person.representative = Some(complete_representative());
        assert!(
            non_dutch_person
                .get_problems(ElectionConfig::EK27)
                .is_all_good()
        );
    }
}
