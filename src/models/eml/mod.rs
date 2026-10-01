pub(crate) mod eml110a;
pub(crate) mod eml210;
pub(crate) mod eml230b;

use chrono::Datelike;
use eks_utils::slugify_teletex;
use eml_nl::{
    common::{CandidateIdentifier, CountryNameCode, ElectionDomain},
    documents::{ElectionIdentifierBuilder, candidate_lists::QualifyingAddress},
    utils::{CandidateId, ElectionCategory, ElectionDomainId, ElectionId, ElectionSubcategory},
};

use crate::{
    AppError, ElectionConfig,
    core::{ElectionType, ModelLocale},
    structs::{common::Gender, persons::PersonalData},
};

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

/// The [`CandidateIdentifier`] EML expects for a candidate's list position
/// (1-based); shared by the 210 nomination and 230b candidate list exports.
pub(crate) fn candidate_identifier(position: usize) -> Result<CandidateIdentifier, AppError> {
    Ok(CandidateIdentifier::new(
        CandidateId::from_u64(position as u64)
            .map_err(|_| AppError::IncompleteData("candidate position is 0"))?,
    ))
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

impl TryFrom<ElectionConfig> for ElectionIdentifierBuilder {
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

        let mut election_id = ElectionIdentifierBuilder::new()
            .id(ElectionId::new(id)?)
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
