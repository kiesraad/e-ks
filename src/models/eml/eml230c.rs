//! The EML 230c total established candidate lists export, built with [`eml_nl`].
//!
//! Unlike the 230b, this contains every contest of the election in one
//! document, and includes the candidates' private details (mailing address
//! and representative).

use std::{
    collections::{HashMap, HashSet},
    num::NonZeroU64,
    sync::LazyLock,
};

use eml_nl::{
    common::{Agent, Contact},
    documents::candidate_lists::{
        CandidateListsCandidate, CandidateListsContest, CandidateListsType,
    },
    utils::NameShortCode,
};
use regex::Regex;

use crate::{
    AppError, CsbStream, ElectionConfig, StreamId,
    models::eml::{
        candidate_id, candidate_identifier,
        eml230b::{
            candidate_lists_document, contest_affiliations, contests, public_candidate_details,
        },
    },
    structs::{
        common::FullName,
        persons::{Person, PersonId},
    },
};

/// Maximum length of a `NameShortCode`, in bytes as checked by [`eml_nl`]
const MAX_SHORT_CODE_LEN: usize = 15;

static LETTERS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\p{L}+").expect("valid letters regex"));

/// Build the EML 230c established candidate lists XML for all contests of
/// the election
///
/// `Ok(None)` when no group qualifies
pub fn eml230c(
    election: &ElectionConfig,
    numbered_groups: &[(NonZeroU64, &CsbStream)],
) -> Result<Option<Vec<u8>>, AppError> {
    let mut short_codes = ShortCodes::default();
    let mut eml_contests = Vec::new();

    for (contest_identifier, district) in contests(election)? {
        let affiliations =
            contest_affiliations(district, numbered_groups, |store, position, person| {
                short_codes.candidate(store.stream_id, position, person)
            })?;
        if affiliations.is_empty() {
            continue;
        }

        eml_contests.push(
            CandidateListsContest::builder()
                .identifier(contest_identifier)
                .affiliations(affiliations)
                .build()?,
        );
    }

    if eml_contests.is_empty() {
        return Ok(None);
    }

    Ok(Some(candidate_lists_document(
        election,
        CandidateListsType::Multiple,
        eml_contests,
    )?))
}

/// The short codes handed out so far in a 230c document
///
/// A candidate is fully described at their first appearance, under a short
/// code that is unique within the election, and only referred to by that
/// short code in later contests.
#[derive(Default)]
struct ShortCodes {
    assigned: HashMap<(StreamId, PersonId), NameShortCode>,
    taken: HashSet<NameShortCode>,
}

impl ShortCodes {
    fn candidate(
        &mut self,
        stream_id: StreamId,
        position: usize,
        person: &Person,
    ) -> Result<CandidateListsCandidate, AppError> {
        if let Some(short_code) = self.assigned.get(&(stream_id, person.id)) {
            return Ok(CandidateListsCandidate::reference(
                candidate_id(position)?,
                short_code.clone(),
            ));
        }

        let short_code = self.new_short_code(&person.name)?;
        self.assigned
            .insert((stream_id, person.id), short_code.clone());

        let builder = CandidateListsCandidate::builder()
            .identifier(candidate_identifier(position)?.with_short_code(short_code));
        let mut builder = public_candidate_details(builder, person)?;
        if let Some(contact) = Option::<Contact>::from(person) {
            builder = builder.contact(contact);
        }
        if let Some(agent) = Option::<Agent>::from(person) {
            builder = builder.agent(agent);
        }

        Ok(builder.build()?)
    }

