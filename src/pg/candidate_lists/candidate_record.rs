use crate::{
    common::MinimalNameForm,
    structs::{
        common::{BsnOrNoneConfirmed, DutchAddress},
        persons::{Person, Representative},
    },
};
use serde::{Deserialize, Serialize};
use validate::Validate;

use crate::{
    OptionStringExt,
    common::{DutchAddressForm, FullNameForm},
    constants::DEFAULT_DATE_FORMAT,
    core::AnyLocale,
    persons::{PersonalDataFieldsForm, RepresentativeForm},
};

const NO_BSN: &str = "kandidaat heeft geen BSN";
pub(crate) const CSV_HEADERS: [&str; 22] = [
    "voorletters",
    "roepnaam",
    "voorvoegsel",
    "achternaam",
    "woonplaats",
    "landcode",
    "bsn",
    "geboortedatum",
    "geslacht",
    "correspondentie_postcode",
    "correspondentie_huisnummer",
    "correspondentie_toevoeging",
    "correspondentie_straatnaam",
    "correspondentie_plaats",
    "gemachtigde_voorletters",
    "gemachtigde_voorvoegsel",
    "gemachtigde_achternaam",
    "gemachtigde_postcode",
    "gemachtigde_huisnummer",
    "gemachtigde_toevoeging",
    "gemachtigde_straatnaam",
    "gemachtigde_plaats",
];

