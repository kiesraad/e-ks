//! The EML 230b established candidate lists export, built with [`eml_nl`].

use std::{collections::HashSet, num::NonZeroU64};

use eml_nl::{
    common::{AuthorityIdentifier, ContestIdentifier, ManagingAuthority, QualifyingAddress},
    documents::{
        EML, ElectionIdentifierBuilder,
        candidate_lists::{
            CandidateLists, CandidateListsAffiliation, CandidateListsCandidate,
            CandidateListsCandidateBuilder, CandidateListsContest, CandidateListsType,
        },
    },
    io::EMLWrite,
    utils::{
        AffiliationId, AffiliationType, AuthorityId, CommitteeCategory, ContestId,
        ElectionCategory, PublicationLanguage,
    },
};

use crate::{
    AppError, CsbStream, ElectionConfig, ElectoralDistrict,
    models::{csb_model_inputs, eml::candidate_identifier},
    projection::WithCorrections,
    structs::{
        candidate_lists::{CandidateList, CandidateListId},
        list_designation::ListDesignation,
        persons::Person,
    },
};

/// The group's non-scrapped list that covers `district`, or its first
/// remaining list for a single-district election (`district: None`)
fn current_list(
    valid: &[(ElectoralDistrict, CandidateList)],
    district: Option<ElectoralDistrict>,
) -> Option<CandidateList> {
    match district {
        Some(district) => valid.iter().find(|(d, _)| *d == district),
        None => valid.first(),
    }
    .map(|(_, list)| list.clone())
}

/// Get the affiliation type for a set of lists
fn affiliation_type(
    valid: &[(ElectoralDistrict, CandidateList)],
    list_id: CandidateListId,
) -> AffiliationType {
    let distinct_lists: HashSet<CandidateListId> = valid.iter().map(|(_, list)| list.id).collect();

    if distinct_lists.len() > 1 {
        AffiliationType::GroupOfLists
    } else if valid.iter().filter(|(_, list)| list.id == list_id).count() > 1 {
        AffiliationType::SetOfEqualLists
    } else {
        AffiliationType::StandAloneList
    }
}

/// 1-based position among this group's shared lists
fn shared_list_set_number(
    valid: &[(ElectoralDistrict, CandidateList)],
    list_id: CandidateListId,
) -> Option<NonZeroU64> {
    // gather lists that occur in multiple districts
    let mut list_sets: Vec<&CandidateList> = Vec::new();
    for (_, list) in valid {
        if list_sets.iter().any(|shared| shared.id == list.id) {
            continue;
        }
        if valid.iter().filter(|(_, list)| list.id == list_id).count() > 1 {
            list_sets.push(list);
        }
    }
    list_sets.sort_by_key(|list| list.created_at);

    // position
    let position = list_sets.iter().position(|list| list.id == list_id)?;
    NonZeroU64::new(position as u64 + 1)
}

/// The contests of `election`: a single `geen` contest when it has only one
/// district, otherwise one contest per district
pub fn contests(
    election: &ElectionConfig,
) -> Result<Vec<(ContestIdentifier, Option<ElectoralDistrict>)>, AppError> {
    if election.has_only_one_district() {
        return Ok(vec![(ContestIdentifier::geen(), None)]);
    }

    election
        .electoral_districts()
        .iter()
        .map(|district| {
            let identifier =
                ContestIdentifier::new(ContestId::new(district.region_number().to_string())?)
                    .with_name(district.title());
            Ok((identifier, Some(*district)))
        })
        .collect()
}

/// Build the EML 230b established candidate lists XML for one contest
///
/// `Ok(None)` when no group qualifies
pub fn eml230b(
    election: &ElectionConfig,
    contest_identifier: ContestIdentifier,
    district: Option<ElectoralDistrict>,
    numbered_groups: &[(NonZeroU64, &CsbStream)],
) -> Result<Option<Vec<u8>>, AppError> {
    let affiliations = contest_affiliations(district, numbered_groups, |_, position, person| {
        candidate(position, person)
    })?;

    if affiliations.is_empty() {
        return Ok(None);
    }

    let contest = CandidateListsContest::builder()
        .identifier(contest_identifier)
        .affiliations(affiliations)
        .build()?;

    Ok(Some(candidate_lists_document(
        election,
        CandidateListsType::Single,
        vec![contest],
    )?))
}

