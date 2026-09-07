use std::{str::FromStr, time::Duration};

use chrono::NaiveDate;
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

use super::{BrpCheckedField, BrpField, BrpFinding, BrpPerson, BrpValue, person::BrpResidence};
use crate::{
    AppError,
    structs::{
        common::{Bsn, BsnOrNoneConfirmed, DateOfBirth, Gender, LastNamePrefix},
        persons::{Person, PersonId},
    },
};

/// The BRP accepts at most twenty burgerservicenummers per request.
pub const BRP_BSN_BATCH_SIZE: usize = 10;

/// The fields requested per candidate.
pub const CANDIDATE_FIELDS: &[BrpField] = &[
    BrpField::Bsn,
    BrpField::Initials,
    BrpField::LastNamePrefix,
    BrpField::LastName,
    BrpField::Gender,
    BrpField::DateOfBirth,
    BrpField::PlaceOfResidence,
    BrpField::DateOfDeath,
    BrpField::Nationality,
    BrpField::SuffrageExclusion,
];

/// The `geslacht` code the BRP uses when the gender is unknown.
const GENDER_UNKNOWN_CODE: &str = "O";

/// The `geslacht` code the BRP uses for a gender this application records.
fn brp_gender_code(gender: Gender) -> &'static str {
    match gender {
        Gender::Male => "M",
        Gender::Female => "V",
    }
}

/// The date format the BRP uses for a complete date.
const BRP_DATE_FORMAT: &str = "%Y-%m-%d";

#[derive(Clone)]
pub struct BrpClient {
    http_client: Client,
    base_url: String,
    api_key: SecretString,
    persons_endpoint: String,
    timeout: Duration,
}

impl BrpClient {
    pub fn new(
        base_url: &str,
        api_key: SecretString,
        persons_endpoint: &str,
        timeout: Duration,
    ) -> Self {
        Self {
            http_client: Client::new(),
            base_url: base_url.to_string(),
            api_key,
            persons_endpoint: persons_endpoint.to_string(),
            timeout,
        }
    }

    /// A client pointed at `base_url`, for tests serving their own responses.
    #[cfg(test)]
    pub fn new_for_test(base_url: &str) -> Self {
        use crate::constants;

        BrpClient::new(
            base_url,
            SecretString::from(""),
            constants::BRP_PERSONS_ENDPOINT,
            Duration::from_secs(5),
        )
    }

    pub async fn get_persons(&self, query: &BrpQuery) -> Result<Vec<BrpPerson>, AppError> {
        let url = format!("{}/{}", self.base_url, self.persons_endpoint);

        let response = self
            .http_client
            .post(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.api_key.expose_secret()),
            )
            .json(query)
            .timeout(self.timeout)
            .send()
            .await?
            .error_for_status()?;

