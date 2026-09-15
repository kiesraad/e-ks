pub(crate) mod eml110a;
pub(crate) mod eml210;

use chrono::Datelike;
use eks_utils::slugify_teletex;
use eml_nl::{
    common::ElectionDomain,
    documents::ElectionIdentifierBuilder,
    utils::{ElectionCategory, ElectionDomainId, ElectionId, ElectionSubcategory},
};

use crate::{
    AppError, ElectionConfig,
    core::{ElectionType, ModelLocale},
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
            ElectionType::Kc | ElectionType::Kcni => {
                todo!("Kiescolleges don't have an official code yet in EML-NL")
            }
            ElectionType::Er => ElectionCategory::ER,
        }
    }
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
            ElectionType::Kc | ElectionType::Kcni => {
                todo!("Kiescolleges don't have an official code yet in EML-NL")
            }
            ElectionType::Er => ElectionSubcategory::ER1,
        }
    }
}

impl TryFrom<ElectionConfig> for ElectionIdentifierBuilder {
    type Error = AppError;

    fn try_from(value: ElectionConfig) -> Result<Self, Self::Error> {
        let category = ElectionCategory::from(value.election_type());
        let year = value.election_date().year();

        let id = if let Some(region) = value.region_title() {
            format!(
                "{}{}_{}",
                category.to_eml_value(),
                year,
                slugify_teletex(region, false)
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

        if let Some(region_title) = value.region_title() {
            // PS elections don't include the domain id for some reason
            let domain_id = if category == ElectionCategory::PS {
                None
            } else {
                let region_number = value
                    .region_number()
                    .expect("region_number is set alongside region_title");
                Some(ElectionDomainId::new(region_number.to_string())?)
            };
            election_id = election_id.domain(ElectionDomain::new(domain_id, region_title));
        }

        Ok(election_id)
    }
}
