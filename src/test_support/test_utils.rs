//! Test helpers for building sample domain data and reading responses.
use crate::structs::{
    common::{
        Address, Appellation, BsnOrNoneConfirmed, CountryCode, DateOfBirth, DutchAddress,
        FirstName, FullName, Gender, HouseNumber, HouseNumberAddition, Initials, LastName,
        LastNamePrefix, Locality, PlaceOfResidence, PostalCode, PreviousElectionResults,
        StreetName,
    },
    list_submitters::{ListSubmitter, ListSubmitterId},
    name_authorisations::{NameAuthorisation, NameAuthorisationId},
    persons::{Person, PersonId, PersonalData},
    political_groups::PoliticalGroup,
};
use http_body_util::BodyExt;

use crate::{
    AppError, Context, ElectionConfig, ElectoralDistrict, PgStore, TokenValue,
    common::{DutchAddressForm, FullNameForm, InternationalAddressForm, MinimalNameForm},
    list_submitters::ListSubmitterForm,
    name_authorisations::NameAuthorisationForm,
    persons::{AddressForm, PersonalDataForm, RepresentativeForm},
    political_groups::PoliticalGroupForm,
    structs::{
        candidate_lists::{CandidateList, CandidateListId},
        list_designation::ListDesignation,
    },
};

pub async fn response_body_string(response: axum::response::Response) -> String {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    String::from_utf8(bytes.to_vec()).expect("utf-8 body")
}

/// The `Display` rendering of an optional field.
pub fn display_opt<T: std::fmt::Display>(value: &Option<T>) -> Option<String> {
    value.as_ref().map(ToString::to_string)
}

pub fn extract_csrf_token(body: &str) -> Option<TokenValue> {
    let marker = "name=\"csrf_token\" value=\"";
    body.split(marker)
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .map(|token| TokenValue(token.to_string()))
}

pub fn sample_full_name(
    first_name: Option<&str>,
    last_name: &str,
    last_name_prefix: Option<&str>,
    initials: &str,
) -> FullName {
    FullName {
        first_name: first_name.map(parse_first_name),
        last_name: parse_last_name(last_name),
        last_name_prefix: last_name_prefix.map(parse_last_name_prefix),
        initials: parse_initials(initials),
    }
}

pub fn parse_last_name(value: &str) -> LastName {
    value.parse::<LastName>().expect("last name")
}

pub fn parse_last_name_prefix(value: &str) -> LastNamePrefix {
    value.parse::<LastNamePrefix>().expect("last name prefix")
}

pub fn parse_initials(value: &str) -> Initials {
    value.parse::<Initials>().expect("initials")
}

pub fn parse_first_name(value: &str) -> FirstName {
    value.parse::<FirstName>().expect("first name")
}

pub fn parse_country_code(value: &str) -> CountryCode {
    value.parse::<CountryCode>().expect("country code")
}

fn sample_full_name_form(
    first_name: &str,
    last_name: &str,
    last_name_prefix: &str,
    initials: &str,
) -> FullNameForm {
    FullNameForm {
        first_name: first_name.to_string(),
        last_name: last_name.to_string(),
        last_name_prefix: last_name_prefix.to_string(),
        initials: initials.to_string(),
    }
}

fn sample_minimal_name_form(
    last_name: &str,
    last_name_prefix: &str,
    initials: &str,
) -> MinimalNameForm {
    MinimalNameForm {
        last_name: last_name.to_string(),
        last_name_prefix: last_name_prefix.to_string(),
        initials: initials.to_string(),
    }
}

pub fn sample_dutch_address(
    locality: &str,
    postal_code: &str,
    house_number: &str,
    house_number_addition: &str,
    street_name: &str,
) -> DutchAddress {
    DutchAddress {
        locality: Some(locality.parse::<Locality>().expect("locality")),
        postal_code: Some(postal_code.parse::<PostalCode>().expect("postal code")),
        house_number: Some(house_number.parse::<HouseNumber>().expect("house number")),
        house_number_addition: Some(
            house_number_addition
                .parse::<HouseNumberAddition>()
                .expect("house number addition"),
        ),
        street_name: Some(street_name.parse::<StreetName>().expect("street name")),
        known_in_bag: Some(true),
    }
}

fn sample_dutch_address_form(
    locality: &str,
    postal_code: &str,
    house_number: &str,
    house_number_addition: &str,
    street_name: &str,
) -> DutchAddressForm {
    DutchAddressForm {
        locality: locality.to_string(),
        postal_code: postal_code.to_string(),
        house_number: house_number.to_string(),
        house_number_addition: house_number_addition.to_string(),
        street_name: street_name.to_string(),
    }
}