        match response.json::<BrpResponse>().await? {
            BrpResponse::ConsultWithBsn { persons }
            | BrpResponse::SearchByLastNameAndDateOfBirth { persons } => Ok(persons),
        }
    }

    /// Check up to [`BRP_BSN_BATCH_SIZE`] candidates in a single BRP request
    /// and return the findings per candidate.
    ///
    /// An `Err` means the BRP could not be consulted at all, so the caller has
    /// to stop rather than treat the batch as clean. Anything wrong with an
    /// individual candidate is a [`BrpFinding`], not an error.
    pub async fn verify_batch(
        &self,
        persons: &[Person],
    ) -> Result<Vec<(PersonId, Vec<BrpFinding>)>, AppError> {
        if persons.len() > BRP_BSN_BATCH_SIZE {
            return Err(AppError::BrpError(format!(
                "a batch of {} candidates exceeds the batch size of {BRP_BSN_BATCH_SIZE}",
                persons.len()
            )));
        }

        let with_bsn: Vec<&Person> = persons
            .iter()
            .filter(|person| bsn_of(person).is_some())
            .collect();

        let brp_persons = if with_bsn.is_empty() {
            Vec::new()
        } else {
            let query = BrpQuery::ConsultWithBsn {
                bsn: with_bsn.iter().filter_map(|p| bsn_of(p).cloned()).collect(),
                fields: CANDIDATE_FIELDS.to_vec(),
            };
            self.get_persons(&query).await?
        };

        let mut results = Vec::with_capacity(persons.len());
        for person in persons {
            // Responses come back as one list, so each candidate is matched
            // by burgerservicenummer.
            let matched: Vec<&BrpPerson> = match bsn_of(person) {
                Some(bsn) => brp_persons
                    .iter()
                    .filter(|brp_person| brp_person.bsn.as_deref() == Some(bsn.expose()))
                    .collect(),
                None => Vec::new(),
            };

            let findings = match matched.as_slice() {
                // Nobody to compare against yet: a burgerservicenummer that is
                // missing or wrong should still not leave the candidate
                // unchecked, so their other details are searched on.
                [] => self.findings_without_a_bsn_match(person).await?,
                [brp_person] => findings_for(person, brp_person),
                _ => vec![BrpFinding::BsnNotUnique],
            };

            results.push((person.id, findings));
        }

        Ok(results)
    }

    /// The findings for a candidate the burgerservicenummer did not resolve:
    /// why the lookup failed, plus a comparison against the one person their
    /// other personal details identify, if there is exactly one.
    async fn findings_without_a_bsn_match(
        &self,
        person: &Person,
    ) -> Result<Vec<BrpFinding>, AppError> {
        let reason = match person.personal_data.bsn {
            Some(BsnOrNoneConfirmed::Bsn(_)) => BrpFinding::BsnUnknown,
            Some(BsnOrNoneConfirmed::NoneConfirmed) => BrpFinding::BsnNoneConfirmed,
            None => BrpFinding::BsnMissing,
        };
        let mut findings = vec![reason];

        let Some(found) = self.search_by_personal_details(person).await? else {
            return Ok(findings);
        };

        match found.as_slice() {
            [] => {}
            [bsn] => {
                findings.push(BrpFinding::BsnMatchedByPersonalDetails { bsn: bsn.clone() });
                if let Some(brp_person) = self.get_person_by_bsn(bsn).await? {
                    findings.extend(findings_for(person, &brp_person));
                }
            }
            _ => findings.push(BrpFinding::PersonalDetailsNotUnique),
        }

        Ok(findings)
    }

    /// Search the BRP on the candidate's other personal details, narrowing the
    /// query until it identifies one person.
    ///
    /// `Ok(None)` means the candidate holds too little to search on at all,
    /// which is not the same as a search that found nobody.
    async fn search_by_personal_details(
        &self,
        person: &Person,
    ) -> Result<Option<Vec<Bsn>>, AppError> {
        let Some(date_of_birth) = person.personal_data.date_of_birth.as_ref() else {
            return Ok(None);
        };
        let last_name = person.name.last_name.to_string();
        if last_name.trim().is_empty() {
            return Ok(None);
        }
        let date_of_birth = date_of_birth.format(BRP_DATE_FORMAT).to_string();
        let prefix = person
            .name
            .last_name_prefix
            .as_ref()
            .map(ToString::to_string);
        let gender = person.personal_data.gender.map(brp_gender_code);

        // Broadest first, so the common case is one request; the narrower
        // combinations only run when the broad one cannot tell two people
        // apart, and only for the details this candidate actually has.
        let mut narrowings: Vec<(Option<String>, Option<&str>)> = vec![(None, None)];
        if gender.is_some() {
            narrowings.push((None, gender));
        }
        if prefix.is_some() {
            narrowings.push((prefix.clone(), None));
            if gender.is_some() {
                narrowings.push((prefix, gender));
            }
        }

        let mut found = Vec::new();
        for (prefix, gender) in narrowings {
            found = self
                .get_persons(&BrpQuery::SearchByLastNameAndDateOfBirth {
                    last_name: last_name.clone(),
                    date_of_birth: date_of_birth.clone(),
                    last_name_prefix: prefix,
                    gender: gender.map(str::to_string),
                    // A deceased candidate has to stay findable.
                    include_deceased: true,
                    fields: vec![BrpField::Bsn],
                })
                .await?
                .into_iter()
                // A number this application cannot read is a number it cannot
                // look anyone up by either.
                .filter_map(|brp_person| brp_person.bsn?.parse::<Bsn>().ok())
                .collect();

            // Narrowing a search that found nobody cannot find anyone, and
            // once it identifies one person there is nothing left to narrow.
            if found.len() <= 1 {
                break;
            }
        }

        Ok(Some(found))
    }

    /// The full record of one person, for comparing every checked field after
    /// a search identified them.
    async fn get_person_by_bsn(&self, bsn: &Bsn) -> Result<Option<BrpPerson>, AppError> {
        Ok(self
            .get_persons(&BrpQuery::ConsultWithBsn {
                bsn: vec![bsn.clone()],
                fields: CANDIDATE_FIELDS.to_vec(),
            })
            .await?
            .into_iter()
            .next())
    }
}

/// The candidate's burgerservicenummer, if they have one recorded.
fn bsn_of(person: &Person) -> Option<&Bsn> {
    match &person.personal_data.bsn {
        Some(BsnOrNoneConfirmed::Bsn(bsn)) => Some(bsn),
        _ => None,
    }
}