/// The affiliations of the groups with a valid list in `district` (or in the
/// only district, when `None`), in the established list order, like the I 4
/// report's "Geldige lijsten" section
///
/// `candidate` is called in document order.
pub(super) fn contest_affiliations(
    district: Option<ElectoralDistrict>,
    numbered_groups: &[(NonZeroU64, &CsbStream)],
    mut candidate: impl FnMut(&CsbStream, usize, &Person) -> Result<CandidateListsCandidate, AppError>,
) -> Result<Vec<CandidateListsAffiliation>, AppError> {
    let mut numbered_groups = numbered_groups.to_vec();
    numbered_groups.sort_by_key(|(position, _)| *position);

    let mut affiliations = Vec::new();
    for (position, store) in numbered_groups {
        let scrapped = store.get_scrapped();
        let valid_lists = csb_model_inputs::valid_lists_by_district(store, &scrapped);
        let Some(list) = current_list(&valid_lists, district) else {
            continue;
        };
        let candidates = csb_model_inputs::valid_candidates(store, &scrapped, &list)?;
        if candidates.is_empty() {
            continue;
        }

        // A blank list ("Blanco") leaves RegisteredName empty
        let is_blank = scrapped.is_appellation_scrapped()
            || store
                .get_political_group(WithCorrections::All)
                .list_designation
                == Some(ListDesignation::Blank);
        let appellation = (!is_blank)
            .then(|| store.get_appellation_with_scrapped(WithCorrections::All, &scrapped));

        let candidates = candidates
            .iter()
            .map(|(position, person)| candidate(store, *position, person))
            .collect::<Result<Vec<_>, AppError>>()?;

        affiliations.push(affiliation(
            position,
            appellation,
            candidates,
            affiliation_type(&valid_lists, list.id),
            shared_list_set_number(&valid_lists, list.id),
        )?);
    }

    Ok(affiliations)
}

/// Serialize the established candidate lists of `contests` as a 230b or 230c
pub(super) fn candidate_lists_document(
    election: &ElectionConfig,
    lists_type: CandidateListsType,
    contests: Vec<CandidateListsContest>,
) -> Result<Vec<u8>, AppError> {
    let now = chrono::Utc::now();
    let candidate_lists = CandidateLists::builder()
        .lists_type(lists_type)
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
        .contests(contests)
        .build()?;

    Ok(EML::from_candidate_lists_doc(candidate_lists).write_eml_root(true, true)?)
}

fn affiliation(
    position: NonZeroU64,
    appellation: Option<String>,
    candidates: Vec<CandidateListsCandidate>,
    affiliation_type: AffiliationType,
    belongs_to_set: Option<NonZeroU64>,
) -> Result<CandidateListsAffiliation, AppError> {
    let mut builder = CandidateListsAffiliation::builder()
        .id(AffiliationId::new(position))
        .affiliation_type(affiliation_type)
        .publish_gender(true)
        .publication_language(PublicationLanguage::Dutch)
        .candidates(candidates);

    if let Some(appellation) = appellation {
        builder = builder.registered_name(appellation);
    }

    if let Some(belongs_to_set) = belongs_to_set {
        builder = builder.belongs_to_set(belongs_to_set);
    }

    Ok(builder.build()?)
}

fn candidate(position: usize, person: &Person) -> Result<CandidateListsCandidate, AppError> {
    let builder = CandidateListsCandidate::builder().identifier(candidate_identifier(position)?);
    Ok(public_candidate_details(builder, person)?.build()?)
}

