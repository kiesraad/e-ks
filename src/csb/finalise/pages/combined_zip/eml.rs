use std::num::NonZeroU64;

use eks_utils::slugify_teletex;
use eml_nl::utils::ElectionId;
use sha2::{Digest, Sha256};

use crate::{
    AppError, CsbContext, CsbMainStore, CsbStoreData, CsbStream, ElectionConfig, ElectoralDistrict,
    csb::{
        examination::extractors::CsbPoliticalGroup,
        finalise::{numbering::ListNumbering, pages::combined_zip::Files},
    },
    models::eml::{
        eml110a::eml110a,
        eml230b::{contests, eml230b},
        eml230c::eml230c,
    },
    store::StoreRegistry,
    utils::format_hash,
};

/// One EML 230b file per district, and the EML 230c for the whole election,
/// in that order
///
/// Districts without established lists get no 230b file.
pub(super) async fn eml230_files(
    context: &CsbContext,
    registry: &StoreRegistry<CsbStoreData>,
    main_store: &CsbMainStore,
) -> Result<(Files, Files), AppError> {
    let election = main_store.election;
    let streams = registry.stores_for_election(election).await?;
    let political_groups: Vec<CsbPoliticalGroup> = streams
        .iter()
        .map(CsbPoliticalGroup::new_from_csb_store)
        .collect();
    let mut numbering = ListNumbering::new(
        &political_groups,
        &main_store.registered_political_groups(),
        &main_store.list_order(),
    );

    numbering.record_if_incomplete(context, main_store).await?;

    // Each group's established, final list number, paired with its store
    let numbered_groups: Vec<(NonZeroU64, &CsbStream)> = numbering
        .groups
        .iter()
        .enumerate()
        .filter_map(|(index, group)| {
            let store = streams
                .iter()
                .find(|store| store.stream_id == group.stream_id)?;
            let position = NonZeroU64::new((index + 1) as u64).expect("index + 1 is non-zero");
            Some((position, store))
        })
        .collect();

    let mut eml230bs = Files::new();

    for (contest_identifier, district) in contests(&election)? {
        if let Some(bytes) = eml230b(&election, contest_identifier, district, &numbered_groups)? {
            eml230bs.push((eml230b_filename(&election, district)?, bytes));
        }
    }

    let election_id = ElectionId::try_from(election)?;

    Ok((
        eml230bs,
        eml230c(&election, &numbered_groups)?
            .map(|eml| {
                vec![(
                    format!("Totaallijsten_{}.eml.xml", election_id.value()),
                    eml,
                )]
            })
            .unwrap_or_default(),
    ))
}

/// The file name for the EML 230b of `district`, e.g.
/// `Kandidatenlijsten_EK2027_Drenthe.eml.xml`
pub(super) fn eml230b_filename(
    election: &ElectionConfig,
    district: Option<ElectoralDistrict>,
) -> Result<String, AppError> {
    Ok(format!(
        "{}.eml.xml",
        candidate_lists_stem(election, district)?
    ))
}

/// The file name without extension shared by the EML 230b and the candidate
/// list document of `district`, e.g. `Kandidatenlijsten_EK2027_Drenthe`, or
/// without district when the election has only one
pub(super) fn candidate_lists_stem(
    election: &ElectionConfig,
    district: Option<ElectoralDistrict>,
) -> Result<String, AppError> {
    let election_id = ElectionId::try_from(*election)?;
    Ok(
        match district.filter(|_| !election.has_only_one_district()) {
            Some(district) => format!(
                "Kandidatenlijsten_{}_{}",
                election_id.value(),
                slugify_teletex(district.title(), false)
            ),
            None => format!("Kandidatenlijsten_{}", election_id.value()),
        },
    )
}

/// The EML 110a with its hashcode file
pub(super) fn eml110a_files(main_store: &CsbMainStore) -> Result<Files, AppError> {
    let election_id = ElectionId::try_from(main_store.election)?;

    let eml110a = eml110a(
        &main_store.election,
        &main_store.registered_political_groups(),
    )?;
    let hash = eml_hash(&eml110a);

    let filename = format!("Verkiezingsdefinitie_{}.eml.xml", election_id.value());

    Ok(vec![
        (filename.clone(), eml110a),
        (format!("{filename}.hashcode.txt"), hash.into_bytes()),
    ])
}

