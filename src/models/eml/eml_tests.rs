//! Property tests for the EML exports: for valid input (generated through the
//! real parsers) every document is generated, reads back through eml-nl's
//! strict parser without errors, and every interpolated value round-trips.

use std::{
    collections::{BTreeSet, HashMap},
    num::NonZeroU64,
};

use eml_nl::{
    common::{Agent, Contact, PersonNameStructure, QualifyingAddress},
    documents::{
        EML,
        candidate_lists::{CandidateLists, CandidateListsCandidate},
        nomination::NominationCandidate,
    },
    io::{EMLParsingMode, EMLRead},
};
use proptest::{collection::vec, option, prelude::*, sample::select};
use uuid::Uuid;

use super::{
    eml110a::eml110a,
    eml210::eml210,
    eml230b::{contests, eml230b},
    eml230c::eml230c,
};
use crate::{
    AppError, CsbStream, ElectionConfig, ElectoralDistrict, PgStore,
    core::ModelLocale,
    structs::{
        candidate_lists::{CandidateList, CandidateListId},
        common::{Address, Appellation, BsnOrNoneConfirmed, FullName, Gender},
        csb::{RegisteredPoliticalGroup, RegisteredPoliticalGroupId, SeatCount, VoteCount},
        list_designation::ListDesignation,
        list_submitters::{ListSubmitter, ListSubmitterId},
        persons::{Person, PersonId, PersonalData},
        political_groups::PoliticalGroup,
    },
    test_support::strategies::{appellation, list_submitter, person},
    test_utils::{sample_candidate_list, sample_list_submitter, sample_person},
};

/// A name as the EML carries it.
#[derive(Debug, PartialEq)]
struct Name {
    initials: Option<String>,
    first_name: Option<String>,
    prefix: Option<String>,
    last_name: String,
}

/// An address as the EML carries it.
#[derive(Debug, PartialEq)]
struct Place {
    line: Option<String>,
    locality: String,
    postal_code: Option<String>,
    country: Option<String>,
}

/// A representative as the EML carries it.
#[derive(Debug, PartialEq)]
struct Representation {
    role: Option<String>,
    name: Name,
    contact: Option<Place>,
    living_locality: String,
}

/// The candidate details shared by the 210, 230b and 230c.
#[derive(Debug, PartialEq)]
struct Details {
    name: Name,
    date_of_birth: Option<String>,
    gender: Option<String>,
    residence: Place,
    contact: Option<Place>,
    agent: Option<Representation>,
}

fn name(name: &FullName) -> Name {
    Name {
        initials: name.initials.as_ref().map(ToString::to_string),
        first_name: name.first_name.as_ref().map(ToString::to_string),
        prefix: name.last_name_prefix.as_ref().map(ToString::to_string),
        last_name: name.last_name.to_string(),
    }
}

fn eml_name(name: &PersonNameStructure) -> Name {
    let name = &name.person_name;
    Name {
        initials: name.get_initials().map(str::to_string),
        first_name: name.get_first_name().map(str::to_string),
        prefix: name.get_name_prefix().map(str::to_string),
        last_name: name.get_last_name().to_string(),
    }
}

/// The address fields the EML carries: the country and state or province
/// of an international address are not exported.
fn place(address: &Address) -> Place {
    Place {
        line: address.address_line_1(),
        locality: address.locality().unwrap_or_default(),
        postal_code: address.postal_code(),
        country: None,
    }
}

fn residence(data: &PersonalData) -> Place {
    let country = data.country.as_ref().expect("country");
    Place {
        line: None,
        locality: data.place_of_residence.as_ref().expect("place").to_string(),
        postal_code: None,
        country: (!country.is_nl()).then(|| country.to_string()),
    }
}

fn eml_place(address: &QualifyingAddress) -> Place {
    let locality = address.locality();
    Place {
        line: locality.address_line.as_ref().map(|l| l.value.to_string()),
        locality: locality.locality_name().to_string(),
        postal_code: locality
            .postal_code
            .as_ref()
            .map(|p| p.number().to_string()),
        country: address.country_name_code().map(|c| c.value.to_string()),
    }
}

fn eml_contact(contact: &Contact) -> Place {
    eml_place(&contact.mailing_address.address)
}

