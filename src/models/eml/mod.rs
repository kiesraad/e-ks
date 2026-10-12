pub(crate) mod eml110a;
pub(crate) mod eml210;
pub(crate) mod eml230b;
pub(crate) mod eml230c;

use chrono::Datelike;
use eks_utils::slugify_teletex;
use eml_nl::{
    common::{
        Agent, AgentIdentifier, CandidateIdentifier, Contact, CountryNameCode, ElectionDomain,
        FirstName, LastName, LivingAddress, MailingAddress, NameLineInitials, NamePrefix,
        PersonName, PersonNameStructure, QualifyingAddress, QualifyingAddressLocality,
    },
    documents::ElectionIdentifierBuilder,
    utils::{CandidateId, ElectionCategory, ElectionDomainId, ElectionId, ElectionSubcategory},
};

use crate::{
    AppError, ElectionConfig,
    core::{ElectionType, ModelLocale},
    structs::{
        common::{Address, DutchAddress, FullName, Gender},
        persons::{Person, PersonalData, Representative},
    },
};

impl From<&FullName> for PersonNameStructure {
    fn from(val: &FullName) -> Self {
        PersonNameStructure::new(PersonName {
            name_line_initials: val
                .initials
                .as_ref()
                .map(|initials| NameLineInitials::new(initials.to_string())),
            first_name: val
                .first_name
                .as_ref()
                .map(|n| FirstName::new(n.to_string())),
            name_prefix: val
                .last_name_prefix
                .as_ref()
                .map(|n| NamePrefix::new(n.to_string())),
            last_name: LastName::new(val.last_name.to_string()),
            person_name_type: None,
            code: None,
            name_details_key_ref: None,
        })
    }
}

impl From<&Address> for QualifyingAddress {
    fn from(address: &Address) -> QualifyingAddress {
        let locality = QualifyingAddressLocality::new(
            address
                .locality()
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
        )
        .with_postal_code_option(address.postal_code())
        .with_address_line_option(address.address_line_1());

        QualifyingAddress::Locality(locality)
    }
}

impl From<&DutchAddress> for LivingAddress {
    fn from(address: &DutchAddress) -> LivingAddress {
        LivingAddress::new(
            address
                .locality
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
        )
    }
}

impl From<&Address> for Contact {
    fn from(address: &Address) -> Contact {
        Contact::new(MailingAddress::new(QualifyingAddress::from(address)))
    }
}

impl From<&Representative> for Agent {
    fn from(representative: &Representative) -> Agent {
        Agent {
            role: Some("H10".to_string()),
            agent_identifier: AgentIdentifier::new(&representative.name),
            contact: Some((&Address::Dutch(representative.address.clone())).into()),
            living_address: (&representative.address).into(),
        }
    }
}

impl From<ElectionType> for ElectionCategory {
    fn from(value: ElectionType) -> Self {
        match value {
            ElectionType::Tk => ElectionCategory::TK,
            ElectionType::Ek => ElectionCategory::EK,
            ElectionType::Gr => ElectionCategory::GR,
            ElectionType::Ps => ElectionCategory::PS,
            ElectionType::Ws => ElectionCategory::AB,
            ElectionType::Ep => ElectionCategory::EP,
            ElectionType::Kc | ElectionType::Kcni => ElectionCategory::KC,
            ElectionType::Er => ElectionCategory::ER,
        }
    }
}

impl From<&PersonalData> for eml_nl::utils::Gender {
    fn from(data: &PersonalData) -> Self {
        match data.gender {
            None => eml_nl::utils::Gender::Unknown,
            Some(Gender::Female) => eml_nl::utils::Gender::Female,
            Some(Gender::Male) => eml_nl::utils::Gender::Male,
        }
    }
}

impl TryFrom<&PersonalData> for QualifyingAddress {
    type Error = AppError;

    fn try_from(data: &PersonalData) -> Result<Self, Self::Error> {
        Ok(QualifyingAddress::new(
            data.place_of_residence
                .as_ref()
                .ok_or(AppError::IncompleteData("missing place of residence"))?
                .to_string(),
            match data
                .country
                .as_ref()
                .ok_or(AppError::IncompleteData("missing country"))?
            {
                country if country.is_nl() => None,
                country => Some(CountryNameCode::new(country.to_string())),
            },
        ))
    }
}