/// The candidate's details as published in the 230b, which the 230c extends
pub(super) fn public_candidate_details(
    builder: CandidateListsCandidateBuilder,
    person: &Person,
) -> Result<CandidateListsCandidateBuilder, AppError> {
    let mut builder = builder
        .full_name(&person.name)
        .gender(&person.personal_data)
        .qualifying_address(QualifyingAddress::try_from(&person.personal_data)?);

    if let Some(date_of_birth) = person.personal_data.date_of_birth.as_ref() {
        builder = builder.date_of_birth(**date_of_birth);
    }

    Ok(builder)
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
        structs::{common::CountryCode, persons::PersonId, political_groups::PoliticalGroup},
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

    /// A list covering `districts`, for [`affiliation_type`] and
    /// [`shared_list_set_number`] tests; other fields don't matter for
    /// classification.
    fn sample_list_for(
        id: CandidateListId,
        districts: impl IntoIterator<Item = ElectoralDistrict>,
    ) -> CandidateList {
        CandidateList {
            id,
            electoral_districts: districts.into_iter().collect(),
            ..Default::default()
        }
    }

    /// A group with only one list, covering only the district it is being
    /// classified for: "op zichzelf staande lijst", e.g. a party in PS23
    /// Limburg that only ran in Maastricht, and nowhere else in the
    /// province.
    #[test]
    fn single_district_participation_is_standalone() {
        let list = sample_list_for(CandidateListId::new(), [ElectoralDistrict::PsMaastricht]);
        let valid = vec![(ElectoralDistrict::PsMaastricht, list.clone())];

        assert_eq!(
            affiliation_type(&valid, list.id),
            AffiliationType::StandAloneList
        );
        assert_eq!(shared_list_set_number(&valid, list.id), None);
    }

    /// A group with a single list declared identical across several
    /// districts: "stel gelijkluidende lijsten", e.g. every party in EK23,
    /// which all used one list for every kieskring.
    #[test]
    fn one_list_across_several_districts_is_a_set_of_equal_lists() {
        let list = sample_list_for(
            CandidateListId::new(),
            [ElectoralDistrict::PsMaastricht, ElectoralDistrict::PsVenlo],
        );
        let valid = vec![
            (ElectoralDistrict::PsMaastricht, list.clone()),
            (ElectoralDistrict::PsVenlo, list.clone()),
        ];

        assert_eq!(
            affiliation_type(&valid, list.id),
            AffiliationType::SetOfEqualLists
        );
        assert_eq!(shared_list_set_number(&valid, list.id), NonZeroU64::new(1));
    }

    /// A group running different lists in different districts is a
    /// "lijstengroep" for every one of its lists, e.g. most parties in PS23
    /// Limburg, which had separate lists for Maastricht and Venlo.
    #[test]
    fn different_lists_per_district_is_a_group_of_lists() {
        let list1 = sample_list_for(CandidateListId::new(), [ElectoralDistrict::PsMaastricht]);
        let list2 = sample_list_for(CandidateListId::new(), [ElectoralDistrict::PsVenlo]);
        let valid = vec![
            (ElectoralDistrict::PsMaastricht, list1.clone()),
            (ElectoralDistrict::PsVenlo, list2.clone()),
        ];

        assert_eq!(
            affiliation_type(&valid, list1.id),
            AffiliationType::GroupOfLists
        );
        assert_eq!(
            affiliation_type(&valid, list2.id),
            AffiliationType::GroupOfLists
        );
        assert_eq!(shared_list_set_number(&valid, list1.id), None);
        assert_eq!(shared_list_set_number(&valid, list2.id), None);
    }

    /// Within a "lijstengroep", a list can still be declared identical
    /// across a subset of its districts, and also gets a `BelongsToSet`,
    /// on its shared lists.
    #[test]
    fn shared_subset_within_a_group_of_lists_still_gets_a_set() {
        let shared_list = sample_list_for(
            CandidateListId::new(),
            [ElectoralDistrict::PsMaastricht, ElectoralDistrict::PsVenlo],
        );
        let solo_list = sample_list_for(CandidateListId::new(), [ElectoralDistrict::Limburg]);
        let valid = vec![
            (ElectoralDistrict::PsMaastricht, shared_list.clone()),
            (ElectoralDistrict::PsVenlo, shared_list.clone()),
            (ElectoralDistrict::Limburg, solo_list.clone()),
        ];

        assert_eq!(
            affiliation_type(&valid, shared_list.id),
            AffiliationType::GroupOfLists
        );
        assert_eq!(
            affiliation_type(&valid, solo_list.id),
            AffiliationType::GroupOfLists
        );
        assert_eq!(
            shared_list_set_number(&valid, shared_list.id),
            NonZeroU64::new(1)
        );
        assert_eq!(shared_list_set_number(&valid, solo_list.id), None);
    }

    /// A group that registers more than one shared-list group in the same
    /// election gets each numbered in turn, by creation order.
    #[test]
    fn numbers_several_shared_list_groups_in_creation_order() {
        // Created later, but listed first in `valid`, to prove ordering
        // follows `created_at` rather than list order.
        let created_second = CandidateList {
            created_at: chrono::DateTime::from_timestamp(1, 0).unwrap().into(),
            ..sample_list_for(
                CandidateListId::new(),
                [ElectoralDistrict::PsMaastricht, ElectoralDistrict::PsVenlo],
            )
        };
        let created_first = CandidateList {
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap().into(),
            ..sample_list_for(
                CandidateListId::new(),
                [ElectoralDistrict::Limburg, ElectoralDistrict::WsFryslan],
            )
        };
        let valid = vec![
            (ElectoralDistrict::PsMaastricht, created_second.clone()),
            (ElectoralDistrict::PsVenlo, created_second.clone()),
            (ElectoralDistrict::Limburg, created_first.clone()),
            (ElectoralDistrict::WsFryslan, created_first.clone()),
        ];

        assert_eq!(
            shared_list_set_number(&valid, created_first.id),
            NonZeroU64::new(1)
        );
        assert_eq!(
            shared_list_set_number(&valid, created_second.id),
            NonZeroU64::new(2)
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

        let store1 = CsbStream {
            election,
            ..CsbStream::new_for_test()
        };
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

        let store2 = CsbStream {
            election,
            ..CsbStream::new_for_test()
        };
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

        let store3 = CsbStream {
            election,
            ..CsbStream::new_for_test()
        };
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

    /// A contest with no established groups produces nothing.
    #[test]
    fn no_groups_produces_no_document() {
        let result = eml230b(&ElectionConfig::EK27, ContestIdentifier::geen(), None, &[]).unwrap();
        assert!(result.is_none());
    }
}
