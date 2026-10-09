use std::num::NonZeroU64;

use axum::{
    body::Body,
    extract::State,
    http::HeaderValue,
    response::{IntoResponse, Response},
};
use eks_utils::slugify_teletex;
use eml_nl::utils::ElectionId;
use tokio::io::{DuplexStream, duplex};
use tokio_util::io::ReaderStream;

use crate::{
    AppError, AppRequestState, CsbMainStore, CsbStoreData, CsbStream, ElectionConfig,
    ElectoralDistrict,
    core::ZipResponseWriter,
    csb::{
        examination::{extractors::CsbPoliticalGroup, numbering::ListNumbering},
        finalise::paths::CsbEmlZipDownloadPath,
    },
    models::{
        documents::ZIP_CONTENT_TYPE,
        eml::{
            eml230b::{contests, eml230b},
            eml230c::eml230c,
        },
    },
    store::StoreRegistry,
    utils::no_cache_headers,
};

/// The established candidate lists per electoral district ("kieskring"):
/// 1 EML 230b file per district plus 1 EML 230c file with all districts
pub async fn download_eml_zip<S: AppRequestState>(
    _: CsbEmlZipDownloadPath,
    main_store: CsbMainStore,
    State(state): State<S>,
) -> Result<Response, AppError> {
    let election = main_store.election;

    let files = eml230_files(state.csb_store_registry(), &main_store).await?;

    let filename = format!("eml-{}.zip", election.filename_slug());
    let headers = no_cache_headers::generate_attachment_headers(
        &filename,
        HeaderValue::from_static(ZIP_CONTENT_TYPE),
    )?;

    let (reader, writer) = duplex(64 * 1024);
    let body = Body::from_stream(ReaderStream::new(reader));

    tokio::spawn(async move {
        if let Err(err) = write_eml_zip(files, writer).await {
            tracing::error!(error = ?err, "failed to stream EML zip");
        }
    });

    Ok((headers, body).into_response())
}

// Temporary EML zip: eventually these should be combined with all the other exported documents
async fn write_eml_zip(
    files: Vec<(String, Vec<u8>)>,
    writer: DuplexStream,
) -> Result<(), AppError> {
    let mut zipper = ZipResponseWriter::new(writer);

    for (name, bytes) in files {
        zipper.add_file(&name, &bytes).await?;
    }

    zipper.finish().await
}

/// One EML 230b file per district, and the EML 230c for the whole election
///
/// Districts without established lists are left out.
async fn eml230_files(
    registry: &StoreRegistry<CsbStoreData>,
    main_store: &CsbMainStore,
) -> Result<Vec<(String, Vec<u8>)>, AppError> {
    let election = main_store.election;
    let streams = registry.stores_for_election(election).await?;
    let political_groups: Vec<CsbPoliticalGroup> = streams
        .iter()
        .map(CsbPoliticalGroup::new_from_csb_store)
        .collect();
    let numbering = ListNumbering::new(
        &political_groups,
        &main_store.registered_political_groups(),
        &main_store.list_order(),
    );

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

    let mut files = Vec::new();

    for (contest_identifier, district) in contests(&election)? {
        let Some(bytes) = eml230b(&election, contest_identifier, district, &numbered_groups)?
        else {
            continue;
        };

        files.push((eml230b_filename(&election, district)?, bytes));
    }

    if let Some(bytes) = eml230c(&election, &numbered_groups)? {
        files.push((eml230c_filename(&election)?, bytes));
    }

    Ok(files)
}

/// The file name for the EML 230b of `district`, e.g.
/// `Kandidatenlijsten_EK2027_Drenthe.eml.xml`, or without district when the
/// election has only one
fn eml230b_filename(
    election: &ElectionConfig,
    district: Option<ElectoralDistrict>,
) -> Result<String, AppError> {
    let election_id = ElectionId::try_from(*election)?;
    Ok(match district {
        Some(district) => format!(
            "Kandidatenlijsten_{}_{}.eml.xml",
            election_id.value(),
            slugify_teletex(district.title(), false)
        ),
        None => format!("Kandidatenlijsten_{}.eml.xml", election_id.value()),
    })
}

