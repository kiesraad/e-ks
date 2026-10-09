//! The EML 230b established candidate lists export, built with [`eml_nl`].

use std::num::NonZeroU64;

use eml_nl::{
    common::{AuthorityIdentifier, ContestIdentifier, ManagingAuthority},
    documents::{
        EML, ElectionIdentifierBuilder,
        candidate_lists::{
            CandidateLists, CandidateListsAffiliation, CandidateListsCandidate,
            CandidateListsContest, CandidateListsType, QualifyingAddress,
        },
    },
    io::EMLWrite,
    utils::{
        AffiliationId, AffiliationType, AuthorityId, CommitteeCategory, ElectionCategory,
        PublicationLanguage,
    },
};

use crate::{
    AppError, CsbStream, ElectionConfig, ElectoralDistrict,
    models::{csb_model_inputs, eml::candidate_identifier, established_lists::EstablishedLists},
    projection::WithCorrections,
    structs::{list_designation::ListDesignation, persons::Person},
};

/// Build the EML 230b established candidate lists XML for one contest
///
/// `Ok(None)` when no group qualifies
pub fn eml230b(
    election: &ElectionConfig,
    contest_identifier: ContestIdentifier,
    district: Option<ElectoralDistrict>,
    numbered_groups: &[(NonZeroU64, &CsbStream)],
) -> Result<Option<Vec<u8>>, AppError> {
    let mut affiliations = Vec::new();
    for (position, store) in numbered_groups {
        let scrapped = store.get_scrapped();
        let Some(established) = EstablishedLists::new(store, &scrapped)? else {
            continue;
        };
        let Some((list_district, list)) = established.list(district) else {
            continue;
        };
        let candidates = csb_model_inputs::valid_candidates(store, &scrapped, list)?;

        // A blank list ("Blanco") leaves RegisteredName empty
        let is_blank = scrapped.is_appellation_scrapped()
            || store
                .get_political_group(WithCorrections::All)
                .list_designation
                == Some(ListDesignation::Blank);
        let appellation = (!is_blank)
            .then(|| store.get_appellation_with_scrapped(WithCorrections::All, &scrapped));

        affiliations.push((
            *position,
            affiliation(
                *position,
                appellation,
                &candidates,
                established.sets().affiliation_type(),
                established.sets().set_number(&list_district),
            )?,
        ));
    }

    if affiliations.is_empty() {
        return Ok(None);
    }

    // Printed in the established list order, like the I 4 report's "Geldige lijsten" section
    affiliations.sort_by_key(|(position, _)| *position);
    let affiliations: Vec<_> = affiliations.into_iter().map(|(_, a)| a).collect();

    let now = chrono::Utc::now();
    let candidate_lists = CandidateLists::builder()
        .lists_type(CandidateListsType::Single)
        .transaction_id(1)
        .managing_authority(ManagingAuthority::new(
            AuthorityIdentifier::new(AuthorityId::new("CSB")?)
                .with_name(managing_authority_name(election)),
        ))
        .issue_date(now.date_naive())
        .creation_date_time(now)
        .election_identifier(
            ElectionIdentifierBuilder::try_from(*election)?.build_for_candidate_lists()?,
        )
        .push_contest(
            CandidateListsContest::builder()
                .identifier(contest_identifier)
                .affiliations(affiliations)
                .build()?,
        )
        .build()?;

    Ok(Some(
        EML::from_candidate_lists_doc(candidate_lists).write_eml_root(true, true)?,
    ))
}

fn affiliation(
    position: NonZeroU64,
    appellation: Option<String>,
    candidates: &[(usize, Person)],
    affiliation_type: AffiliationType,
    belongs_to_set: Option<NonZeroU64>,
) -> Result<CandidateListsAffiliation, AppError> {
    let mut builder = CandidateListsAffiliation::builder()
        .id(AffiliationId::new(position))
        .affiliation_type(affiliation_type)
        .publish_gender(true)
        .publication_language(PublicationLanguage::Dutch);

    if let Some(appellation) = appellation {
        builder = builder.registered_name(appellation);
    }

    if let Some(belongs_to_set) = belongs_to_set {
        builder = builder.belongs_to_set(belongs_to_set);
    }

    for (position, person) in candidates {
        builder = builder.push_candidate(candidate(*position, person)?);
    }

    Ok(builder.build()?)
}

fn candidate(position: usize, person: &Person) -> Result<CandidateListsCandidate, AppError> {
    let mut builder = CandidateListsCandidate::builder()
        .identifier(candidate_identifier(position)?)
        .full_name(&person.name)
        .gender(&person.personal_data)
        .qualifying_address(QualifyingAddress::try_from(&person.personal_data)?);

    if let Some(date_of_birth) = person.personal_data.date_of_birth.as_ref() {
        builder = builder.date_of_birth(**date_of_birth);
    }

    Ok(builder.build()?)
}