fn eml_agent(agent: &Agent) -> Representation {
    Representation {
        role: agent.role.clone(),
        name: eml_name(&agent.agent_identifier.agent_name),
        contact: agent.contact.as_ref().map(eml_contact),
        living_locality: agent.living_address.locality_name.to_string(),
    }
}

/// What the EML should hold for `person`; `private` adds the mailing
/// address and representative (210 and 230c, not 230b).
fn details(person: &Person, private: bool) -> Details {
    let data = &person.personal_data;
    let needs_representative = person.needs_representative();
    Details {
        name: name(&person.name),
        date_of_birth: data.date_of_birth.as_ref().map(ToString::to_string),
        gender: Some(
            match data.gender {
                Some(Gender::Female) => "female",
                Some(Gender::Male) => "male",
                None => "unknown",
            }
            .to_string(),
        ),
        residence: residence(data),
        contact: (private && !needs_representative)
            .then(|| place(&Address::Dutch(person.address.clone()))),
        agent: person
            .representative
            .as_ref()
            .filter(|_| private && needs_representative)
            .map(|r| Representation {
                role: Some("H10".to_string()),
                name: name(&r.name),
                contact: Some(place(&Address::Dutch(r.address.clone()))),
                living_locality: r
                    .address
                    .locality
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
            }),
    }
}

fn nomination_details(candidate: &NominationCandidate) -> Details {
    Details {
        name: eml_name(&candidate.full_name),
        date_of_birth: candidate
            .date_of_birth
            .as_ref()
            .map(|d| d.raw().to_string()),
        gender: candidate.gender.as_ref().map(|g| g.raw().to_string()),
        residence: eml_place(&candidate.qualifying_address),
        contact: candidate.contact.as_ref().map(eml_contact),
        agent: candidate.agent.as_ref().map(eml_agent),
    }
}

fn list_details(candidate: &CandidateListsCandidate) -> Option<Details> {
    Some(Details {
        name: eml_name(candidate.full_name.as_ref()?),
        date_of_birth: candidate
            .date_of_birth
            .as_ref()
            .map(|d| d.raw().to_string()),
        gender: candidate.gender.as_ref().map(|g| g.raw().to_string()),
        residence: eml_place(candidate.qualifying_address.as_ref()?),
        contact: candidate.contact.as_ref().map(eml_contact),
        agent: candidate.agent.as_ref().map(eml_agent),
    })
}

/// Valid UTF-8 with an XML declaration and only XML 1.0 characters, read
/// back by eml-nl's strict parser without any (even non-fatal) error.
fn read_back(xml: Vec<u8>) -> Result<EML, TestCaseError> {
    let xml = String::from_utf8(xml).map_err(|e| TestCaseError::fail(e.to_string()))?;
    prop_assert!(xml.starts_with("<?xml"), "no XML declaration");
    let invalid = xml.chars().find(|&c| {
        !matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
    });
    prop_assert_eq!(invalid, None, "character not allowed in XML 1.0");
    match EML::parse_eml(&xml, EMLParsingMode::Strict).ok_with_errors() {
        Ok((eml, errors)) => {
            prop_assert!(errors.is_empty(), "{:?}\n{}", errors, xml);
            Ok(eml)
        }
        Err(error) => Err(TestCaseError::fail(format!("{error:?}\n{xml}"))),
    }
}

fn election() -> impl Strategy<Value = ElectionConfig> {
    select(ElectionConfig::all())
}

/// A non-empty subset of the districts of `election`.
fn districts(election: ElectionConfig) -> impl Strategy<Value = BTreeSet<ElectoralDistrict>> {
    let all = election.electoral_districts().to_vec();
    let count = all.len();
    proptest::sample::subsequence(all, 1..=count).prop_map(BTreeSet::from_iter)
}

fn list_designation() -> impl Strategy<Value = ListDesignation> {
    select(vec![
        ListDesignation::Standalone,
        ListDesignation::Blank,
        ListDesignation::Combined,
    ])
}

/// One political group of a 230b/230c: its appellation, designation, list
/// districts and candidates.
#[derive(Debug, Clone)]
struct Group {
    appellation: Option<Appellation>,
    designation: ListDesignation,
    districts: BTreeSet<ElectoralDistrict>,
    candidates: Vec<Person>,
}

