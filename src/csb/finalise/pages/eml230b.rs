use std::{num::NonZeroU64, sync::Arc};

use axum::{
    body::Body,
    extract::State,
    http::HeaderValue,
    response::{IntoResponse, Response},
};
use eks_utils::slugify_teletex;
use eml_nl::{common::ContestIdentifier, utils::ContestId};
use tokio::io::{DuplexStream, duplex};
use tokio_util::io::ReaderStream;

use crate::{
    AppError, AppRequestState, CsbMainStore, CsbStoreData, StreamId,
    core::ZipResponseWriter,
    csb::{
        examination::{extractors::CsbPoliticalGroup, numbering::ListNumbering},
        finalise::paths::CsbEml230bDownloadPath,
    },
    models::{documents::ZIP_CONTENT_TYPE, eml::eml230b::eml230b},
    store::StoreRegistry,
    utils::no_cache_headers,
};

/// The established candidate lists per electoral district ("kieskring"):
/// 1 file per district, or a single file when the election does not have multiple districts
pub async fn download_eml230b<S: AppRequestState>(
    _: CsbEml230bDownloadPath,
    main_store: CsbMainStore,
    State(state): State<S>,
) -> Result<Response, AppError> {
    let election = main_store.election;

    let files = eml230b_files(state.csb_store_registry(), &main_store).await?;

    let filename = format!("eml230b-{}.zip", election.filename_slug());
    let headers = no_cache_headers::generate_attachment_headers(
        &filename,
        HeaderValue::from_static(ZIP_CONTENT_TYPE),
    )?;

    let (reader, writer) = duplex(64 * 1024);
    let body = Body::from_stream(ReaderStream::new(reader));

    tokio::spawn(async move {
        if let Err(err) = write_eml230b_zip(files, writer).await {
            tracing::error!(error = ?err, "failed to stream eml230b zip");
        }
    });

    Ok((headers, body).into_response())
}

// Temporary EML230b zip: eventually these should be combined with all the other exported documents
async fn write_eml230b_zip(
    files: Vec<(String, Vec<u8>)>,
    writer: DuplexStream,
) -> Result<(), AppError> {
    let mut zipper = ZipResponseWriter::new(writer);

    for (name, bytes) in files {
        zipper.add_file(&name, &bytes).await?;
    }

    zipper.finish().await
}