/// Everything the BRP says about one candidate that the committee should see.
fn findings_for(person: &Person, brp_person: &BrpPerson) -> Vec<BrpFinding> {
    let mut findings = Vec::new();

    findings.extend(last_name_prefix_finding(person, brp_person));
    findings.extend(last_name_finding(person, brp_person));
    findings.extend(initials_finding(person, brp_person));
    findings.extend(gender_finding(person, brp_person));
    findings.extend(date_of_birth_finding(person, brp_person));
    findings.extend(place_of_residence_finding(person, brp_person));
    findings.extend(eligibility_findings(brp_person));

    findings
}

/// Compare one candidate value with the BRP's, keeping the three outcomes
/// apart: no value, an uninterpretable value, or a difference.
///
/// `into_value` carries the parsed value into the finding, so a difference is
/// reported in the type of the field it is about.
fn compare<T>(
    field: BrpCheckedField,
    ours: Option<&T>,
    theirs: Option<&str>,
    into_value: impl FnOnce(T) -> BrpValue,
) -> Option<BrpFinding>
where
    T: FromStr + PartialEq,
{
    // A field the BRP left out is as unverified as one it returned empty.
    let raw = theirs.map(str::trim).unwrap_or_default();
    if raw.is_empty() {
        return Some(BrpFinding::MissingInBrp { field });
    }

    let Ok(parsed) = raw.parse::<T>() else {
        return Some(BrpFinding::Unparsable {
            field,
            brp_value: raw.to_string(),
        });
    };

    match ours {
        Some(ours) if *ours == parsed => None,
        _ => Some(BrpFinding::Mismatch {
            brp_value: into_value(parsed),
        }),
    }
}

fn last_name_finding(person: &Person, brp_person: &BrpPerson) -> Option<BrpFinding> {
    compare(
        BrpCheckedField::LastName,
        Some(&person.name.last_name),
        brp_person
            .name
            .as_ref()
            .and_then(|name| name.last_name.as_deref()),
        BrpValue::LastName,
    )
}

/// Unlike every other field, an absent `voorvoegsel` is a value rather than a
/// gap: only a candidate who has one is told the BRP holds none.
fn last_name_prefix_finding(person: &Person, brp_person: &BrpPerson) -> Option<BrpFinding> {
    let field = BrpCheckedField::LastNamePrefix;
    let ours = person.name.last_name_prefix.as_ref();

    let theirs = brp_person
        .name
        .as_ref()
        .and_then(|name| name.last_name_prefix.as_deref())
        .map(str::trim)
        .filter(|prefix| !prefix.is_empty());

    let Some(theirs) = theirs else {
        return ours.map(|_| BrpFinding::MissingInBrp { field });
    };

    // Uncomparable is not the same as differing.
    let Ok(brp_prefix) = theirs.parse::<LastNamePrefix>() else {
        return Some(BrpFinding::Unparsable {
            field,
            brp_value: theirs.to_string(),
        });
    };

    (ours != Some(&brp_prefix)).then_some(BrpFinding::Mismatch {
        brp_value: BrpValue::LastNamePrefix(brp_prefix),
    })
}

fn initials_finding(person: &Person, brp_person: &BrpPerson) -> Option<BrpFinding> {
    compare(
        BrpCheckedField::Initials,
        Some(&person.name.initials),
        brp_person
            .name
            .as_ref()
            .and_then(|name| name.initials.as_deref()),
        BrpValue::Initials,
    )
}

/// Only compared when the candidate supplied a gender, since that is also when
/// it is printed on the list.
fn gender_finding(person: &Person, brp_person: &BrpPerson) -> Option<BrpFinding> {
    let ours = person.personal_data.gender.as_ref()?;
    let field = BrpCheckedField::Gender;

    // "O" is the BRP's own "unknown", not a value we failed to parse.
    match brp_person.gender_code() {
        Some(code) if code.eq_ignore_ascii_case(GENDER_UNKNOWN_CODE) => {
            Some(BrpFinding::MissingInBrp { field })
        }
        code => compare(field, Some(ours), code, BrpValue::Gender),
    }
}

fn date_of_birth_finding(person: &Person, brp_person: &BrpPerson) -> Option<BrpFinding> {
    let field = BrpCheckedField::DateOfBirth;
    // A partial date (a year without a month, say) comes back without a
    // `datum`, so there is nothing to compare.
    let Some(raw) = brp_person.date_of_birth() else {
        return Some(BrpFinding::MissingInBrp { field });
    };

    match parse_brp_date(raw) {
        None => Some(BrpFinding::Unparsable {
            field,
            brp_value: raw.to_string(),
        }),
        Some(date) if person.personal_data.date_of_birth == Some(DateOfBirth::from(date)) => None,
        Some(date) => Some(BrpFinding::Mismatch {
            brp_value: BrpValue::DateOfBirth(date),
        }),
    }
}