/// The file name for the EML 230c, e.g. `Totaallijsten_EK2027.eml.xml`
fn eml230c_filename(election: &ElectionConfig) -> Result<String, AppError> {
    Ok(format!(
        "Totaallijsten_{}.eml.xml",
        ElectionId::try_from(*election)?.value()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    use crate::{
        AppState, CsbAction, CsbMainAction, CsbUser, ElectionConfig, ElectoralDistrict,
        PgStoreData, Province, StreamId,
        structs::{
            candidate_lists::CandidateList, csb::sample_registered_political_group,
            list_designation::ListDesignation, persons::PersonId, political_groups::PoliticalGroup,
        },
        test_utils::sample_person,
    };

    /// Import a group with one candidate on a list in each of the districts
    async fn sample_group(
        state: &AppState,
        election: ElectionConfig,
        appellation: &str,
        districts: impl IntoIterator<Item = ElectoralDistrict>,
    ) -> StreamId {
        let stream_id = StreamId::new();
        let store = state
            .csb_store_for_stream(stream_id, election)
            .await
            .unwrap()
            .acting_as_test_user();

        let candidate = sample_person(PersonId::new());
        let mut snapshot = PgStoreData {
            political_group: PoliticalGroup {
                appellation: Some(appellation.parse().unwrap()),
                list_designation: Some(ListDesignation::Standalone),
                ..Default::default()
            },
            ..PgStoreData::default()
        };
        snapshot.persons.insert(candidate.id, candidate.clone());
        let list = CandidateList {
            electoral_districts: districts.into_iter().collect(),
            candidates: vec![candidate.id],
            ..Default::default()
        };
        snapshot.candidate_lists.insert(list.id, list);

        store
            .update(CsbAction::Import {
                hash: [0u8; 32],
                source_stream_id: StreamId::new(),
                snapshot: Box::new(snapshot),
            })
            .await
            .unwrap();

        stream_id
    }

    /// A test [`CsbMainStore`] scoped to `election`, since
    /// [`CsbMainStore::new_for_test`] always scopes to EK27.
    fn main_store_for(election: ElectionConfig) -> CsbMainStore {
        CsbMainStore {
            election,
            ..CsbMainStore::new_for_test()
        }
    }

    /// A blank list ("Blanco") has no `RegisteredName`, the way the
    /// Kiesraad's own 230b exports leave it empty, rather than printing
    /// "Blanco" or a name derived from the first candidate.
    #[tokio::test]
    async fn eml230_files_blank_list_has_no_registered_name() -> Result<(), AppError> {
        let election = ElectionConfig::PS27(Province::Groningen);
        assert!(election.has_only_one_district());

        let state = AppState::new_for_tests().await;
        let stream_id = StreamId::new();
        let store = state
            .csb_store_for_stream(stream_id, election)
            .await?
            .acting_as_test_user();

        let candidate = sample_person(PersonId::new());
        let mut snapshot = PgStoreData {
            political_group: PoliticalGroup {
                appellation: None,
                list_designation: Some(ListDesignation::Blank),
                ..Default::default()
            },
            ..PgStoreData::default()
        };
        snapshot.persons.insert(candidate.id, candidate.clone());
        let list = CandidateList {
            electoral_districts: election.electoral_districts().iter().copied().collect(),
            candidates: vec![candidate.id],
            ..Default::default()
        };
        snapshot.candidate_lists.insert(list.id, list);
        store
            .update(CsbAction::Import {
                hash: [0u8; 32],
                source_stream_id: StreamId::new(),
                snapshot: Box::new(snapshot),
            })
            .await?;

        let main_store = main_store_for(election);
        let files = eml230_files(state.csb_store_registry(), &main_store).await?;

        assert_eq!(files.len(), 2);
        for (_, bytes) in &files {
            let xml = String::from_utf8(bytes.clone()).unwrap();
            assert!(!xml.to_lowercase().contains("blanco"));
            assert!(xml.contains("<RegisteredName/>"));
        }

        Ok(())
    }

    /// A single-district election yields one 230b, with the `geen` contest.
    #[tokio::test]
    async fn eml230_files_single_district_yields_one_geen_file() -> Result<(), AppError> {
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

        let files = eml230_files(state.csb_store_registry(), &main_store).await?;

        assert_eq!(files.len(), 2);
        assert_eq!(files[0].0, "Kandidatenlijsten_PS2027_Groningen.eml.xml");
        assert_eq!(files[1].0, "Totaallijsten_PS2027_Groningen.eml.xml");
        let xml = String::from_utf8(files[0].1.clone()).unwrap();
        assert!(xml.contains(r#"<ContestIdentifier Id="geen"/>"#));
        assert!(xml.contains("Kiesraad Demo"));

        Ok(())
    }

    /// A multi-district election yields one file per district the group's
    /// list covers; each file's contest identifies that district only.
    #[tokio::test]
    async fn eml230_files_multi_district_yields_one_file_per_district() -> Result<(), AppError> {
        let election = ElectionConfig::PS27(Province::Limburg);
        let districts = election.electoral_districts();
        assert!(districts.len() > 1, "province needs multiple districts");

        let state = AppState::new_for_tests().await;
        sample_group(&state, election, "Alleen Eerste", [districts[0]]).await;
        let main_store = main_store_for(election);

        let files = eml230_files(state.csb_store_registry(), &main_store).await?;

        // Only the district the group's list covers gets a 230b file, next to the 230c
        assert_eq!(files.len(), 2);
        assert_eq!(files[1].0, "Totaallijsten_PS2027_Limburg.eml.xml");
        assert_eq!(
            files[0].0,
            "Kandidatenlijsten_PS2027_Limburg_Maastricht.eml.xml"
        );
        let xml = String::from_utf8(files[0].1.clone()).unwrap();
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
        let election = ElectionConfig::PS27(Province::Limburg);
        let districts = election.electoral_districts();

        let state = AppState::new_for_tests().await;
        sample_group(&state, election, "Overal", districts.iter().copied()).await;
        let main_store = main_store_for(election);

        let files = eml230_files(state.csb_store_registry(), &main_store).await?;

        assert_eq!(files.len(), districts.len() + 1);
        for (_, bytes) in &files {
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
        assert_eq!(
            eml230b_filename(
                &ElectionConfig::PS27(Province::Limburg),
                Some(ElectoralDistrict::PsMaastricht)
            )
            .unwrap(),
            "Kandidatenlijsten_PS2027_Limburg_Maastricht.eml.xml"
        );
    }

    #[test]
    fn filename_uses_election_id() {
        assert_eq!(
            eml230c_filename(&ElectionConfig::EK27).unwrap(),
            "Totaallijsten_EK2027.eml.xml"
        );
        assert_eq!(
            eml230c_filename(&ElectionConfig::PS27(Province::Groningen)).unwrap(),
            "Totaallijsten_PS2027_Groningen.eml.xml"
        );
    }

    #[tokio::test]
    async fn download_eml_zip_returns_zip_response() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        sample_group(
            &state,
            ElectionConfig::EK27,
            "Kiesraad Demo",
            ElectionConfig::EK27.electoral_districts().to_vec(),
        )
        .await;
        let main_store = CsbMainStore::new_for_test();

        let response = download_eml_zip(CsbEmlZipDownloadPath, main_store, State(state))
            .await?
            .into_response();

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let headers = response.headers();
        assert_eq!(
            headers
                .get(axum::http::header::CONTENT_TYPE)
                .expect("content type"),
            ZIP_CONTENT_TYPE,
        );
        assert_eq!(
            headers
                .get(axum::http::header::CONTENT_DISPOSITION)
                .expect("content disposition"),
            "attachment; filename=\"eml-ek27.zip\""
        );

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert!(body.starts_with(b"PK"), "body is not a ZIP archive");

        Ok(())
    }

    #[tokio::test]
    async fn download_eml_zip_skips_groups_without_valid_candidates() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let stream_id = StreamId::new();
        let store = state
            .csb_store_for_stream(stream_id, ElectionConfig::EK27)
            .await?
            .acting_as_test_user();
        let mut snapshot = PgStoreData {
            political_group: PoliticalGroup {
                appellation: Some("Leeg".parse().unwrap()),
                list_designation: Some(ListDesignation::Standalone),
                ..Default::default()
            },
            ..PgStoreData::default()
        };
        let list = CandidateList {
            electoral_districts: ElectionConfig::EK27.electoral_districts()[0..1]
                .iter()
                .copied()
                .collect(),
            ..Default::default()
        };
        snapshot.candidate_lists.insert(list.id, list);
        store
            .update(CsbAction::Import {
                hash: [0u8; 32],
                source_stream_id: StreamId::new(),
                snapshot: Box::new(snapshot),
            })
            .await?;

        let main_store = CsbMainStore::new_for_test();
        let files = eml230_files(state.csb_store_registry(), &main_store).await?;

        assert!(files.is_empty());

        Ok(())
    }

    /// The final list order (recorded, then on votes, then by lot) sets both
    /// each affiliation's own list number and the print order.
    #[tokio::test]
    async fn eml230_files_numbers_by_the_final_list_order() -> Result<(), AppError> {
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

        let files = eml230_files(state.csb_store_registry(), &main_store).await?;

        // The recorded order puts "Loting" first, so it gets list number 1
        // and prints first; "Gezeteld" is 2 and prints second.
        let xml = String::from_utf8(files[0].1.clone()).unwrap();
        let position = |needle: &str| xml.find(needle).expect("group in export");
        assert!(position(r#"<AffiliationIdentifier Id="1">"#) < position("Loting"));
        assert!(position("Loting") < position(r#"<AffiliationIdentifier Id="2">"#));
        assert!(position(r#"<AffiliationIdentifier Id="2">"#) < position("Gezeteld"));

        Ok(())
    }

    /// The print order follows the established list number, not the order
    /// the underlying lists were created in.
    #[tokio::test]
    async fn eml230_files_prints_by_established_list_number() -> Result<(), AppError> {
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
        let files = eml230_files(state.csb_store_registry(), &main_store).await?;

        let xml = String::from_utf8(files[0].1.clone()).unwrap();
        let position = |needle: &str| xml.find(needle).expect("group in export");
        assert!(position("Andere Partij") < position("Zebra Partij"));

        Ok(())
    }
}