/// One EML 230b file per district
///
/// Districts that without an established lists are left out.
async fn eml230b_files(
    registry: &StoreRegistry<CsbStoreData>,
    main_store: &CsbMainStore,
) -> Result<Vec<(String, Vec<u8>)>, AppError> {
    let election = main_store.election;
    let streams = registry.stores_for_election(election).await?;
    let snapshots: Vec<(StreamId, Arc<CsbStoreData>)> = streams
        .iter()
        .map(|store| (store.stream_id, store.snapshot()))
        .collect();
    let political_groups: Vec<CsbPoliticalGroup> = streams
        .iter()
        .zip(&snapshots)
        .map(|(store, (_, data))| CsbPoliticalGroup::from_snapshot(store, data))
        .collect();
    let main = main_store.snapshot();
    let registered: Vec<_> = main
        .registered_political_groups()
        .into_iter()
        .cloned()
        .collect();
    let numbering = ListNumbering::new(&political_groups, &registered, main.list_order());

    // Each group's established, final list number, paired with its snapshot
    let numbered_groups: Vec<(NonZeroU64, &CsbStoreData)> = numbering
        .groups
        .iter()
        .enumerate()
        .filter_map(|(index, group)| {
            let (_, data) = snapshots.iter().find(|(id, _)| *id == group.stream_id)?;
            let position = NonZeroU64::new((index + 1) as u64).expect("index + 1 is non-zero");
            Some((position, data.as_ref()))
        })
        .collect();

    let mut files = Vec::new();

    if election.has_only_one_district() {
        if let Some(bytes) = eml230b(&election, ContestIdentifier::geen(), None, &numbered_groups)?
        {
            files.push(("eml230b.eml.xml".to_string(), bytes));
        }
    } else {
        for district in election.electoral_districts() {
            let contest_identifier =
                ContestIdentifier::new(ContestId::new(district.region_number().to_string())?)
                    .with_name(district.title());
            let Some(bytes) = eml230b(
                &election,
                contest_identifier,
                Some(*district),
                &numbered_groups,
            )?
            else {
                continue;
            };

            let filename = format!(
                "eml230b-{}.eml.xml",
                slugify_teletex(district.title(), true)
            );
            files.push((filename, bytes));
        }
    }

    Ok(files)
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
        CsbMainStore::new_for_test_with_election(election)
    }

    /// A blank list ("Blanco") has no `RegisteredName`, the way the
    /// Kiesraad's own 230b exports leave it empty, rather than printing
    /// "Blanco" or a name derived from the first candidate.
    #[tokio::test]
    async fn eml230b_files_blank_list_has_no_registered_name() -> Result<(), AppError> {
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
        let files = eml230b_files(state.csb_store_registry(), &main_store).await?;

        assert_eq!(files.len(), 1);
        let xml = String::from_utf8(files[0].1.clone()).unwrap();
        assert!(!xml.to_lowercase().contains("blanco"));
        assert!(xml.contains("<RegisteredName/>"));

        Ok(())
    }

    /// A single-district election yields one file, with the `geen` contest.
    #[tokio::test]
    async fn eml230b_files_single_district_yields_one_geen_file() -> Result<(), AppError> {
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

        let files = eml230b_files(state.csb_store_registry(), &main_store).await?;

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, "eml230b.eml.xml");
        let xml = String::from_utf8(files[0].1.clone()).unwrap();
        assert!(xml.contains(r#"<ContestIdentifier Id="geen"/>"#));
        assert!(xml.contains("Kiesraad Demo"));

        Ok(())
    }

    /// A multi-district election yields one file per district the group's
    /// list covers; each file's contest identifies that district only.
    #[tokio::test]
    async fn eml230b_files_multi_district_yields_one_file_per_district() -> Result<(), AppError> {
        let election = ElectionConfig::PS27(Province::Limburg);
        let districts = election.electoral_districts();
        assert!(districts.len() > 1, "province needs multiple districts");

        let state = AppState::new_for_tests().await;
        sample_group(&state, election, "Alleen Eerste", [districts[0]]).await;
        let main_store = main_store_for(election);

        let files = eml230b_files(state.csb_store_registry(), &main_store).await?;

        // Only the district the group's list covers gets a file.
        assert_eq!(files.len(), 1);
        assert_eq!(
            files[0].0,
            format!(
                "eml230b-{}.eml.xml",
                slugify_teletex(districts[0].title(), true)
            )
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
    async fn eml230b_files_group_in_every_district_appears_in_every_file() -> Result<(), AppError> {
        let election = ElectionConfig::PS27(Province::Limburg);
        let districts = election.electoral_districts();

        let state = AppState::new_for_tests().await;
        sample_group(&state, election, "Overal", districts.iter().copied()).await;
        let main_store = main_store_for(election);

        let files = eml230b_files(state.csb_store_registry(), &main_store).await?;

        assert_eq!(files.len(), districts.len());
        for (_, bytes) in &files {
            let xml = String::from_utf8(bytes.clone()).unwrap();
            assert!(xml.contains(r#"Id="1""#));
            assert!(xml.contains("Overal"));
        }

        Ok(())
    }

    #[tokio::test]
    async fn download_eml230b_returns_zip_response() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        sample_group(
            &state,
            ElectionConfig::EK27,
            "Kiesraad Demo",
            ElectionConfig::EK27.electoral_districts().to_vec(),
        )
        .await;
        let main_store = CsbMainStore::new_for_test();

        let response = download_eml230b(CsbEml230bDownloadPath, main_store, State(state))
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
            "attachment; filename=\"eml230b-ek27.zip\""
        );

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert!(body.starts_with(b"PK"), "body is not a ZIP archive");

        Ok(())
    }

    #[tokio::test]
    async fn download_eml230b_skips_groups_without_valid_candidates() -> Result<(), AppError> {
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
        let files = eml230b_files(state.csb_store_registry(), &main_store).await?;

        assert!(files.is_empty());

        Ok(())
    }

    /// The final list order (recorded, then on votes, then by lot) sets both
    /// each affiliation's own list number and the print order.
    #[tokio::test]
    async fn eml230b_files_numbers_by_the_final_list_order() -> Result<(), AppError> {
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

        let files = eml230b_files(state.csb_store_registry(), &main_store).await?;

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
    async fn eml230b_files_prints_by_established_list_number() -> Result<(), AppError> {
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
        let files = eml230b_files(state.csb_store_registry(), &main_store).await?;

        let xml = String::from_utf8(files[0].1.clone()).unwrap();
        let position = |needle: &str| xml.find(needle).expect("group in export");
        assert!(position("Andere Partij") < position("Zebra Partij"));

        Ok(())
    }
}