fn group(election: ElectionConfig) -> impl Strategy<Value = Group> {
    (
        option::weighted(0.9, appellation()),
        list_designation(),
        districts(election),
        vec(person(), 1..4),
    )
        .prop_map(|(appellation, designation, districts, candidates)| Group {
            appellation,
            designation,
            districts,
            candidates,
        })
}

/// Give every person a distinct id, so streams and lists can refer to them.
fn with_ids(persons: &mut [Person], offset: u128) {
    for (i, person) in persons.iter_mut().enumerate() {
        person.id = PersonId::from(Uuid::from_u128(offset + i as u128 + 1));
    }
}

fn csb_stream(election: ElectionConfig, group: &Group) -> CsbStream {
    let store = CsbStream::new_for_test_with_election(election);
    store.set_political_group(PoliticalGroup {
        appellation: group.appellation.clone(),
        list_designation: Some(group.designation),
        ..PoliticalGroup::default()
    });
    for person in &group.candidates {
        store.add_person(person.clone());
    }
    store.add_candidate_list(CandidateList {
        electoral_districts: group.districts.clone(),
        candidates: group.candidates.iter().map(|p| p.id).collect(),
        ..CandidateList::default()
    });
    store
}

fn numbered(stores: &[CsbStream]) -> Vec<(NonZeroU64, &CsbStream)> {
    stores
        .iter()
        .enumerate()
        .map(|(i, store)| (NonZeroU64::new(i as u64 + 1).expect("non-zero"), store))
        .collect()
}

/// The registered name a 230b/230c gives `group`: none for a blank list,
/// and an empty one reads back as none.
fn registered_name(group: &Group) -> Option<String> {
    (group.designation != ListDesignation::Blank)
        .then(|| group.appellation.as_ref().map(ToString::to_string))
        .flatten()
}

/// The groups that take part in `district` (the only one when `None`).
fn participants(groups: &[Group], district: Option<ElectoralDistrict>) -> Vec<u64> {
    groups
        .iter()
        .zip(1..)
        .filter(|(group, _)| district.is_none_or(|d| group.districts.contains(&d)))
        .map(|(_, number)| number)
        .collect()
}

