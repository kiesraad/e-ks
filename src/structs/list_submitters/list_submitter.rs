use crate::{
    Locale, id_newtype,
    structs::common::{
        Address, CountryCode, FullName, InternationalAddress, InternationalPostalCode, PostalCode,
        Problematic, Problems, Severity,
    },
    trans,
};
use serde::{Deserialize, Serialize};

id_newtype!(pub struct ListSubmitterId);

#[derive(Default, Debug, Clone)]
pub struct ListSubmitterData {
    pub name: FullName,
    pub address: InternationalAddress,
}

impl From<ListSubmitterData> for ListSubmitter {
    fn from(value: ListSubmitterData) -> Self {
        let is_dutch = value
            .address
            .country
            .as_ref()
            .is_none_or(CountryCode::is_nl);

        let address = if is_dutch {
            try_into_dutch_address(&value.address)
                .map(Address::Dutch)
                .unwrap_or(Address::International(value.address))
        } else {
            Address::International(value.address)
        };

        ListSubmitter {
            name: value.name,
            address,
            ..Default::default()
        }
    }
}

impl From<ListSubmitter> for ListSubmitterData {
    fn from(value: ListSubmitter) -> Self {
        let address = match value.address {
            Address::Dutch(address) => InternationalAddress {
                street_name: address.street_name,
                house_number: address.house_number,
                house_number_addition: address.house_number_addition,
                locality: address.locality,
                state_or_province: None,
                postal_code: address.postal_code.map(|postal_code| {
                    postal_code
                        .to_string()
                        .parse::<InternationalPostalCode>()
                        .expect("dutch postal code must fit international postal code")
                }),
                country: None,
            },
            Address::International(address) => address,
        };

        ListSubmitterData {
            name: value.name,
            address,
        }
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(PartialEq))]
pub struct ListSubmitter {
    pub id: ListSubmitterId,
    pub name: FullName,
    pub address: Address,
    #[serde(skip)]
    pub is_substitute: bool,
}

impl Problematic<()> for ListSubmitter {
    fn get_problems(&self, _: ()) -> Problems {
        let severity = if self.is_substitute {
            Severity::Warn
        } else {
            Severity::Error
        };

        if self.is_empty() && !self.is_substitute {
            return Problems::new_empty(); // error gets returned in general problems
        }

        Problems::merge(vec![
            self.name.get_problems(severity),
            self.address.get_problems(severity),
        ])
    }
}

impl ListSubmitter {
    pub fn is_empty(&self) -> bool {
        self.name.is_empty() && self.address.is_empty()
    }

    /// Clone the stored submitters with the (unpersisted) substitute flag set.
    pub fn clone_as_substitutes<'a>(
        submitters: impl IntoIterator<Item = &'a ListSubmitter>,
    ) -> Vec<ListSubmitter> {
        submitters
            .into_iter()
            .cloned()
            .map(|mut submitter| {
                submitter.is_substitute = true;
                submitter
            })
            .collect()
    }

    /// Build the updated submitter from validated form data, keeping this
    /// submitter's id and refreshing the address's BAG flag.
    pub fn updated_from(&self, data: ListSubmitterData) -> Self {
        let mut updated = ListSubmitter {
            id: self.id,
            ..data.into()
        };
        updated.address.update_is_known_in_bag();
        updated
    }

    pub fn address_line_1(&self) -> String {
        self.address.address_line_1().unwrap_or_default()
    }

    pub fn address_line_2(&self) -> String {
        self.address.address_line_2().unwrap_or_default()
    }

    pub fn display(&self, locale: &Locale, is_substitute: bool) -> String {
        let role = if is_substitute {
            trans!("political_group.substitute_submitter", locale)
        } else {
            trans!("political_group.list_submitter", locale)
        };

        format!("{} ({role})", self.name.display())
    }
}

fn try_into_dutch_address(
    address: &InternationalAddress,
) -> Option<crate::structs::common::DutchAddress> {
    Some(crate::structs::common::DutchAddress {
        street_name: address.street_name.clone(),
        house_number: address.house_number.clone(),
        house_number_addition: address.house_number_addition.clone(),
        locality: address.locality.clone(),
        postal_code: address
            .postal_code
            .as_ref()
            .map(|postal_code| postal_code.to_string().parse::<PostalCode>())
            .transpose()
            .ok()?,
        known_in_bag: None,
    })
}

#[cfg(test)]
mod tests {
    use crate::structs::common::{EmptyAddressProblems, PotentialProblems};

    use super::*;

    fn incomplete_submitter(is_substitute: bool) -> ListSubmitter {
        ListSubmitter {
            id: ListSubmitterId::new(),
            name: FullName {
                last_name_prefix: Some("van".parse().unwrap()),
                ..Default::default()
            },
            address: Address::Dutch(crate::structs::common::DutchAddress::default()),
            is_substitute,
        }
    }

    #[test]
    fn main_submitter_problems_use_error_severity() {
        let problems = incomplete_submitter(false).get_problems(());

        assert!(
            problems
                .potential_problems
                .contains(&PotentialProblems::NoLastName(Severity::Error))
        );
        assert!(problems.potential_problems.iter().any(|pp| match pp {
            PotentialProblems::IncompleteAddress {
                severity: Severity::Error,
                problems,
            } => {
                problems.contains(&EmptyAddressProblems::StreetName)
            }
            _ => false,
        }));
        assert!(problems.info_problems.is_empty());
    }

    #[test]
    fn international_submitter_address_is_not_bag_checked() {
        let data = ListSubmitterData {
            name: FullName {
                last_name: "Bos".parse().expect("last name"),
                initials: "E.F.".parse().expect("initials"),
                ..Default::default()
            },
            address: InternationalAddress {
                street_name: Some("Downing Street".parse().expect("street name")),
                house_number: Some("10".parse().expect("house number")),
                house_number_addition: None,
                locality: Some("London".parse().expect("locality")),
                state_or_province: None,
                postal_code: Some("SW1A 2AA".parse().expect("postal code")),
                country: Some("GB".parse().expect("country code")),
            },
        };

        let submitter = ListSubmitter::from(data);

        assert!(matches!(submitter.address, Address::International(_)));
        assert!(
            !submitter
                .get_problems(())
                .potential_problems
                .contains(&PotentialProblems::UnknownAddress)
        );
    }

    #[test]
    fn substitute_submitter_problems_use_warn_severity() {
        let problems = incomplete_submitter(true).get_problems(());

        assert!(
            problems
                .potential_problems
                .contains(&PotentialProblems::NoLastName(Severity::Warn))
        );
        assert!(problems.potential_problems.iter().any(|pp| match pp {
            PotentialProblems::IncompleteAddress {
                severity: Severity::Warn,
                problems,
            } => {
                problems.contains(&EmptyAddressProblems::StreetName)
            }
            _ => false,
        }));
        assert!(problems.info_problems.is_empty());
    }
}