    /// The letters of the last name (without prefix) followed by those of the
    /// initials, e.g. `DijkAB` for "A.B. van Dijk", numbered from 2 onwards
    /// when already taken
    fn new_short_code(&mut self, name: &FullName) -> Result<NameShortCode, AppError> {
        let initials = name
            .initials
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        let letters: String = LETTERS
            .find_iter(&format!("{}{initials}", name.last_name))
            .map(|m| m.as_str())
            .collect();

        for number in 1u32.. {
            let suffix = if number == 1 {
                String::new()
            } else {
                number.to_string()
            };

            let mut code = String::new();
            for c in letters.chars() {
                if code.len() + c.len_utf8() + suffix.len() > MAX_SHORT_CODE_LEN {
                    break;
                }
                code.push(c);
            }
            code.push_str(&suffix);
            if code.is_empty() {
                continue;
            }

            let code = NameShortCode::new(code)?;
            if self.taken.insert(code.clone()) {
                return Ok(code);
            }
        }

        Err(AppError::InternalServerError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{assert_matches, collections::BTreeSet, str::FromStr};

    use eml_nl::documents::EML;

    use crate::{
        ElectoralDistrict, Province, WaterCouncil,
        models::eml::remove_variable_fields,
        projection::WithCorrections,
        structs::{
            candidate_lists::CandidateList, common::CountryCode, list_designation::ListDesignation,
            persons::Representative, political_groups::PoliticalGroup,
        },
        test_utils::{sample_dutch_address, sample_full_name, sample_person},
    };

    fn full_name(last_name: &str, prefix: Option<&str>, initials: &str) -> FullName {
        sample_full_name(None, last_name, prefix, initials)
    }

    fn short_code(short_codes: &mut ShortCodes, name: &FullName) -> String {
        short_codes
            .new_short_code(name)
            .unwrap()
            .value()
            .to_string()
    }

    #[test]
    fn short_code_is_last_name_and_initials_without_prefix() {
        let mut short_codes = ShortCodes::default();
        assert_eq!(
            short_code(&mut short_codes, &full_name("Dijk", Some("van"), "A.B.")),
            "DijkAB"
        );
        assert_eq!(
            short_code(&mut short_codes, &full_name("Inönü", None, "A.Ç.")),
            "InönüAÇ"
        );
        assert_eq!(
            short_code(&mut short_codes, &full_name("Groot-'t Hart", None, "J.")),
            "GroottHartJ"
        );
    }

    #[test]
    fn short_code_is_numbered_when_taken() {
        let mut short_codes = ShortCodes::default();
        let name = full_name("Jansen", None, "J.");
        assert_eq!(short_code(&mut short_codes, &name), "JansenJ");
        assert_eq!(short_code(&mut short_codes, &name), "JansenJ2");
        assert_eq!(short_code(&mut short_codes, &name), "JansenJ3");
    }

    #[test]
    fn short_code_is_truncated_to_fit_suffix() {
        let mut short_codes = ShortCodes::default();
        let name = full_name("Wolfeschlegelsteinhausen", None, "H.");
        assert_eq!(short_code(&mut short_codes, &name), "Wolfeschlegelst");
        assert_eq!(short_code(&mut short_codes, &name), "Wolfeschlegels2");

        // Non-ASCII letters take more than one byte
        let name = full_name("Çağatayöğüşçııı", None, "A.");
        assert_eq!(short_code(&mut short_codes, &name), "Çağatayöğü");
    }

    fn check_eml(response: &str, expected: &str) {
        // should parse, which also checks the short codes and references
        assert_matches!(response.parse().unwrap(), EML::CandidateLists(_));
        assert_matches!(expected.parse().unwrap(), EML::CandidateLists(_));

        assert_eq!(
            remove_variable_fields(response),
            remove_variable_fields(expected),
            "received XML:\n{}",
            response
        );
    }

    fn group(
        election: ElectionConfig,
        appellation: &str,
        lists: Vec<(BTreeSet<ElectoralDistrict>, Vec<Person>)>,
    ) -> CsbStream {
        let store = CsbStream {
            election,
            ..CsbStream::new_for_test()
        };
        store.set_political_group(PoliticalGroup {
            appellation: Some(appellation.parse().unwrap()),
            list_designation: Some(ListDesignation::Standalone),
            ..Default::default()
        });
        for (districts, candidates) in lists {
            for person in &candidates {
                if store.get_person(person.id, WithCorrections::All).is_none() {
                    store.add_person(person.clone());
                }
            }
            store.add_candidate_list(CandidateList {
                electoral_districts: districts,
                candidates: candidates.iter().map(|person| person.id).collect(),
                ..Default::default()
            });
        }
        store
    }

    fn candidate(last_name: &str) -> Person {
        let mut person = sample_person(PersonId::new());
        person.name.last_name = last_name.parse().unwrap();
        person
    }

    /// Two groups: the first with the same list in every kieskring, the second
    /// with a different person who has the same name as the first group's
    /// first candidate, so their short code gets numbered (`EenHAHA2`). With
    /// more than one kieskring, the second group has a different list in the
    /// second kieskring that also contains this person.
    fn sample_stores(election: ElectionConfig) -> Vec<CsbStream> {
        let districts = election.electoral_districts();
        let first = districts[0];
        let second = districts.get(1).copied();

        let candidate1 = candidate("Een");
        let mut candidate2 = candidate("Twee");
        candidate2.personal_data.gender = None;
        candidate2.personal_data.country = CountryCode::from_str("BE").ok();
        candidate2.representative = Some(Representative {
            name: sample_full_name(Some("Bob"), "Bouwer", Some("de"), "B."),
            address: sample_dutch_address("Nijmegen", "1234AB", "22", "c", "Bouwstraat"),
        });
        let store1 = group(
            election,
            "Kiesraad Demo",
            vec![(
                [Some(first), second].into_iter().flatten().collect(),
                vec![candidate1, candidate2],
            )],
        );

        let namesake = candidate("Een");
        let candidate3 = candidate("Drie");
        let mut lists = vec![(BTreeSet::from([first]), vec![namesake.clone(), candidate3])];
        if let Some(second) = second {
            lists.push((BTreeSet::from([second]), vec![candidate("Vier"), namesake]));
        }
        let store2 = group(election, "Andere Partij", lists);

        vec![store1, store2]
    }

    fn numbered(stores: &[CsbStream]) -> Vec<(NonZeroU64, &CsbStream)> {
        stores
            .iter()
            .enumerate()
            .map(|(index, store)| (NonZeroU64::new(index as u64 + 1).unwrap(), store))
            .collect()
    }

    fn export(election: ElectionConfig) -> String {
        let stores = sample_stores(election);
        let eml = eml230c(&election, &numbered(&stores)).unwrap().unwrap();
        String::from_utf8(eml).unwrap()
    }

    #[test]
    fn ek_export() {
        check_eml(
            &export(ElectionConfig::EK27),
            include_str!("testdata/230c-ek27.eml.xml"),
        );
    }

    #[test]
    fn ps1_export() {
        check_eml(
            &export(ElectionConfig::PS27(Province::Groningen)),
            include_str!("testdata/230c-ps27-1.eml.xml"),
        );
    }

    #[test]
    fn ps2_export() {
        check_eml(
            &export(ElectionConfig::PS27(Province::Limburg)),
            include_str!("testdata/230c-ps27-2.eml.xml"),
        );
    }

    #[test]
    fn ws_export() {
        check_eml(
            &export(ElectionConfig::WS27(WaterCouncil::Fryslan)),
            include_str!("testdata/230c-ws27.eml.xml"),
        );
    }

    #[test]
    fn no_groups_produces_no_document() {
        assert!(eml230c(&ElectionConfig::EK27, &[]).unwrap().is_none());
    }
}