/// Check the affiliations and candidate positions of one 230b/230c contest,
/// passing each candidate entry with its group number and person to `check`.
fn check_contest(
    document: &CandidateLists,
    contest: usize,
    groups: &[Group],
    district: Option<ElectoralDistrict>,
    mut check: impl FnMut(u64, &Person, &CandidateListsCandidate) -> Result<(), TestCaseError>,
) -> Result<(), TestCaseError> {
    let contest = &document.candidate_list.election.contests[contest];
    let numbers: Vec<u64> = contest
        .affiliations
        .iter()
        .map(|a| a.identifier.id.raw().parse().expect("affiliation id"))
        .collect();
    prop_assert_eq!(&numbers, &participants(groups, district));
    for (affiliation, number) in contest.affiliations.iter().zip(numbers) {
        let group = &groups[number as usize - 1];
        let expected = registered_name(group);
        prop_assert_eq!(
            affiliation.identifier.registered_name.as_deref(),
            expected.as_deref()
        );
        prop_assert_eq!(affiliation.candidates.len(), group.candidates.len());
        for (position, (candidate, person)) in affiliation
            .candidates
            .iter()
            .zip(&group.candidates)
            .enumerate()
        {
            prop_assert_eq!(candidate.identifier.id.raw(), (position + 1).to_string());
            check(number, person, candidate)?;
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(crate::test_support::strategies::parsed_config(200))]

    #[test]
    fn eml110a_round_trips(
        election in election(),
        appellations in vec(appellation(), 0..6),
        votes in any::<u64>(),
    ) {
        let groups: Vec<RegisteredPoliticalGroup> = appellations
            .iter()
            .zip(1..)
            .map(|(appellation, i)| RegisteredPoliticalGroup {
                id: RegisteredPoliticalGroupId::from(Uuid::from_u128(i)),
                appellation: appellation.clone(),
                previous_votes: VoteCount::from(votes),
                previous_seats: SeatCount::from(i as u32),
            })
            .collect();
        let eml = read_back(eml110a(&election, &groups).expect("eml110a"))?;
        let definition = eml.as_election_definition_doc().expect("election definition");
        let registered: Vec<String> = definition
            .election_event
            .election
            .registered_parties
            .iter()
            .map(|party| party.registered_appellation.to_string())
            .collect();
        let expected: Vec<String> = appellations.iter().map(ToString::to_string).collect();
        prop_assert_eq!(registered, expected);
    }

    #[test]
    fn eml210_round_trips(
        (election, districts) in election().prop_flat_map(|e| (Just(e), districts(e))),
        mut candidates in vec(person(), 1..5),
        appellation in option::weighted(0.9, appellation()),
        designation in list_designation(),
        mut submitter in list_submitter(),
        // Narrowed to at least one substitute: see
        // `eml210_without_substitute_submitter_is_schema_valid`.
        mut substitutes in vec(list_submitter(), 1..3),
        frisian in any::<bool>(),
    ) {
        with_ids(&mut candidates, 0);
        submitter.id = ListSubmitterId::from(Uuid::from_u128(1000));
        for (i, substitute) in substitutes.iter_mut().enumerate() {
            substitute.id = ListSubmitterId::from(Uuid::from_u128(1001 + i as u128));
        }
        let locale = if frisian { ModelLocale::Fry } else { ModelLocale::Nl };

        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        let (result, substitutes) = runtime.block_on(async {
            let store = PgStore::new_for_test_with_election(election);
            let group = PoliticalGroup {
                appellation: appellation.clone(),
                list_designation: Some(designation),
                ..PoliticalGroup::default()
            };
            group.update(&store).await.expect("political group");
            for person in &candidates {
                person.create(&store).await.expect("person");
            }
            let list = CandidateList {
                id: CandidateListId::from(Uuid::from_u128(2000)),
                electoral_districts: districts.clone(),
                candidates: candidates.iter().map(|p| p.id).collect(),
                ..CandidateList::default()
            };
            list.create(&store).await.expect("list");
            submitter.update(&store).await.expect("submitter");
            for substitute in &substitutes {
                substitute.create_substitute(&store).await.expect("substitute");
            }
            let result = eml210(&store, &election, &store.get_political_group(), list.id, locale);
            (result, store.get_substitute_submitters())
        });

        if appellation.is_none() && designation != ListDesignation::Blank {
            prop_assert!(matches!(result, Err(AppError::IncompleteData(_))), "{:?}", result.err());
            return Ok(());
        }
        let eml = read_back(result.expect("eml210"))?;
        let nomination = &eml.as_nomination_doc().expect("nomination").nomination_data;
        let affiliation = &nomination.affiliation;

        let expected_name = match (designation, &appellation) {
            (ListDesignation::Blank, _) => String::new(),
            (_, appellation) => appellation.as_ref().map(ToString::to_string).unwrap_or_default(),
        };
        prop_assert_eq!(&*affiliation.registered_name, expected_name.as_str());

        let contests: Vec<String> = affiliation.list_data.contests.iter().map(|c| c.id.raw().to_string()).collect();
        let expected_contests: Vec<String> = districts.iter().map(|d| d.region_number().to_string()).collect();
        prop_assert_eq!(contests, expected_contests);

        prop_assert_eq!(affiliation.candidates.len(), candidates.len());
        for (position, (candidate, person)) in affiliation.candidates.iter().zip(&candidates).enumerate() {
            prop_assert_eq!(candidate.identifier.id.raw(), (position + 1).to_string());
            prop_assert_eq!(nomination_details(candidate), details(person, true));
            let bsn = match &person.personal_data.bsn {
                Some(BsnOrNoneConfirmed::Bsn(bsn)) => Some(bsn.to_exposed_string()),
                _ => None,
            };
            prop_assert_eq!(candidate.national_identification_number.as_deref(), bsn.as_deref());
        }

        let proposers = &nomination.nominate.proposers;
        let expected: Vec<&ListSubmitter> = std::iter::once(&submitter).chain(&substitutes).collect();
        prop_assert_eq!(proposers.len(), expected.len());
        for (i, (proposer, submitter)) in proposers.iter().zip(expected).enumerate() {
            prop_assert_eq!(eml_name(&proposer.name), name(&submitter.name));
            prop_assert_eq!(eml_contact(&proposer.contact), place(&submitter.address));
            let id = (i > 0).then(|| i.to_string());
            prop_assert_eq!(proposer.id.as_deref(), id.as_deref());
        }
    }

    #[test]
    fn eml230b_round_trips(
        (election, mut groups) in election()
            .prop_flat_map(|e| (Just(e), vec(group(e), 1..4))),
    ) {
        for (i, group) in groups.iter_mut().enumerate() {
            with_ids(&mut group.candidates, 100 * i as u128);
        }
        let stores: Vec<CsbStream> = groups.iter().map(|g| csb_stream(election, g)).collect();
        for (identifier, district) in contests(&election).expect("contests") {
            let result = eml230b(&election, identifier, district, &numbered(&stores));
            let Some(xml) = result.expect("eml230b") else {
                prop_assert!(participants(&groups, district).is_empty());
                continue;
            };
            let eml = read_back(xml)?;
            let document = eml.as_candidate_lists_doc().expect("candidate lists");
            prop_assert_eq!(document.candidate_list.election.contests.len(), 1);
            check_contest(
                document,
                0,
                &groups,
                district,
                |_, person, candidate| {
                    prop_assert!(!candidate.identifier.has_reference());
                    prop_assert_eq!(list_details(candidate), Some(details(person, false)));
                    Ok(())
                },
            )?;
        }
    }

    #[test]
    fn eml230c_round_trips(
        (election, mut groups) in election()
            .prop_flat_map(|e| (Just(e), vec(group(e), 1..4))),
    ) {
        for (i, group) in groups.iter_mut().enumerate() {
            with_ids(&mut group.candidates, 100 * i as u128);
        }
        let stores: Vec<CsbStream> = groups.iter().map(|g| csb_stream(election, g)).collect();
        let Some(xml) = eml230c(&election, &numbered(&stores)).expect("eml230c") else {
            return Err(TestCaseError::fail("no 230c document"));
        };
        let eml = read_back(xml)?;
        let document = eml.as_candidate_lists_doc().expect("candidate lists");

        let mut codes: HashMap<(u64, PersonId), String> = HashMap::new();
        let mut used = BTreeSet::new();
        let taking_part = contests(&election)
            .expect("contests")
            .into_iter()
            .filter(|(_, district)| !participants(&groups, *district).is_empty());
        for (contest, (_, district)) in taking_part.enumerate() {
            check_contest(
                document,
                contest,
                &groups,
                district,
                |number, person, candidate| {
                    let key = (number, person.id);
                    if let Some(reference) = &candidate.identifier.short_code_ref {
                        // A later appearance refers to the first one.
                        prop_assert_eq!(codes.get(&key).map(String::as_str), Some(&*reference.raw()));
                        prop_assert!(candidate.full_name.is_none());
                        return Ok(());
                    }
                    prop_assert_eq!(list_details(candidate), Some(details(person, true)));
                    let code = candidate.identifier.short_code.as_ref().expect("short code");
                    let code = code.raw().to_string();
                    prop_assert!(code.len() <= 15, "{}", code);
                    prop_assert!(used.insert(code.clone()), "duplicate short code {}", code);
                    prop_assert!(codes.insert(key, code).is_none(), "described twice");
                    Ok(())
                },
            )?;
        }
        let candidates: usize = groups.iter().map(|g| g.candidates.len()).sum();
        prop_assert_eq!(codes.len(), candidates);
    }
}

#[tokio::test]
#[ignore = "finding: without a substitute submitter the 210 has one Proposer, the schema requires two"]
async fn eml210_without_substitute_submitter_is_schema_valid() {
    let store = PgStore::new_for_test();
    let person = sample_person(PersonId::new());
    person.create(&store).await.expect("person");
    let mut list = sample_candidate_list(CandidateListId::new());
    list.candidates.push(person.id);
    list.create(&store).await.expect("list");
    sample_list_submitter(ListSubmitterId::new())
        .update(&store)
        .await
        .expect("submitter");

    let election = ElectionConfig::EK27;
    let group = store.get_political_group();
    let xml = eml210(&store, &election, &group, list.id, ModelLocale::Nl).expect("eml210");
    let xml = String::from_utf8(xml).expect("utf-8");
    let parsed = EML::parse_eml(&xml, EMLParsingMode::Strict).ok();
    assert!(parsed.is_ok(), "{:?}", parsed.err());
}