/// The [`CandidateId`] EML expects for a candidate's list position (1-based)
pub(crate) fn candidate_id(position: usize) -> Result<CandidateId, AppError> {
    CandidateId::from_u64(position as u64)
        .map_err(|_| AppError::IncompleteData("candidate position is 0"))
}

/// The [`CandidateIdentifier`] EML expects for a candidate's list position
/// (1-based); shared by the 210 nomination and 230b/230c candidate list exports.
pub(crate) fn candidate_identifier(position: usize) -> Result<CandidateIdentifier, AppError> {
    Ok(CandidateIdentifier::new(candidate_id(position)?))
}

/// The candidate's own mailing address, unless they need a representative
pub(crate) fn contact(person: &Person) -> Option<Contact> {
    (!person.needs_representative()).then(|| (&Address::Dutch(person.address.clone())).into())
}

/// The candidate's representative ("gemachtigde"), when they need one
pub(crate) fn agent(person: &Person) -> Option<Agent> {
    person
        .needs_representative()
        .then(|| person.representative.as_ref().map(Into::into))
        .flatten()
}

impl From<&ElectionConfig> for ElectionSubcategory {
    fn from(value: &ElectionConfig) -> Self {
        match value.election_type() {
            ElectionType::Tk => ElectionSubcategory::TK,
            ElectionType::Ek => ElectionSubcategory::EK,
            ElectionType::Gr => {
                if value.nineteen_or_more_seats() {
                    ElectionSubcategory::GR2
                } else {
                    ElectionSubcategory::GR1
                }
            }
            ElectionType::Ps => {
                if value.has_only_one_district() {
                    ElectionSubcategory::PS1
                } else {
                    ElectionSubcategory::PS2
                }
            }
            ElectionType::Ws => {
                if value.nineteen_or_more_seats() {
                    ElectionSubcategory::AB2
                } else {
                    ElectionSubcategory::AB1
                }
            }
            ElectionType::Ep => ElectionSubcategory::EP,
            ElectionType::Kc => ElectionSubcategory::KCCN,
            ElectionType::Kcni => ElectionSubcategory::KCNI,
            ElectionType::Er => ElectionSubcategory::ER1,
        }
    }
}

impl TryFrom<ElectionConfig> for ElectionId {
    type Error = AppError;

    fn try_from(value: ElectionConfig) -> Result<Self, Self::Error> {
        let category = ElectionCategory::from(value.election_type());
        let year = value.election_date().year();

        let id = if let Some(domain) = value.domain_title() {
            format!(
                "{}{}_{}",
                category.to_eml_value(),
                year,
                slugify_teletex(domain, false)
            )
        } else {
            format!("{}{}", category.to_eml_value(), year)
        };

        Ok(ElectionId::new(id)?)
    }
}

impl TryFrom<ElectionConfig> for ElectionIdentifierBuilder {
    type Error = AppError;

    fn try_from(value: ElectionConfig) -> Result<Self, Self::Error> {
        let category = ElectionCategory::from(value.election_type());

        let mut election_id = ElectionIdentifierBuilder::new()
            .id(ElectionId::try_from(value)?)
            .name(value.full_formal_title(ModelLocale::Nl))
            .category(category)
            .subcategory(&value)
            .election_date(value.election_date())
            .nomination_date(value.nomination_day_date());

        if let Some(domain_title) = value.domain_title() {
            // PS elections don't include the domain id for some reason
            let domain_id = if category == ElectionCategory::PS {
                None
            } else {
                let domain_number = value
                    .domain_number()
                    .expect("domain_number is set alongside domain_title");
                Some(ElectionDomainId::new(domain_number.to_string())?)
            };
            election_id = election_id.domain(ElectionDomain::new(domain_id, domain_title));
        }

        Ok(election_id)
    }
}

/// Remove the variable fields from an EML string
#[cfg(test)]
pub(crate) fn remove_variable_fields(eml: &str) -> String {
    let eml = regex::Regex::new(r"<IssueDate>.*?</IssueDate>")
        .unwrap()
        .replace(eml, "<IssueDate/>")
        .into_owned();
    regex::Regex::new(r"<kr:CreationDateTime>.*?</kr:CreationDateTime>")
        .unwrap()
        .replace(&eml, "<kr:CreationDateTime/>")
        .into_owned()
}
