//! Input data types shared by the PDF models, and their conversions from the
//! application store types.
//!
//! Type-checked example values live in `super::examples`.

use chrono::NaiveDate;
use tracing::error;

use crate::{
    AppError, ElectionConfig,
    core::{ElectionType, ModelLocale},
    structs::{
        candidate_lists::CandidateList,
        candidates::Candidate as AppCandidate,
        common::{Address, BsnOrNoneConfirmed},
        list_submitters::ListSubmitter,
        name_authorisations::NameAuthorisation as AppNameAuthorisation,
        persons::Representative,
    },
};

/// Input data shared by the H models.
#[derive(Debug, Clone)]
pub struct ModelData {
    pub election_name: String,
    pub election_type: ElectionType,
    pub appellation: String,
    pub candidates: Vec<Candidate>,
    pub locale: ModelLocale,
    pub event_id: usize,
    pub sha_hash: String,
}

/// The electoral districts a candidate list applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElectoralDistricts {
    All,
    Some(Vec<String>),
    /// The election has only one district, so the models omit the section.
    OnlyOne,
}

impl ElectoralDistricts {
    pub fn from(list: &CandidateList, election_config: &ElectionConfig) -> Self {
        if election_config.has_only_one_district() {
            ElectoralDistricts::OnlyOne
        } else if list.contains_all_districts(election_config) {
            ElectoralDistricts::All
        } else {
            ElectoralDistricts::Some(
                list.electoral_districts
                    .iter()
                    .map(|d| d.title().to_string())
                    .collect(),
            )
        }
    }
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub last_name: String,
    /// Initials as printed on the model, e.g., optionally including the gender
    /// and first name
    pub initials: String,
    pub date_of_birth: NaiveDate,
    pub locality: String,
    pub position: usize,
}

impl Candidate {
    pub fn try_from(candidate: &AppCandidate, locale: ModelLocale) -> Result<Self, AppError> {
        Ok(Self {
            last_name: candidate.person.name.last_name_with_prefix(),
            initials: candidate.person.initials_as_printed_on_list(locale.into()),
            date_of_birth: candidate
                .person
                .personal_data
                .date_of_birth
                .clone()
                .ok_or(AppError::IncompleteData("Missing birth date for candidate"))?
                .into(),
            locality: candidate
                .person
                .personal_data
                .locality()
                .clone()
                .ok_or(AppError::IncompleteData("Missing locality for candidate"))?
                .to_string(),
            position: candidate.position,
        })
    }
}

/// Sort the candidates by position, check the list has no holes, and map them
/// to model inputs.
pub fn ordered_candidates(
    candidates: &mut [AppCandidate],
    locale: ModelLocale,
) -> Result<Vec<Candidate>, AppError> {
    candidates.sort_by_key(|c| c.position);

    for (i, candidate) in candidates.iter().enumerate() {
        if candidate.position != i + 1 {
            error!(
                expected_position = i + 1,
                actual_position = candidate.position,
                person_id = %candidate.person.id,
                "Found a hole in candidate list",
            );
            return Err(AppError::IntegrityViolation);
        }
    }

    candidates
        .iter()
        .map(|c| Candidate::try_from(c, locale))
        .collect::<Result<Vec<_>, _>>()
}

#[derive(Debug, Clone)]
pub struct Person {
    pub last_name: String,
    /// Initials as printed on the model, e.g., optionally including the first name
    pub initials: String,
    /// Optional in the inputs: H 3 only prints the submitter's name, so this is
    /// left at its default there.
    pub postal_address: PostalAddress,
}

impl From<ListSubmitter> for Person {
    fn from(submitter: ListSubmitter) -> Self {
        Person {
            last_name: submitter.name.last_name_with_prefix(),
            initials: submitter.name.initials_with_first_name(),
            postal_address: (&submitter.address).into(),
        }
    }
}