#[derive(Debug, Serialize, Deserialize, Clone, Default, Validate)]
#[validate(target = "Person")]
#[serde(default)]
pub struct CandidateRecord {
    #[serde(flatten)]
    #[validate(flatten)]
    name: FullNameForm,
    #[serde(flatten)]
    #[validate(flatten)]
    personal_data: PersonalDataFieldsForm,
    #[serde(flatten)]
    #[validate(flatten)]
    address: DutchAddressForm,
    #[serde(flatten)]
    #[validate(flatten)]
    representative: Option<RepresentativeForm>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub(crate) struct CandidateRecordCsv {
    voorletters: String,
    roepnaam: String,
    voorvoegsel: String,
    achternaam: String,

    woonplaats: String,
    landcode: String,
    bsn: String,
    geboortedatum: String,
    geslacht: String,

    correspondentie_postcode: String,
    correspondentie_huisnummer: String,
    correspondentie_toevoeging: String,
    correspondentie_straatnaam: String,
    correspondentie_plaats: String,

    gemachtigde_voorletters: String,
    gemachtigde_voorvoegsel: String,
    gemachtigde_achternaam: String,

    gemachtigde_postcode: String,
    gemachtigde_huisnummer: String,
    gemachtigde_toevoeging: String,
    gemachtigde_straatnaam: String,
    gemachtigde_plaats: String,
}

impl From<CandidateRecordCsv> for CandidateRecord {
    fn from(csv: CandidateRecordCsv) -> Self {
        let representative = RepresentativeForm {
            name: MinimalNameForm {
                last_name: csv.gemachtigde_achternaam,
                last_name_prefix: csv.gemachtigde_voorvoegsel,
                initials: csv.gemachtigde_voorletters,
            },
            address: DutchAddressForm {
                locality: csv.gemachtigde_plaats,
                postal_code: csv.gemachtigde_postcode,
                house_number: csv.gemachtigde_huisnummer,
                house_number_addition: csv.gemachtigde_toevoeging,
                street_name: csv.gemachtigde_straatnaam,
            },
        };

        CandidateRecord {
            name: FullNameForm {
                first_name: csv.roepnaam,
                last_name: csv.achternaam,
                last_name_prefix: csv.voorvoegsel,
                initials: csv.voorletters,
            },
            personal_data: PersonalDataFieldsForm {
                gender: csv.geslacht.parse().unwrap_or_default(),
                date_of_birth: csv.geboortedatum,
                bsn: bsn_from_csv(&csv.bsn),
                place_of_residence: csv.woonplaats,
                country: country_from_csv(csv.landcode),
            },
            address: DutchAddressForm {
                locality: csv.correspondentie_plaats,
                postal_code: csv.correspondentie_postcode,
                house_number: csv.correspondentie_huisnummer,
                house_number_addition: csv.correspondentie_toevoeging,
                street_name: csv.correspondentie_straatnaam,
            },
            representative: if representative.is_empty() {
                None
            } else {
                Some(representative)
            },
        }
    }
}

impl From<Person> for CandidateRecordCsv {
    fn from(person: Person) -> Self {
        let needs_representative = person.needs_representative();
        let Person {
            name: candidate_name,
            personal_data: candidate_personal_data,
            address: person_address,
            representative: person_representative,
            ..
        } = person;

        let representative = if needs_representative {
            person_representative.unwrap_or_default()
        } else {
            Representative::default()
        };

        let address = if needs_representative {
            DutchAddress::default()
        } else {
            person_address
        };

        CandidateRecordCsv {
            voorletters: candidate_name.initials.to_string_or_default(),
            roepnaam: candidate_name.first_name.to_string_or_default(),
            voorvoegsel: candidate_name.last_name_prefix.to_string_or_default(),
            achternaam: candidate_name.last_name.to_string(),

            woonplaats: candidate_personal_data
                .place_of_residence
                .to_string_or_default(),
            landcode: candidate_personal_data.country.to_string_or_default(),
            bsn: match candidate_personal_data.bsn {
                Some(BsnOrNoneConfirmed::NoneConfirmed) => NO_BSN.to_string(),
                Some(BsnOrNoneConfirmed::Bsn(bsn)) => bsn.to_exposed_string(),
                None => String::new(),
            },
            geboortedatum: candidate_personal_data
                .date_of_birth
                .map(|d| d.format(DEFAULT_DATE_FORMAT))
                .to_string_or_default(),
            geslacht: match candidate_personal_data.gender {
                Some(gender) => gender.abbreviation(AnyLocale::Nl).to_string(),
                None => String::new(),
            },

            correspondentie_postcode: address.postal_code.to_string_or_default(),
            correspondentie_huisnummer: address.house_number.to_string_or_default(),
            correspondentie_toevoeging: address.house_number_addition.to_string_or_default(),
            correspondentie_straatnaam: address.street_name.to_string_or_default(),
            correspondentie_plaats: address.locality.to_string_or_default(),

            gemachtigde_voorletters: representative.name.initials.to_string_or_default(),
            gemachtigde_voorvoegsel: representative.name.last_name_prefix.to_string_or_default(),
            gemachtigde_achternaam: representative.name.last_name.to_string(),
            gemachtigde_postcode: representative.address.postal_code.to_string_or_default(),
            gemachtigde_huisnummer: representative.address.house_number.to_string_or_default(),
            gemachtigde_toevoeging: representative
                .address
                .house_number_addition
                .to_string_or_default(),
            gemachtigde_straatnaam: representative.address.street_name.to_string_or_default(),
            gemachtigde_plaats: representative.address.locality.to_string_or_default(),
        }
    }
}

impl From<Person> for CandidateRecord {
    fn from(person: Person) -> Self {
        Self::from(CandidateRecordCsv::from(person))
    }
}

/// An empty `landcode` column defaults to NL, matching the personal details form.
fn country_from_csv(landcode: String) -> String {
    if landcode.trim().is_empty() {
        "NL".to_string()
    } else {
        landcode
    }
}

fn bsn_from_csv(value: &str) -> String {
    let value = value.trim();
    if value == NO_BSN {
        "none-confirmed".to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        structs::{
            common::{FullName, Gender, PlaceOfResidence},
            persons::PersonId,
        },
        test_utils::{display_opt, sample_dutch_address, sample_person},
    };

    use super::*;

    /// Asserts every field of a Dutch address by its `Display` rendering.
    fn assert_address(
        address: &DutchAddress,
        postal_code: &str,
        house_number: &str,
        house_number_addition: &str,
        street_name: &str,
        locality: &str,
    ) {
        assert_eq!(
            display_opt(&address.postal_code).as_deref(),
            Some(postal_code)
        );
        assert_eq!(
            display_opt(&address.house_number).as_deref(),
            Some(house_number)
        );
        assert_eq!(
            display_opt(&address.house_number_addition).as_deref(),
            Some(house_number_addition)
        );
        assert_eq!(
            display_opt(&address.street_name).as_deref(),
            Some(street_name)
        );
        assert_eq!(display_opt(&address.locality).as_deref(), Some(locality));
    }

    #[test]
    fn validate_create_maps_correspondence_address_to_person_address() {
        let record = CandidateRecord::from(CandidateRecordCsv {
            voorletters: "J.".to_string(),
            roepnaam: "Jan".to_string(),
            voorvoegsel: "van de".to_string(),
            achternaam: "Berg".to_string(),
            woonplaats: "Amsterdam".to_string(),
            landcode: "NL".to_string(),
            bsn: NO_BSN.to_string(),
            geboortedatum: "20-10-2000".to_string(),
            geslacht: "m".to_string(),
            correspondentie_postcode: "1234AB".to_string(),
            correspondentie_huisnummer: "12".to_string(),
            correspondentie_toevoeging: "a".to_string(),
            correspondentie_straatnaam: "Mooie Straat".to_string(),
            correspondentie_plaats: "Rotterdam".to_string(),
            ..Default::default()
        });

        let person = record.validate_create().unwrap();

        assert_eq!(person.name.initials.to_string_or_default(), "J.");
        assert_eq!(person.name.first_name.unwrap().to_string(), "Jan");
        assert_eq!(person.name.last_name_prefix.unwrap().to_string(), "van de");
        assert_eq!(person.name.last_name.to_string(), "Berg");
        assert_eq!(person.personal_data.gender, Some(Gender::Male));
        assert_eq!(
            person.personal_data.bsn,
            Some(BsnOrNoneConfirmed::NoneConfirmed)
        );
        assert_eq!(
            person
                .personal_data
                .date_of_birth
                .map(|d| d.format(DEFAULT_DATE_FORMAT).to_string()),
            Some("20-10-2000".to_string())
        );
        assert_eq!(
            display_opt(&person.personal_data.place_of_residence).as_deref(),
            Some("Amsterdam")
        );
        assert_eq!(
            display_opt(&person.personal_data.country).as_deref(),
            Some("NL")
        );
        assert_address(
            &person.address,
            "1234AB",
            "12",
            "a",
            "Mooie Straat",
            "Rotterdam",
        );
        assert_eq!(person.representative, None);
    }

    #[test]
    fn empty_landcode_defaults_to_nl() {
        let record = CandidateRecord::from(CandidateRecordCsv {
            voorletters: "J.".to_string(),
            roepnaam: "Jan".to_string(),
            achternaam: "Berg".to_string(),
            woonplaats: "Amsterdam".to_string(),
            landcode: "  ".to_string(),
            bsn: NO_BSN.to_string(),
            geboortedatum: "20-10-2000".to_string(),
            geslacht: "m".to_string(),
            ..Default::default()
        });

        let person = record.validate_create().unwrap();

        assert_eq!(
            person
                .personal_data
                .country
                .as_ref()
                .map(ToString::to_string),
            Some("NL".to_string())
        );
    }

    #[test]
    fn validate_create_maps_representative_name_and_address_when_present() {
        let record = CandidateRecord::from(CandidateRecordCsv {
            voorletters: "J.".to_string(),
            roepnaam: "Jan".to_string(),
            voorvoegsel: "van de".to_string(),
            achternaam: "Berg".to_string(),
            woonplaats: "Antwerp".to_string(),
            landcode: "BE".to_string(),
            bsn: String::new(),
            geboortedatum: "20-10-2000".to_string(),
            geslacht: "v".to_string(),
            gemachtigde_voorletters: "P.".to_string(),
            gemachtigde_voorvoegsel: String::new(),
            gemachtigde_achternaam: "Puk".to_string(),
            gemachtigde_postcode: "5678CD".to_string(),
            gemachtigde_huisnummer: "34".to_string(),
            gemachtigde_toevoeging: "b".to_string(),
            gemachtigde_straatnaam: "Mooiere Straat".to_string(),
            gemachtigde_plaats: "Den Haag".to_string(),
            ..Default::default()
        });

        let person = record.validate_create().unwrap();

        assert_eq!(person.personal_data.gender, Some(Gender::Female));
        assert_eq!(
            display_opt(&person.personal_data.country).as_deref(),
            Some("BE")
        );
        assert_eq!(person.address, DutchAddress::default());

        let representative = person.representative.as_ref().unwrap();

        assert_eq!(
            representative.name.initials.clone().to_string_or_default(),
            "P."
        );
        assert_eq!(
            display_opt(&representative.name.first_name).as_deref(),
            None
        );
        assert_eq!(representative.name.last_name.to_string(), "Puk");
        assert_address(
            &representative.address,
            "5678CD",
            "34",
            "b",
            "Mooiere Straat",
            "'s-Gravenhage",
        );
    }

    #[test]
    fn csv_export_uses_representative_for_caribbean_nl_residents() {
        let mut person = sample_person(PersonId::new());
        person.representative = Some(Representative {
            name: FullName {
                first_name: None,
                last_name: "Puk".parse().expect("last name"),
                last_name_prefix: None,
                initials: Some("P.".parse().expect("initials")),
            },
            address: sample_dutch_address("Den Haag", "5678 CD", "34", "b", "Mooiere Straat"),
        });

        // a Dutch resident exports the correspondence address, not the representative
        let csv = CandidateRecordCsv::from(person.clone());
        assert_eq!(csv.correspondentie_plaats, "Juinen");
        assert_eq!(csv.gemachtigde_achternaam, "");
        assert_eq!(csv.gemachtigde_postcode, "");

        // a Caribbean Netherlands resident (country NL) exports the representative
        person.personal_data.place_of_residence =
            Some(PlaceOfResidence::Known("Bonaire".to_string()));
        let csv = CandidateRecordCsv::from(person);
        assert_eq!(csv.landcode, "NL");
        assert_eq!(csv.woonplaats, "Bonaire");
        assert_eq!(csv.correspondentie_plaats, "");
        assert_eq!(csv.correspondentie_postcode, "");
        assert_eq!(csv.gemachtigde_achternaam, "Puk");
        assert_eq!(csv.gemachtigde_postcode, "5678CD");
        assert_eq!(csv.gemachtigde_plaats, "'s-Gravenhage");
    }
}