pub fn sample_candidate_list(id: CandidateListId) -> CandidateList {
    CandidateList {
        id,
        electoral_districts: vec![ElectoralDistrict::Utrecht],
        ..Default::default()
    }
}

pub fn sample_person(id: PersonId) -> Person {
    Person {
        id,
        name: sample_full_name(Some("Henk"), "Jansen", None, "H.A.H.A."),
        personal_data: PersonalData {
            gender: Some(Gender::Female),
            date_of_birth: Some("01-02-1990".parse::<DateOfBirth>().unwrap()),
            bsn: Some(BsnOrNoneConfirmed::NoneConfirmed),
            place_of_residence: Some(PlaceOfResidence::Known("Juinen".to_string())),
            country: Some(parse_country_code("NL")),
        },
        address: sample_dutch_address("Juinen", "1234 AB", "10", "A", "Stationsstraat"),
        representative: None,
        ..Default::default()
    }
}

pub fn sample_person_from_brp() -> Person {
    Person {
        id: PersonId::new(),
        name: FullName {
            first_name: Some("Tina-Antïna".parse().unwrap()),
            last_name: "Bruin".parse().unwrap(),
            last_name_prefix: Some("de".parse().unwrap()),
            initials: "T.".parse().unwrap(),
        },
        personal_data: PersonalData {
            gender: Some(Gender::Female),
            bsn: Some("900194054".parse().unwrap()),
            date_of_birth: Some("11-12-1990".parse().unwrap()),
            place_of_residence: Some("Utrecht".parse().unwrap()),
            country: Some("NL".parse().unwrap()),
        },
        address: DutchAddress {
            street_name: Some("Croeselaan".parse().unwrap()),
            house_number: Some("15".parse().unwrap()),
            house_number_addition: None,
            locality: Some("Utrecht".parse().unwrap()),
            postal_code: Some("3521BJ".parse().unwrap()),
            known_in_bag: None,
        },
        representative: None,
        updated_at: Default::default(),
    }
}

pub fn sample_person_with_last_name(id: PersonId, last_name: &str) -> Person {
    sample_person_with(id, None, last_name, None, "H.A.H.A.")
}

pub fn sample_person_with(
    id: PersonId,
    first_name: Option<&str>,
    last_name: &str,
    last_name_prefix: Option<&str>,
    initials: &str,
) -> Person {
    let mut person = sample_person(id);
    person.name = sample_full_name(first_name, last_name, last_name_prefix, initials);
    person
}

pub fn sample_person_form() -> PersonalDataForm {
    PersonalDataForm {
        name: sample_full_name_form("Henk", "Jansen", "", "H.A.H.A."),
        personal_data: crate::persons::PersonalDataFieldsForm {
            gender: "male".to_string(),
            date_of_birth: "01-02-1990".to_string(),
            bsn: "none-confirmed".to_string(),
            place_of_residence: "Juinen".to_string(),
            country: "NL".to_string(),
        },
    }
}

pub fn sample_address_form() -> AddressForm {
    AddressForm {
        address: sample_dutch_address_form("Juinen", "1234 AB", "10", "A", "Stationsstraat"),
    }
}

pub fn sample_representative_form() -> RepresentativeForm {
    RepresentativeForm {
        name: sample_minimal_name_form("Bakker", "", "A.B."),
        address: sample_dutch_address_form("Juinen", "1234 AB", "10", "A", "Stationsstraat"),
    }
}

pub fn sample_political_group() -> PoliticalGroup {
    PoliticalGroup {
        appellation: Some("Kiesraad Demo".parse::<Appellation>().expect("appellation")),
        list_designation: Some(ListDesignation::Standalone),
        previous_election_results: Some(PreviousElectionResults::ZeroSeats),
    }
}

/// A [`PgStore`] in paper-corrections mode: reads serve the imported snapshot
/// of a political group, writes land on the CSB stream.
pub async fn paper_corrections_store() -> Result<PgStore, AppError> {
    let csb_store = crate::CsbStore::new_for_test();
    csb_store
        .update(crate::CsbAction::Import {
            hash: [1; 32],
            source_stream_id: crate::StreamId::new(),
            snapshot: Box::new(crate::PgStoreData {
                political_group: sample_political_group(),
                ..Default::default()
            }),
        })
        .await?;

    Ok(csb_store.paper_corrections())
}

pub fn sample_name_authorisation(id: NameAuthorisationId) -> NameAuthorisation {
    NameAuthorisation {
        id,
        name: sample_full_name(Some("Henk"), "Jansen", Some("de"), "A.B."),
        legal_name: "Kiesraad Demo Partij".parse().expect("legal name"),
    }
}