impl From<&Representative> for Person {
    fn from(representative: &Representative) -> Self {
        Person {
            last_name: representative.name.last_name_with_prefix(),
            initials: representative.name.initials_with_first_name(),
            postal_address: (&Address::Dutch(representative.address.clone())).into(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PostalAddress {
    pub street_address: String,
    pub postal_code: String,
    pub locality: String,
}

impl From<&Address> for PostalAddress {
    fn from(address: &Address) -> Self {
        // Incomplete postal addresses cause warnings but not prevent export
        PostalAddress {
            street_address: address.address_line_1().unwrap_or_default(),
            postal_code: address.postal_code().unwrap_or_default(),
            locality: address.locality().unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct NameAuthorisation {
    pub last_name: String,
    /// Initials as printed on the model, e.g., optionally including the first name
    pub initials: String,
    pub legal_name: String,
}

impl NameAuthorisation {
    /// The representative's name as printed on the model: the non-empty parts
    /// of the last name and initials, comma-separated.
    pub fn name(&self) -> String {
        [self.last_name.as_str(), self.initials.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl From<&AppNameAuthorisation> for NameAuthorisation {
    fn from(name_authorisation: &AppNameAuthorisation) -> Self {
        NameAuthorisation {
            last_name: name_authorisation.name.last_name_with_prefix(),
            initials: name_authorisation.name.initials_with_first_name(),
            legal_name: name_authorisation.legal_name.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DetailedCandidate {
    pub candidate: Candidate,
    pub initials_no_gender: String,
    pub bsn: Option<String>,
    pub representative: Option<Person>,
    pub needs_representative: bool,
    pub postal_address: Option<PostalAddress>,
}

impl DetailedCandidate {
    pub fn try_from(candidate: &AppCandidate, locale: ModelLocale) -> Result<Self, AppError> {
        let needs_representative = candidate.person.needs_representative();
        let (representative, postal_address) = if needs_representative {
            (
                candidate.person.representative.as_ref().map(Person::from),
                None,
            )
        } else {
            (
                None,
                Some(PostalAddress::from(&Address::Dutch(
                    candidate.person.address.clone(),
                ))),
            )
        };

        // BSNs cause warnings but don't prevent export
        let bsn = match candidate.person.personal_data.bsn.as_ref() {
            Some(BsnOrNoneConfirmed::Bsn(bsn)) => Some(bsn.to_exposed_string()),
            _ => None,
        };

        Ok(Self {
            candidate: Candidate::try_from(candidate, locale)?,
            initials_no_gender: candidate.person.name.initials_with_first_name(),
            bsn,
            representative,
            needs_representative,
            postal_address,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::str::FromStr;

    use super::*;
    use crate::{
        ElectoralDistrict,
        core::election::WaterCouncil,
        structs::{
            candidate_lists::CandidateListId,
            common::{
                CountryCode, DutchAddress, FullName, HouseNumber, Initials, LastName, Locality,
                PostalCode, StreetName,
            },
            list_submitters::ListSubmitterId,
            persons::PersonId,
        },
        test_utils::{sample_list_submitter, sample_person, sample_person_with_last_name},
    };

    fn sample_candidate(position: usize) -> AppCandidate {
        AppCandidate {
            list_id: CandidateListId::new(),
            position,
            person: sample_person(PersonId::new()),
        }
    }

    #[test]
    fn ordered_candidates_sorts_and_maps_people() -> Result<(), AppError> {
        let list_id = CandidateListId::new();
        let person_a = sample_person_with_last_name(PersonId::new(), "Alpha");
        let person_b = sample_person_with_last_name(PersonId::new(), "Beta");

        let mut candidates = vec![
            AppCandidate {
                list_id,
                position: 2,
                person: person_a,
            },
            AppCandidate {
                list_id,
                position: 1,
                person: person_b,
            },
        ];

        let ordered = ordered_candidates(&mut candidates, ModelLocale::Nl)?;

        assert_eq!(ordered.len(), 2);
        assert_eq!(ordered[0].last_name, "Beta");
        assert_eq!(ordered[1].last_name, "Alpha");
        assert_eq!(
            ordered[0].date_of_birth,
            chrono::NaiveDate::from_ymd_opt(1990, 2, 1).unwrap()
        );
        assert_eq!(ordered[0].locality, "Juinen");

        Ok(())
    }

    #[test]
    fn ordered_candidates_returns_error_on_hole() {
        let mut candidates = vec![sample_candidate(1), sample_candidate(3)];

        let err = ordered_candidates(&mut candidates, ModelLocale::Nl).unwrap_err();
        assert!(matches!(err, AppError::IntegrityViolation));
    }

    #[test]
    fn candidate_requires_birth_date() {
        let mut candidate = sample_candidate(18);
        candidate.person.personal_data.date_of_birth = None;

        let err = Candidate::try_from(&candidate, ModelLocale::Nl).unwrap_err();
        assert!(matches!(
            err,
            AppError::IncompleteData("Missing birth date for candidate")
        ));
    }

    #[test]
    fn candidate_requires_locality() {
        let mut candidate = sample_candidate(18);
        candidate.person.personal_data.place_of_residence = None;

        let err = Candidate::try_from(&candidate, ModelLocale::Nl).unwrap_err();
        assert!(matches!(
            err,
            AppError::IncompleteData("Missing locality for candidate")
        ));
    }

    #[test]
    fn dutch_candidate_with_postal_address() {
        let mut candidate = sample_candidate(1);
        candidate.person.personal_data.country = Some(CountryCode::from_str("NL").unwrap());
        let detailed_candidate = DetailedCandidate::try_from(&candidate, ModelLocale::Nl).unwrap();

        assert_eq!(
            detailed_candidate.postal_address.unwrap().postal_code,
            candidate.person.address.postal_code.unwrap().to_string()
        );
        assert!(detailed_candidate.representative.is_none());
    }

    #[test]
    fn non_dutch_candidate_with_representative() {
        let mut candidate = sample_candidate(1);
        candidate.person.personal_data.country = Some(CountryCode::from_str("BE").unwrap());
        candidate.person.representative = Some(Representative {
            name: FullName {
                first_name: Some("Anne".parse().unwrap()),
                last_name: LastName::from_str("Dijk").unwrap(),
                last_name_prefix: None,
                initials: Initials::from_str("A.B.").unwrap(),
            },
            address: DutchAddress {
                street_name: Some(StreetName::from_str("street name").unwrap()),
                house_number: Some(HouseNumber::from_str("4").unwrap()),
                house_number_addition: None,
                postal_code: Some(PostalCode::from_str("1234AB").unwrap()),
                locality: Some(Locality::from_str("Amsterdam").unwrap()),
                known_in_bag: Some(true),
            },
        });
        let detailed_candidate = DetailedCandidate::try_from(&candidate, ModelLocale::Nl).unwrap();

        assert!(detailed_candidate.postal_address.is_none());
        assert_eq!(
            detailed_candidate.representative.unwrap().last_name,
            candidate
                .person
                .representative
                .as_ref()
                .unwrap()
                .name
                .last_name_with_prefix()
        );
    }

    #[test]
    fn dutch_candidate_without_postal_address() {
        let mut candidate = sample_candidate(1);
        candidate.person.personal_data.country = Some(CountryCode::from_str("NL").unwrap());
        candidate.person.address.street_name = None;
        let detailed_candidate = DetailedCandidate::try_from(&candidate, ModelLocale::Nl).unwrap();
        assert_eq!(
            detailed_candidate.postal_address.unwrap().street_address,
            ""
        );
    }

    #[test]
    fn non_dutch_candidate_without_representative() {
        let mut candidate = sample_candidate(1);
        candidate.person.personal_data.country = Some(CountryCode::from_str("BE").unwrap());
        let detailed_candidate = DetailedCandidate::try_from(&candidate, ModelLocale::Nl).unwrap();

        assert!(detailed_candidate.needs_representative);
    }

    #[test]
    fn dutch_candidate_without_bsn_confirmed() {
        let mut candidate = sample_candidate(1);
        candidate.person.personal_data.bsn = Some(BsnOrNoneConfirmed::NoneConfirmed);

        let detailed_candidate = DetailedCandidate::try_from(&candidate, ModelLocale::Nl).unwrap();

        assert_eq!(detailed_candidate.bsn, None);
    }

    #[test]
    fn dutch_candidate_without_bsn() {
        let mut candidate = sample_candidate(1);
        candidate.person.personal_data.bsn = None;

        let detailed_candidate = DetailedCandidate::try_from(&candidate, ModelLocale::Nl).unwrap();

        assert_eq!(detailed_candidate.bsn, None);
    }

    #[test]
    fn person_from_list_submitter_maps_fields() {
        let submitter = sample_list_submitter(ListSubmitterId::new());
        let person = Person::from(submitter);

        assert_eq!(person.last_name, "Bos");
        assert_eq!(person.initials, "E.F.");
        assert_eq!(person.postal_address.street_address, "Coolsingel 5B");
        assert_eq!(person.postal_address.postal_code, "3011CC".to_string());
        assert_eq!(person.postal_address.locality, "Rotterdam");
    }

    #[test]
    fn electoral_districts_from_full_list_returns_all() {
        let election = ElectionConfig::EK27;
        let list = CandidateList {
            electoral_districts: election.electoral_districts().iter().copied().collect(),
            ..Default::default()
        };

        assert_eq!(
            ElectoralDistricts::from(&list, &election),
            ElectoralDistricts::All
        );
    }

    #[test]
    fn electoral_districts_from_partial_list_returns_titles() {
        let election = ElectionConfig::EK27;
        let list = CandidateList {
            electoral_districts: BTreeSet::from([
                ElectoralDistrict::Utrecht,
                ElectoralDistrict::NoordHolland,
            ]),
            ..Default::default()
        };

        match ElectoralDistricts::from(&list, &election) {
            ElectoralDistricts::Some(districts) => {
                assert_eq!(
                    districts,
                    vec!["Utrecht".to_string(), "Noord-Holland".to_string()]
                );
            }
            _ => panic!("expected Some districts"),
        }
    }

    #[test]
    fn electoral_districts_with_only_one_district_returns_only_one() {
        let election = ElectionConfig::WS27(WaterCouncil::RijnEnIJssel);
        let list = CandidateList {
            ..Default::default()
        };

        let district = ElectoralDistricts::from(&list, &election);

        assert_eq!(district, ElectoralDistricts::OnlyOne);
    }
}