/// The `woonplaats` is the one address element printed on the candidate list.
/// Each residence shape that carries no `woonplaats` gets its own finding, so
/// "lives abroad" is never reported as "residence unknown".
fn place_of_residence_finding(person: &Person, brp_person: &BrpPerson) -> Option<BrpFinding> {
    let field = BrpCheckedField::PlaceOfResidence;

    match &brp_person.residence {
        Some(BrpResidence::Address { address }) => compare(
            field,
            person.personal_data.place_of_residence.as_ref(),
            address
                .as_ref()
                .and_then(|address| address.place_of_residence.as_deref()),
            BrpValue::PlaceOfResidence,
        ),
        Some(BrpResidence::Abroad) => Some(BrpFinding::ResidenceAbroad),
        Some(BrpResidence::Location) => Some(BrpFinding::ResidenceWithoutAddress),
        Some(BrpResidence::Unknown | BrpResidence::Other) | None => {
            Some(BrpFinding::ResidenceUnknown)
        }
    }
}

/// Whether the BRP shows this candidate can be elected at all: article 56 of
/// the Grondwet requires a living Dutch national who is not excluded from the
/// right to vote. The age requirement is checked against the candidate's own
/// data, not here.
fn eligibility_findings(brp_person: &BrpPerson) -> Vec<BrpFinding> {
    let mut findings = Vec::new();

    if brp_person.is_deceased() {
        findings.push(BrpFinding::Deceased {
            date_of_death: brp_person.date_of_death().and_then(parse_brp_date),
        });
    }

    if !brp_person.is_dutch() {
        findings.push(BrpFinding::NotDutch);
    }

    if brp_person.is_excluded_from_suffrage() {
        findings.push(BrpFinding::ExcludedFromSuffrage);
    }

    findings
}