/// The SHA-256 of an EML as OSV asks for it on import, e.g. `8097 F58F ...`
pub(super) fn eml_hash(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    format_hash(&hash, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{
        AppRequestState, AppState, CsbMainAction, CsbUser, ElectionConfig, ElectoralDistrict,
        Province, StreamId,
        structs::{
            candidate_lists::CandidateList, csb::sample_registered_political_group,
            list_designation::ListDesignation, persons::PersonId, political_groups::PoliticalGroup,
        },
        test_utils::{sample_person, sample_political_group_named, seed_csb_group},
    };

    /// Import a group with one candidate on a list in each of the districts
    async fn sample_group(
        state: &AppState,
        election: ElectionConfig,
        appellation: &str,
        districts: impl IntoIterator<Item = ElectoralDistrict>,
    ) -> StreamId {
        let candidate = sample_person(PersonId::new());
        let list = CandidateList {
            electoral_districts: districts.into_iter().collect(),
            candidates: vec![candidate.id],
            ..Default::default()
        };
        seed_csb_group(
            state,
            election,
            sample_political_group_named(appellation),
            vec![candidate],
            vec![list],
        )
        .await
        .stream_id
    }

    /// A test [`CsbMainStore`] scoped to `election`, since
    /// [`CsbMainStore::new_for_test`] always scopes to EK27.
    fn main_store_for(election: ElectionConfig) -> CsbMainStore {
        CsbMainStore::new_for_test_with_election(election)
    }

    /// A blank list ("Blanco") has no `RegisteredName`, the way the
    /// Kiesraad's own 230b exports leave it empty, rather than printing
    /// "Blanco" or a name derived from the first candidate.
    #[tokio::test]
    async fn eml230_files_blank_list_has_no_registered_name() -> Result<(), AppError> {
        let election = ElectionConfig::PS27(Province::Groningen);
        assert!(election.has_only_one_district());
        let context = CsbContext::new_test();

        let state = AppState::new_for_tests().await;
        let candidate = sample_person(PersonId::new());
        let list = CandidateList {
            electoral_districts: election.electoral_districts().iter().copied().collect(),
            candidates: vec![candidate.id],
            ..Default::default()
        };
        seed_csb_group(
            &state,
            election,
            PoliticalGroup {
                appellation: None,
                list_designation: Some(ListDesignation::Blank),
                ..Default::default()
            },
            vec![candidate],
            vec![list],
        )
        .await;

        let main_store = main_store_for(election);
        let (eml230bs, eml230c) =
            eml230_files(&context, state.csb_store_registry(), &main_store).await?;

        assert_eq!(eml230bs.len(), 1);
        assert_eq!(eml230c.len(), 1);
        for (_, bytes) in eml230bs.iter().chain(&eml230c) {
            let xml = String::from_utf8(bytes.clone()).unwrap();
            assert!(!xml.to_lowercase().contains("blanco"));
            assert!(xml.contains("<RegisteredName/>"));
        }

        Ok(())
    }

    /// A single-district election yields one 230b, with the `geen` contest.
    #[tokio::test]
    async fn eml230_files_single_district_yields_one_geen_file() -> Result<(), AppError> {
        let context = CsbContext::new_test();
        let election = ElectionConfig::PS27(Province::Groningen);
        assert!(election.has_only_one_district());

        let state = AppState::new_for_tests().await;
        sample_group(
            &state,
            election,
            "Kiesraad Demo",
            election.electoral_districts().to_vec(),
        )
        .await;
        let main_store = main_store_for(election);

        let (eml230bs, eml230c) =
            eml230_files(&context, state.csb_store_registry(), &main_store).await?;

        assert_eq!(eml230bs.len(), 1);
        assert_eq!(eml230bs[0].0, "Kandidatenlijsten_PS2027_Groningen.eml.xml");
        assert_eq!(eml230c[0].0, "Totaallijsten_PS2027_Groningen.eml.xml");
        let xml = String::from_utf8(eml230bs[0].1.clone()).unwrap();
        assert!(xml.contains(r#"<ContestIdentifier Id="geen"/>"#));
        assert!(xml.contains("Kiesraad Demo"));

        Ok(())
    }

    /// A multi-district election yields one file per district the group's
    /// list covers; each file's contest identifies that district only.
    #[tokio::test]
    async fn eml230_files_multi_district_yields_one_file_per_district() -> Result<(), AppError> {
        let context = CsbContext::new_test();
        let election = ElectionConfig::PS27(Province::Limburg);
        let districts = election.electoral_districts();
        assert!(districts.len() > 1, "province needs multiple districts");

        let state = AppState::new_for_tests().await;
        sample_group(&state, election, "Alleen Eerste", [districts[0]]).await;
        let main_store = main_store_for(election);

        let (eml230bs, eml230c) =
            eml230_files(&context, state.csb_store_registry(), &main_store).await?;

        // Only the district the group's list covers gets a 230b file
        assert_eq!(eml230c[0].0, "Totaallijsten_PS2027_Limburg.eml.xml");
        assert_eq!(eml230bs.len(), 1);
        assert_eq!(
            eml230bs[0].0,
            "Kandidatenlijsten_PS2027_Limburg_Maastricht.eml.xml"
        );
        let xml = String::from_utf8(eml230bs[0].1.clone()).unwrap();
        assert!(xml.contains(&format!(
            r#"<ContestIdentifier Id="{}">"#,
            districts[0].region_number()
        )));
        assert!(xml.contains(&format!(
            "<ContestName>{}</ContestName>",
            districts[0].title()
        )));

        Ok(())
    }

    /// A group with lists in every district of a province appears in every
    /// district's file, under the same list number in each.
    #[tokio::test]
    async fn eml230_files_group_in_every_district_appears_in_every_file() -> Result<(), AppError> {
        let context = CsbContext::new_test();
        let election = ElectionConfig::PS27(Province::Limburg);
        let districts = election.electoral_districts();

        let state = AppState::new_for_tests().await;
        sample_group(&state, election, "Overal", districts.iter().copied()).await;
        let main_store = main_store_for(election);

        let (eml230bs, eml230c) =
            eml230_files(&context, state.csb_store_registry(), &main_store).await?;

        // One 230b per district, plus the 230c for the whole election
        assert_eq!(eml230bs.len(), districts.len());
        for (_, bytes) in eml230bs.iter().chain(&eml230c) {
            let xml = String::from_utf8(bytes.clone()).unwrap();
            assert!(xml.contains(r#"Id="1""#));
            assert!(xml.contains("Overal"));
        }

        Ok(())
    }

    #[test]
    fn filename_uses_election_id_and_district() {
        assert_eq!(
            eml230b_filename(&ElectionConfig::EK27, Some(ElectoralDistrict::Drenthe)).unwrap(),
            "Kandidatenlijsten_EK2027_Drenthe.eml.xml"
        );
        assert_eq!(
            eml230b_filename(&ElectionConfig::EK27, Some(ElectoralDistrict::Fryslan)).unwrap(),
            "Kandidatenlijsten_EK2027_Fryslan.eml.xml"
        );
        assert_eq!(
            eml230b_filename(&ElectionConfig::PS27(Province::Groningen), None).unwrap(),
            "Kandidatenlijsten_PS2027_Groningen.eml.xml"
        );
        // The only district of an election is left out
        assert_eq!(
            eml230b_filename(
                &ElectionConfig::PS27(Province::Groningen),
                Some(ElectoralDistrict::PsGroningen)
            )
            .unwrap(),
            "Kandidatenlijsten_PS2027_Groningen.eml.xml"
        );
        assert_eq!(
            eml230b_filename(
                &ElectionConfig::PS27(Province::Limburg),
                Some(ElectoralDistrict::PsMaastricht)
            )
            .unwrap(),
            "Kandidatenlijsten_PS2027_Limburg_Maastricht.eml.xml"
        );
    }

    /// A group without valid candidates gets no 230b file; the 230c is still
    /// there.
    #[tokio::test]
    async fn eml230_files_skips_groups_without_valid_candidates() -> Result<(), AppError> {
        let context = CsbContext::new_test();
        let state = AppState::new_for_tests().await;
        let list = CandidateList {
            electoral_districts: ElectionConfig::EK27.electoral_districts()[0..1]
                .iter()
                .copied()
                .collect(),
            ..Default::default()
        };
        seed_csb_group(
            &state,
            ElectionConfig::EK27,
            sample_political_group_named("Leeg"),
            vec![],
            vec![list],
        )
        .await;

        let main_store = CsbMainStore::new_for_test();
        let (eml230bs, eml230c) =
            eml230_files(&context, state.csb_store_registry(), &main_store).await?;

        assert!(eml230bs.is_empty());
        assert!(eml230c.is_empty());

        Ok(())
    }

    /// The final list order (recorded, then on votes, then by lot) sets both
    /// each affiliation's own list number and the print order.
    #[tokio::test]
    async fn eml230_files_numbers_by_the_final_list_order() -> Result<(), AppError> {
        let context = CsbContext::new_test();
        let state = AppState::new_for_tests().await;
        let all_districts = ElectionConfig::EK27.electoral_districts().to_vec();
        let seated = sample_group(
            &state,
            ElectionConfig::EK27,
            "Gezeteld",
            all_districts.clone(),
        )
        .await;
        let by_lot = sample_group(&state, ElectionConfig::EK27, "Loting", all_districts).await;

        let main_store = CsbMainStore::new_for_test();
        main_store
            .update(
                CsbMainAction::CreateRegisteredPoliticalGroup(sample_registered_political_group(
                    "Gezeteld", 1000, 2,
                ))
                .by(CsbUser::new_test()),
            )
            .await?;
        main_store
            .update(CsbMainAction::UpdateListOrder(vec![by_lot, seated]).by(CsbUser::new_test()))
            .await?;

        let (eml230bs, _) = eml230_files(&context, state.csb_store_registry(), &main_store).await?;

        // The recorded order puts "Loting" first, so it gets list number 1
        // and prints first; "Gezeteld" is 2 and prints second.
        let xml = String::from_utf8(eml230bs[0].1.clone()).unwrap();
        let position = |needle: &str| xml.find(needle).expect("group in export");
        assert!(position(r#"<AffiliationIdentifier Id="1">"#) < position("Loting"));
        assert!(position("Loting") < position(r#"<AffiliationIdentifier Id="2">"#));
        assert!(position(r#"<AffiliationIdentifier Id="2">"#) < position("Gezeteld"));

        Ok(())
    }

    /// The first download records the starting order when none was recorded
    /// yet, and numbers the lists by it; further downloads leave it alone.
    #[tokio::test]
    async fn eml230_files_records_the_list_order_once() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let all_districts = ElectionConfig::EK27.electoral_districts().to_vec();
        // Created in reverse, so the alphabetical starting order differs
        let second = sample_group(
            &state,
            ElectionConfig::EK27,
            "Zebra Partij",
            all_districts.clone(),
        )
        .await;
        let first =
            sample_group(&state, ElectionConfig::EK27, "Andere Partij", all_districts).await;
        let main_store = CsbMainStore::new_for_test();
        assert_eq!(main_store.list_order_events(), 0);

        let assert_numbered = |(eml230bs, _): (Files, Files)| {
            let xml = String::from_utf8(eml230bs[0].1.clone()).unwrap();
            let position = |needle: &str| xml.find(needle).expect("group in export");
            assert!(position(r#"<AffiliationIdentifier Id="1">"#) < position("Andere Partij"));
            assert!(position("Andere Partij") < position(r#"<AffiliationIdentifier Id="2">"#));
            assert!(position(r#"<AffiliationIdentifier Id="2">"#) < position("Zebra Partij"));
        };

        assert_numbered(
            eml230_files(
                &CsbContext::new_test(),
                state.csb_store_registry(),
                &main_store,
            )
            .await?,
        );
        assert_eq!(main_store.list_order_events(), 1);
        assert_eq!(main_store.list_order(), [first, second]);

        assert_numbered(
            eml230_files(
                &CsbContext::new_test(),
                state.csb_store_registry(),
                &main_store,
            )
            .await?,
        );
        assert_eq!(main_store.list_order_events(), 1);

        Ok(())
    }

    /// The print order follows the established list number, not the order
    /// the underlying lists were created in.
    #[tokio::test]
    async fn eml230_files_prints_by_established_list_number() -> Result<(), AppError> {
        let context = CsbContext::new_test();
        let state = AppState::new_for_tests().await;
        let all_districts = ElectionConfig::EK27.electoral_districts().to_vec();
        // Created first, but alphabetically (and so numbered) last.
        sample_group(
            &state,
            ElectionConfig::EK27,
            "Zebra Partij",
            all_districts.clone(),
        )
        .await;
        // Created second, but alphabetically (and so numbered) first.
        sample_group(&state, ElectionConfig::EK27, "Andere Partij", all_districts).await;

        let main_store = CsbMainStore::new_for_test();
        let (eml230bs, _) = eml230_files(&context, state.csb_store_registry(), &main_store).await?;

        let xml = String::from_utf8(eml230bs[0].1.clone()).unwrap();
        let position = |needle: &str| xml.find(needle).expect("group in export");
        assert!(position("Andere Partij") < position("Zebra Partij"));

        Ok(())
    }
}