/// The name the Kiesraad publishes for the CSB of this election
fn managing_authority_name(election: &ElectionConfig) -> String {
    // First look for a CSB for this election with a `CommitteeName`
    let election_category = ElectionCategory::from(election.election_type());
    if let Some(name) = election.electoral_districts().iter().find_map(|district| {
        district
            .committees(election_category)
            .into_iter()
            .find(|committee| committee.category == CommitteeCategory::CSB)
            .and_then(|committee| committee.name.map(|name| name.to_string()))
    }) {
        return name; // this will return e.g. "De Kiesraad" for EK
    }

    // Otherwise fall back name based on election domain
    match election.domain_title() {
        Some(domain) => format!("Centraal stembureau {domain}"),
        None => "De Kiesraad".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::eml::remove_variable_fields;
    use std::{assert_matches, collections::BTreeSet, str::FromStr};

    use crate::{
        Province, WaterCouncil,
        structs::{
            candidate_lists::CandidateList, common::CountryCode, persons::PersonId,
            political_groups::PoliticalGroup,
        },
        test_utils::sample_person,
    };

    fn check_eml(response: &str, expected: &str) {
        // should parse
        assert_matches!(response.parse().unwrap(), EML::CandidateLists(_));
        assert_matches!(expected.parse().unwrap(), EML::CandidateLists(_));

        // should match the expected document, other than the variable fields
        assert_eq!(
            remove_variable_fields(response),
            remove_variable_fields(expected),
            "received XML:\n{}",
            response
        );
    }

    /// Three established groups in `districts`: two standalone lists (one
    /// with a foreign, gender-unspecified candidate) and a blank list.
    fn sample_stores(
        election: ElectionConfig,
        districts: BTreeSet<ElectoralDistrict>,
    ) -> Vec<CsbStream> {
        let mut candidate1 = sample_person(PersonId::new());
        candidate1.name.last_name = "Candidate I".parse().unwrap();

        let mut candidate2 = sample_person(PersonId::new());
        candidate2.name.last_name = "Candidate II".parse().unwrap();
        candidate2.personal_data.gender = None;
        candidate2.personal_data.country = CountryCode::from_str("BE").ok();

        let mut candidate3 = sample_person(PersonId::new());
        candidate3.name.last_name = "Candidate III".parse().unwrap();

        let mut candidate4 = sample_person(PersonId::new());
        candidate4.name.last_name = "Candidate IV".parse().unwrap();

        let store1 = CsbStream::new_for_test_with_election(election);
        store1.set_political_group(PoliticalGroup {
            appellation: Some("Kiesraad Demo".parse().unwrap()),
            list_designation: Some(ListDesignation::Standalone),
            ..Default::default()
        });
        store1.add_person(candidate1.clone());
        store1.add_person(candidate2.clone());
        store1.add_candidate_list(CandidateList {
            electoral_districts: districts.clone(),
            candidates: vec![candidate1.id, candidate2.id],
            ..Default::default()
        });

        let store2 = CsbStream::new_for_test_with_election(election);
        store2.set_political_group(PoliticalGroup {
            appellation: Some("Andere Partij".parse().unwrap()),
            list_designation: Some(ListDesignation::Standalone),
            ..Default::default()
        });
        store2.add_person(candidate3.clone());
        store2.add_candidate_list(CandidateList {
            electoral_districts: districts.clone(),
            candidates: vec![candidate3.id],
            ..Default::default()
        });

        let store3 = CsbStream::new_for_test_with_election(election);
        store3.set_political_group(PoliticalGroup {
            appellation: None,
            list_designation: Some(ListDesignation::Blank),
            ..Default::default()
        });
        store3.add_person(candidate4.clone());
        store3.add_candidate_list(CandidateList {
            electoral_districts: districts,
            candidates: vec![candidate4.id],
            ..Default::default()
        });

        vec![store1, store2, store3]
    }

    fn numbered(stores: &[CsbStream]) -> Vec<(NonZeroU64, &CsbStream)> {
        stores
            .iter()
            .enumerate()
            .map(|(index, store)| (NonZeroU64::new(index as u64 + 1).unwrap(), store))
            .collect()
    }

    /// The CSB's published name: the tree's own override for EK ("De
    /// Kiesraad"), otherwise "Centraal stembureau {domain}" -- Limburg's CSB
    /// is named after the province, not its "Maastricht" hosting kieskring,
    /// matching the Kiesraad's own 230b exports.
    #[test]
    fn managing_authority_name_uses_tree_committee_name_or_domain() {
        assert_eq!(
            managing_authority_name(&ElectionConfig::EK27),
            "De Kiesraad"
        );
        assert_eq!(
            managing_authority_name(&ElectionConfig::PS27(Province::Groningen)),
            "Centraal stembureau Groningen"
        );
        assert_eq!(
            managing_authority_name(&ElectionConfig::PS27(Province::Limburg)),
            "Centraal stembureau Limburg"
        );
        assert_eq!(
            managing_authority_name(&ElectionConfig::WS27(WaterCouncil::Fryslan)),
            "Centraal stembureau Fryslân"
        );
    }

    /// An election with multiple districts: one 230b per district,
    /// identifying the specific kieskring it was established for.
    #[test]
    fn ek_export() {
        let districts = &ElectionConfig::EK27.electoral_districts()[..2];
        let district = districts[0];
        let stores = sample_stores(ElectionConfig::EK27, districts.iter().copied().collect());
        let contest_identifier = ContestIdentifier::new(
            eml_nl::utils::ContestId::new(district.region_number().to_string()).unwrap(),
        )
        .with_name(district.title());

        let eml = eml230b(
            &ElectionConfig::EK27,
            contest_identifier,
            Some(district),
            &numbered(&stores),
        )
        .unwrap()
        .unwrap();
        check_eml(
            &String::from_utf8(eml).unwrap(),
            include_str!("testdata/230b-ek27.eml.xml"),
        );
    }

    /// A single-district election: the contest is `geen`.
    #[test]
    fn ps1_export() {
        let election = ElectionConfig::PS27(Province::Groningen);
        let stores = sample_stores(
            election,
            election.electoral_districts().iter().copied().collect(),
        );

        let eml = eml230b(
            &election,
            ContestIdentifier::geen(),
            None,
            &numbered(&stores),
        )
        .unwrap()
        .unwrap();
        check_eml(
            &String::from_utf8(eml).unwrap(),
            include_str!("testdata/230b-ps27-1.eml.xml"),
        );
    }

    /// A multi-district election: one 230b per district.
    #[test]
    fn ps2_export() {
        let election = ElectionConfig::PS27(Province::Limburg);
        let district = election.electoral_districts()[0];
        let stores = sample_stores(election, BTreeSet::from([district]));
        let contest_identifier = ContestIdentifier::new(
            eml_nl::utils::ContestId::new(district.region_number().to_string()).unwrap(),
        )
        .with_name(district.title());

        let eml = eml230b(
            &election,
            contest_identifier,
            Some(district),
            &numbered(&stores),
        )
        .unwrap()
        .unwrap();
        check_eml(
            &String::from_utf8(eml).unwrap(),
            include_str!("testdata/230b-ps27-2.eml.xml"),
        );
    }

    #[test]
    fn ws_export() {
        let election = ElectionConfig::WS27(WaterCouncil::Fryslan);
        let stores = sample_stores(
            election,
            election.electoral_districts().iter().copied().collect(),
        );

        let eml = eml230b(
            &election,
            ContestIdentifier::geen(),
            None,
            &numbered(&stores),
        )
        .unwrap()
        .unwrap();
        check_eml(
            &String::from_utf8(eml).unwrap(),
            include_str!("testdata/230b-ws27.eml.xml"),
        );
    }

    /// Lists submitted separately, but with the same candidates, form a set of
    /// equal lists
    #[test]
    fn separate_lists_with_the_same_candidates_are_a_set_of_equal_lists() {
        let districts = &ElectionConfig::EK27.electoral_districts()[..2];
        let candidate = sample_person(PersonId::new());
        let store = CsbStream::new_for_test_with_election(ElectionConfig::EK27);
        store.set_political_group(PoliticalGroup {
            appellation: Some("Kiesraad Demo".parse().unwrap()),
            list_designation: Some(ListDesignation::Standalone),
            ..Default::default()
        });
        store.add_person(candidate.clone());
        for district in districts {
            store.add_candidate_list(CandidateList {
                electoral_districts: BTreeSet::from([*district]),
                candidates: vec![candidate.id],
                ..Default::default()
            });
        }

        let eml = eml230b(
            &ElectionConfig::EK27,
            ContestIdentifier::geen(),
            Some(districts[1]),
            &[(NonZeroU64::MIN, &store)],
        )
        .unwrap()
        .unwrap();
        let eml = String::from_utf8(eml).unwrap();
        assert!(
            eml.contains("<Type>stel gelijkluidende lijsten</Type>"),
            "{eml}"
        );
        assert!(eml.contains(r#"BelongsToSet="1""#), "{eml}");
    }

    /// A contest with no established groups produces nothing.
    #[test]
    fn no_groups_produces_no_document() {
        let result = eml230b(&ElectionConfig::EK27, ContestIdentifier::geen(), None, &[]).unwrap();
        assert!(result.is_none());
    }
}