fn parse_brp_date(raw: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(raw.trim(), BRP_DATE_FORMAT).ok()
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum BrpQuery {
    #[serde(rename = "RaadpleegMetBurgerservicenummer")]
    ConsultWithBsn {
        #[serde(rename = "burgerservicenummer")]
        bsn: Vec<Bsn>,
        fields: Vec<BrpField>,
    },
    /// Search on personal details, for a candidate whose burgerservicenummer
    /// resolves to nobody. The BRP matches `geslachtsnaam` exactly and expects
    /// the prefix separately, and it leaves deceased people out unless asked.
    #[serde(rename = "ZoekMetGeslachtsnaamEnGeboortedatum")]
    SearchByLastNameAndDateOfBirth {
        #[serde(rename = "geslachtsnaam")]
        last_name: String,
        #[serde(rename = "geboortedatum")]
        date_of_birth: String,
        #[serde(rename = "voorvoegsel", skip_serializing_if = "Option::is_none")]
        last_name_prefix: Option<String>,
        #[serde(rename = "geslacht", skip_serializing_if = "Option::is_none")]
        gender: Option<String>,
        #[serde(rename = "inclusiefOverledenPersonen")]
        include_deceased: bool,
        fields: Vec<BrpField>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum BrpResponse {
    #[serde(rename = "RaadpleegMetBurgerservicenummer")]
    ConsultWithBsn {
        #[serde(rename = "personen")]
        persons: Vec<BrpPerson>,
    },
    #[serde(rename = "ZoekMetGeslachtsnaamEnGeboortedatum")]
    SearchByLastNameAndDateOfBirth {
        #[serde(rename = "personen")]
        persons: Vec<BrpPerson>,
    },
}

#[cfg(test)]
mod tests {
    use axum::{Router, routing::post};
    use serde_json::{Value, json};
    use tokio::net::TcpListener;

    use super::*;
    use crate::{
        brp_stub::{BrpStub, matching_record},
        constants,
        structs::{brp::BrpFinding, persons::PersonId},
        test_utils::{sample_person, sample_person_from_brp},
    };

    async fn findings_for_record(person: &Person, record: Value) -> Vec<BrpFinding> {
        let stub = BrpStub::serving(vec![record]).await;
        let results = stub
            .client
            .verify_batch(std::slice::from_ref(person))
            .await
            .expect("the stub BRP answers");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, person.id);
        results[0].1.clone()
    }

    /// `count` distinct burgerservicenummers that pass the eleven-proof.
    fn valid_bsns(count: usize) -> Vec<Bsn> {
        let bsns: Vec<Bsn> = (100_000_000u32..)
            .filter_map(|number| number.to_string().parse::<Bsn>().ok())
            .take(count)
            .collect();
        assert_eq!(bsns.len(), count);
        bsns
    }

    fn candidate() -> (Person, String) {
        let person = sample_person_from_brp();
        let bsn = bsn_of(&person)
            .expect("the fixture has a BSN")
            .expose()
            .to_string();
        (person, bsn)
    }

    #[tokio::test]
    async fn a_matching_candidate_produces_no_findings() {
        let (person, bsn) = candidate();

        let findings = findings_for_record(&person, matching_record(&bsn)).await;

        assert!(
            findings.is_empty(),
            "expected no findings, got {findings:?}"
        );
    }

    #[tokio::test]
    async fn the_correspondence_address_is_not_checked() {
        let (mut person, bsn) = candidate();
        // Only the woonplaats is verified.
        person.address.street_name = Some("Heel Andere Laan".parse().unwrap());
        person.address.house_number = Some("999".parse().unwrap());
        person.address.house_number_addition = Some("Z".parse().unwrap());
        person.address.postal_code = Some("1234AB".parse().unwrap());
        person.address.locality = Some("Amsterdam".parse().unwrap());

        let findings = findings_for_record(&person, matching_record(&bsn)).await;

        assert!(
            findings.is_empty(),
            "expected no findings, got {findings:?}"
        );
    }

    #[tokio::test]
    async fn a_different_last_name_is_reported_on_its_own() {
        let (person, bsn) = candidate();
        let mut record = matching_record(&bsn);
        record["naam"]["geslachtsnaam"] = json!("Bruijn");

        let findings = findings_for_record(&person, record).await;

        assert_eq!(
            findings,
            vec![BrpFinding::Mismatch {
                brp_value: BrpValue::LastName("Bruijn".parse().unwrap()),
            }]
        );
    }

    #[tokio::test]
    async fn a_different_prefix_is_reported_apart_from_the_last_name() {
        let (person, bsn) = candidate();
        let mut record = matching_record(&bsn);
        record["naam"]["voorvoegsel"] = json!("van der");

        let findings = findings_for_record(&person, record).await;

        assert_eq!(
            findings,
            vec![BrpFinding::Mismatch {
                brp_value: BrpValue::LastNamePrefix("van der".parse().unwrap()),
            }]
        );
    }

    #[tokio::test]
    async fn a_prefix_the_brp_does_not_have_is_reported_as_missing_there() {
        let (person, bsn) = candidate();
        let mut record = matching_record(&bsn);
        record["naam"]["voorvoegsel"] = json!("");

        let findings = findings_for_record(&person, record).await;

        assert_eq!(
            findings,
            vec![BrpFinding::MissingInBrp {
                field: BrpCheckedField::LastNamePrefix,
            }]
        );
    }

    #[tokio::test]
    async fn a_candidate_without_a_prefix_is_not_told_the_brp_holds_none() {
        let (mut person, bsn) = candidate();
        person.name.last_name_prefix = None;
        let mut record = matching_record(&bsn);
        record["naam"]["voorvoegsel"] = json!("");

        let findings = findings_for_record(&person, record).await;

        assert!(
            findings.is_empty(),
            "expected no findings, got {findings:?}"
        );
    }

    #[tokio::test]
    async fn a_different_place_of_residence_is_reported() {
        let (person, bsn) = candidate();
        let mut record = matching_record(&bsn);
        record["verblijfplaats"]["verblijfadres"]["woonplaats"] = json!("Amsterdam");

        let findings = findings_for_record(&person, record).await;

        assert_eq!(
            findings,
            vec![BrpFinding::Mismatch {
                brp_value: BrpValue::PlaceOfResidence("Amsterdam".parse().unwrap()),
            }]
        );
    }

    #[tokio::test]
    async fn a_brp_value_that_cannot_be_parsed_is_not_reported_as_a_difference() {
        let (person, bsn) = candidate();
        let mut record = matching_record(&bsn);
        // The BRP allows initials our own type rejects. That is a problem
        // reading the BRP, not a claim that the party filled in the wrong ones.
        record["naam"]["voorletters"] = json!("T4");

        let findings = findings_for_record(&person, record).await;

        assert_eq!(
            findings,
            vec![BrpFinding::Unparsable {
                field: BrpCheckedField::Initials,
                brp_value: "T4".to_string(),
            }]
        );
    }

    #[tokio::test]
    async fn an_unparsable_date_of_birth_is_reported_as_unparsable() {
        let (person, bsn) = candidate();
        let mut record = matching_record(&bsn);
        record["geboorte"]["datum"]["datum"] = json!("11-12-1990");

        let findings = findings_for_record(&person, record).await;

        assert_eq!(
            findings,
            vec![BrpFinding::Unparsable {
                field: BrpCheckedField::DateOfBirth,
                brp_value: "11-12-1990".to_string(),
            }]
        );
    }

    #[tokio::test]
    async fn a_gender_the_brp_does_not_know_is_missing_rather_than_unparsable() {
        let (person, bsn) = candidate();
        let mut record = matching_record(&bsn);
        record["geslacht"]["code"] = json!("O");

        let findings = findings_for_record(&person, record).await;

        assert_eq!(
            findings,
            vec![BrpFinding::MissingInBrp {
                field: BrpCheckedField::Gender
            }]
        );
    }

    #[tokio::test]
    async fn a_field_the_brp_left_out_is_reported_rather_than_passed_silently() {
        let (person, bsn) = candidate();
        let mut record = matching_record(&bsn);
        // Saying nothing would read as "verified and correct".
        record["naam"] = json!({ "geslachtsnaam": "Bruin", "voorvoegsel": "de" });

        let findings = findings_for_record(&person, record).await;

        assert_eq!(
            findings,
            vec![BrpFinding::MissingInBrp {
                field: BrpCheckedField::Initials
            }]
        );
    }

    #[tokio::test]
    async fn a_candidate_without_a_gender_is_not_compared_on_it() {
        let (mut person, bsn) = candidate();
        person.personal_data.gender = None;
        let mut record = matching_record(&bsn);
        record["geslacht"]["code"] = json!("M");

        let findings = findings_for_record(&person, record).await;

        assert!(
            findings.is_empty(),
            "expected no findings, got {findings:?}"
        );
    }

    #[tokio::test]
    async fn a_burgerservicenummer_that_resolves_to_nobody_is_searched_around() {
        let (person, _) = candidate();

        // A typo in the burgerservicenummer: nobody has the one on the list,
        // but the candidate's other details identify exactly one person.
        let findings = findings_for_record(&person, matching_record("999992806")).await;

        assert_eq!(
            findings,
            vec![
                BrpFinding::BsnUnknown,
                BrpFinding::BsnMatchedByPersonalDetails {
                    bsn: "999992806".parse().unwrap()
                },
            ],
            "the number is reported as wrong and the rest still compared"
        );
    }

    #[tokio::test]
    async fn a_burgerservicenummer_that_resolves_to_nobody_and_no_match_is_reported_as_unknown() {
        let (mut person, _) = candidate();
        // Nothing in the BRP shares this date of birth.
        person.personal_data.date_of_birth = Some("11-12-1991".parse().unwrap());

        let findings = findings_for_record(&person, matching_record("999992806")).await;

        assert_eq!(findings, vec![BrpFinding::BsnUnknown]);
    }

    #[tokio::test]
    async fn a_candidate_found_by_their_details_is_compared_against_that_person() {
        let (mut person, _) = candidate();
        person.personal_data.bsn = None;
        person.personal_data.place_of_residence = Some("Amsterdam".parse().unwrap());

        let findings = findings_for_record(&person, matching_record("999992806")).await;

        assert_eq!(
            findings,
            vec![
                BrpFinding::BsnMissing,
                BrpFinding::BsnMatchedByPersonalDetails {
                    bsn: "999992806".parse().unwrap()
                },
                BrpFinding::Mismatch {
                    brp_value: BrpValue::PlaceOfResidence("Utrecht".parse().unwrap()),
                },
            ]
        );
    }

    #[tokio::test]
    async fn details_matching_more_than_one_person_are_not_compared_against_either() {
        let (mut person, _) = candidate();
        person.personal_data.bsn = None;
        // Two people share the candidate's name, prefix, gender and date of
        // birth, so no narrowing can tell them apart.
        let stub = BrpStub::serving(vec![
            matching_record("999992806"),
            matching_record("999993653"),
        ])
        .await;

        let results = stub
            .client
            .verify_batch(std::slice::from_ref(&person))
            .await
            .unwrap();

        assert_eq!(
            results[0].1,
            vec![BrpFinding::BsnMissing, BrpFinding::PersonalDetailsNotUnique]
        );
    }

    #[tokio::test]
    async fn the_search_narrows_on_the_details_it_has_before_giving_up() {
        let (mut person, _) = candidate();
        person.personal_data.bsn = None;
        // Same name and date of birth, different gender: the broad search
        // returns both, and narrowing on gender picks one.
        let mut other = matching_record("999993653");
        other["geslacht"]["code"] = json!("M");
        let stub = BrpStub::serving(vec![matching_record("999992806"), other]).await;

        let results = stub
            .client
            .verify_batch(std::slice::from_ref(&person))
            .await
            .unwrap();

        assert_eq!(
            results[0].1,
            vec![
                BrpFinding::BsnMissing,
                BrpFinding::BsnMatchedByPersonalDetails {
                    bsn: "999992806".parse().unwrap()
                },
            ]
        );
        let searches = stub.queries_of_type("ZoekMetGeslachtsnaamEnGeboortedatum");
        assert_eq!(searches.len(), 2, "broad first, then narrowed by gender");
        assert!(searches[0]["geslacht"].is_null());
        assert_eq!(searches[1]["geslacht"], json!("V"));
        // The prefix travels apart from the name, as the BRP expects.
        assert_eq!(searches[0]["geslachtsnaam"], json!("Bruin"));
        assert_eq!(searches[0]["inclusiefOverledenPersonen"], json!(true));
    }

    #[tokio::test]
    async fn a_candidate_with_too_little_to_search_on_is_not_searched_for() {
        let (mut person, _) = candidate();
        person.personal_data.bsn = None;
        person.personal_data.date_of_birth = None;

        let stub = BrpStub::serving(vec![matching_record("999992806")]).await;
        let results = stub
            .client
            .verify_batch(std::slice::from_ref(&person))
            .await
            .unwrap();

        assert_eq!(results[0].1, vec![BrpFinding::BsnMissing]);
        assert_eq!(stub.query_count(), 0, "there was nothing to ask");
    }

    #[tokio::test]
    async fn a_bsn_matching_more_than_one_person_is_reported() {
        let (person, bsn) = candidate();
        let stub = BrpStub::serving(vec![matching_record(&bsn), matching_record(&bsn)]).await;

        let results = stub
            .client
            .verify_batch(std::slice::from_ref(&person))
            .await
            .unwrap();

        assert_eq!(results[0].1, vec![BrpFinding::BsnNotUnique]);
    }

    #[tokio::test]
    async fn why_a_candidate_could_not_be_looked_up_is_reported() {
        let mut without = sample_person(PersonId::new());
        without.personal_data.bsn = None;
        let mut none_confirmed = sample_person(PersonId::new());
        none_confirmed.personal_data.bsn = Some(BsnOrNoneConfirmed::NoneConfirmed);

        let stub = BrpStub::serving(Vec::new()).await;
        let results = stub
            .client
            .verify_batch(&[without.clone(), none_confirmed.clone()])
            .await
            .unwrap();

        // Neither is in the BRP under any of their details, so the reason the
        // lookup failed is all there is to report.
        let findings: Vec<_> = results.into_iter().collect();
        assert!(findings.contains(&(without.id, vec![BrpFinding::BsnMissing])));
        assert!(findings.contains(&(none_confirmed.id, vec![BrpFinding::BsnNoneConfirmed])));
        assert_eq!(
            stub.queries_of_type("RaadpleegMetBurgerservicenummer")
                .len(),
            0,
            "neither candidate has a burgerservicenummer to look up"
        );
    }

    #[tokio::test]
    async fn eligibility_is_checked_against_the_brp() {
        let (person, bsn) = candidate();

        let mut deceased = matching_record(&bsn);
        deceased["overlijden"] = json!({ "datum": { "datum": "2026-01-31" } });
        assert!(
            findings_for_record(&person, deceased)
                .await
                .contains(&BrpFinding::Deceased {
                    date_of_death: NaiveDate::from_ymd_opt(2026, 1, 31)
                })
        );

        let mut not_dutch = matching_record(&bsn);
        not_dutch["nationaliteiten"] = json!([{ "nationaliteit": { "code": "0031" } }]);
        assert!(
            findings_for_record(&person, not_dutch)
                .await
                .contains(&BrpFinding::NotDutch)
        );

        let mut excluded = matching_record(&bsn);
        excluded["uitsluitingKiesrecht"]["uitgeslotenVanKiesrecht"] = json!(true);
        assert!(
            findings_for_record(&person, excluded)
                .await
                .contains(&BrpFinding::ExcludedFromSuffrage)
        );
    }

    #[tokio::test]
    async fn a_date_of_birth_the_brp_does_not_hold_in_full_is_reported_as_missing() {
        let (person, bsn) = candidate();
        let mut record = matching_record(&bsn);
        // A partial date comes back without a `datum`.
        record["geboorte"]["datum"] = json!({ "type": "JaarDatum", "jaar": 1990 });

        let findings = findings_for_record(&person, record).await;

        assert_eq!(
            findings,
            vec![BrpFinding::MissingInBrp {
                field: BrpCheckedField::DateOfBirth
            }]
        );
    }

    #[tokio::test]
    async fn a_death_the_brp_holds_no_full_date_for_is_still_reported() {
        let (person, bsn) = candidate();
        let mut record = matching_record(&bsn);
        // The BRP records the death, but only the year.
        record["overlijden"] = json!({ "datum": { "type": "JaarDatum", "jaar": 2014 } });

        let findings = findings_for_record(&person, record).await;

        assert_eq!(
            findings,
            vec![BrpFinding::Deceased {
                date_of_death: None
            }]
        );
    }

    #[tokio::test]
    async fn each_residence_without_a_woonplaats_gets_its_own_finding() {
        let (person, bsn) = candidate();

        for (residence_type, expected) in [
            ("VerblijfplaatsBuitenland", BrpFinding::ResidenceAbroad),
            ("Locatie", BrpFinding::ResidenceWithoutAddress),
            ("VerblijfplaatsOnbekend", BrpFinding::ResidenceUnknown),
            ("SomethingNew", BrpFinding::ResidenceUnknown),
        ] {
            let mut record = matching_record(&bsn);
            record["verblijfplaats"] = json!({ "type": residence_type });

            let findings = findings_for_record(&person, record).await;

            assert_eq!(
                findings,
                vec![expected],
                "unexpected findings for verblijfplaats {residence_type}"
            );
        }
    }

    #[tokio::test]
    async fn a_batch_is_looked_up_in_a_single_request() {
        let candidates: Vec<Person> = valid_bsns(BRP_BSN_BATCH_SIZE)
            .into_iter()
            .map(|bsn| {
                let mut person = sample_person(PersonId::new());
                person.personal_data.bsn = Some(BsnOrNoneConfirmed::Bsn(bsn));
                person
            })
            .collect();

        let stub = BrpStub::serving(Vec::new()).await;
        let results = stub.client.verify_batch(&candidates).await.unwrap();

        let lookups = stub.queries_of_type("RaadpleegMetBurgerservicenummer");
        assert_eq!(lookups.len(), 1);
        assert_eq!(
            lookups[0]["burgerservicenummer"].as_array().map(Vec::len),
            Some(BRP_BSN_BATCH_SIZE),
            "all candidates in the batch should travel in one request"
        );
        // None of them is in the stub's list.
        assert_eq!(results.len(), BRP_BSN_BATCH_SIZE);
        assert!(results.iter().all(|(_, f)| f == &[BrpFinding::BsnUnknown]));
    }

    #[tokio::test]
    async fn a_batch_larger_than_the_brp_accepts_is_refused() {
        let candidates: Vec<Person> = (0..=BRP_BSN_BATCH_SIZE)
            .map(|_| sample_person(PersonId::new()))
            .collect();

        let stub = BrpStub::serving(Vec::new()).await;
        let result = stub.client.verify_batch(&candidates).await;

        assert!(matches!(result, Err(AppError::BrpError(_))), "{result:?}");
    }

    #[tokio::test]
    async fn an_unreachable_brp_is_an_error_rather_than_an_empty_result() {
        let (person, _) = candidate();
        // Port 1 on loopback refuses connections.
        let client = BrpClient::new_for_test("http://127.0.0.1:1");

        let result = client.verify_batch(std::slice::from_ref(&person)).await;

        assert!(
            result.is_err(),
            "an unreachable BRP must not look like a candidate with no findings"
        );
    }

    #[tokio::test]
    async fn a_brp_error_response_is_an_error() {
        let router = Router::new().route(
            &format!("/{}", constants::BRP_PERSONS_ENDPOINT),
            post(|| async { axum::http::StatusCode::SERVICE_UNAVAILABLE }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

        let (person, _) = candidate();
        let client = BrpClient::new_for_test(&format!("http://{addr}"));
        let result = client.verify_batch(std::slice::from_ref(&person)).await;

        server.abort();
        assert!(result.is_err(), "a 503 from the BRP must not be ignored");
    }

    /// Smoke test against the real mock, which `cargo test` does not start.
    #[tokio::test]
    #[ignore = "requires the personen-mock container: docker compose up -d personen-mock"]
    async fn brp_mock_answers_a_query() {
        let client = BrpClient::new_for_test("http://localhost:5010");
        let query = BrpQuery::ConsultWithBsn {
            bsn: vec!["999993653".parse().unwrap()],
            fields: CANDIDATE_FIELDS.to_vec(),
        };

        let persons = client.get_persons(&query).await.expect("mock answers");

        assert_eq!(persons.len(), 1);
        assert_eq!(persons[0].bsn.as_deref(), Some("999993653"));
    }
}