pub fn sample_name_authorisation_form() -> NameAuthorisationForm {
    NameAuthorisationForm {
        name: sample_minimal_name_form("Jansen", "de", "A.B."),
        legal_name: "Kiesraad Demo Partij".to_string(),
    }
}

pub fn sample_list_submitter(id: ListSubmitterId) -> ListSubmitter {
    ListSubmitter {
        id,
        name: sample_full_name(None, "Bos", None, "E.F."),
        address: Address::Dutch(sample_dutch_address(
            "Rotterdam",
            "3011 CC",
            "5",
            "B",
            "Coolsingel",
        )),
        is_substitute: false,
    }
}

pub fn sample_list_submitter_form() -> ListSubmitterForm {
    ListSubmitterForm {
        name: crate::common::MinimalNameForm {
            last_name: "Bos".to_string(),
            last_name_prefix: String::new(),
            initials: "E.F.".to_string(),
        },
        address: InternationalAddressForm {
            country: String::new(),
            locality: "Rotterdam".to_string(),
            state_or_province: String::new(),
            postal_code: "3011 CC".to_string(),
            house_number: "5".to_string(),
            house_number_addition: "B".to_string(),
            street_name: "Coolsingel".to_string(),
        },
    }
}

pub fn sample_political_group_form() -> PoliticalGroupForm {
    PoliticalGroupForm {
        previous_election_results: PreviousElectionResults::OneToFifteenSeats.to_string(),
        appellation: "Updated Appellation".to_string(),
    }
}

pub async fn setup_documents_test_state(
    list_count: usize,
    candidate_count: usize,
    include_list_submitter: bool,
    include_authorised_agent: bool,
    election: ElectionConfig,
) -> Result<(PgStore, Vec<CandidateListId>, Context), AppError> {
    let store = PgStore::new_for_test_with_election(election);
    let mut list_ids = Vec::new();

    if include_list_submitter {
        sample_list_submitter(ListSubmitterId::new())
            .update(&store)
            .await?;
    }

    if include_authorised_agent {
        sample_name_authorisation(NameAuthorisationId::new())
            .create(&store)
            .await?;
    }

    for _ in 0..list_count {
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        if let Some(district) = CandidateList::available_districts(&store, &election)
            .into_iter()
            .next()
        {
            list.electoral_districts = vec![district];
        }

        for _ in 0..candidate_count {
            let person_id = PersonId::new();
            sample_person(person_id).create(&store).await?;
            list.candidates.push(person_id);
        }

        list.create(&store).await?;
        list_ids.push(list_id);
    }

    Ok((
        store.clone(),
        list_ids,
        Context::new(
            &store,
            crate::Session::new_test_with_locale(crate::Locale::En),
        ),
    ))
}

/// Asserts the zip attachment and no-cache headers on a documents response.
pub fn assert_zip_response_headers(headers: &axum::http::HeaderMap) {
    use axum::http::header;

    assert_eq!(
        headers
            .get(header::CONTENT_TYPE)
            .expect("content type header"),
        "application/zip"
    );
    assert!(
        regex::Regex::new("attachment; filename=\"kiesraad-demo-ek27-v\\d+\\.zip\"")
            .expect("valid filename regex")
            .is_match(
                headers
                    .get(header::CONTENT_DISPOSITION)
                    .expect("content disposition header")
                    .to_str()
                    .expect("ascii header")
            )
    );
    assert_eq!(
        headers
            .get(header::CACHE_CONTROL)
            .expect("cache control header"),
        "no-store, no-cache, must-revalidate, max-age=0"
    );
    assert_eq!(
        headers.get(header::PRAGMA).expect("pragma header"),
        "no-cache"
    );
    assert_eq!(headers.get(header::EXPIRES).expect("expires header"), "0");
}

pub async fn zip_entry_names(response: axum::response::Response) -> Vec<String> {
    use async_zip::base::read1::seek::ZipArchiveReader;
    use futures_lite::io::Cursor;

    let body = Cursor::new(response_body(response).await.to_vec());
    let zip = ZipArchiveReader::open(body).await.expect("zip body");

    zip.cdrs()
        .iter()
        .map(|cdr| {
            cdr.insecure_file_name
                .as_str()
                .expect("utf-8 zip entry name")
                .to_string()
        })
        .collect()
}

async fn response_body(response: axum::response::Response) -> bytes::Bytes {
    use http_body_util::BodyExt;

    response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes()
}
