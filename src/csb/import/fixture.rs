use uuid::Uuid;

use crate::{
    AppError, AppRequestState, CsbAction, CsbStore, CsbStream, CsbUser, ElectionConfig,
    ElectoralDistrict, PgStoreData, StreamId,
    projection::WithCorrections,
    structs::{
        candidate_lists::{CandidateList, CandidateListId},
        csb::{Omission, OmissionCategory, OmissionType},
        persons::PersonId,
    },
};

/// Marks CSB imports that were created from fixtures
pub const FIXTURE_IMPORT_HASH: [u8; 32] = [0; 32];

/// Create a political group stream with fixtures and import it as a CSB stream
/// if it doesn't exist already
pub async fn import_csb_fixture<S: AppRequestState>(
    state: &S,
    election: ElectionConfig,
    user: CsbUser,
) -> Result<(), AppError> {
    // Skip if a fixture import already exists for this election.
    for store in state
        .csb_store_registry()
        .stores_for_election(election)
        .await?
    {
        let comes_from_fixtures = store.data.read().events.first().is_some_and(
            |e| matches!(&e.payload.action, CsbAction::Import { hash, .. } if *hash == FIXTURE_IMPORT_HASH),
        );
        if comes_from_fixtures {
            return Ok(());
        }
    }

    let pg_stream_id = StreamId::new();
    let app_store = state.store_for_stream(pg_stream_id, election, true).await?;
    let events = app_store.data.read().events.clone();
    let snapshot = PgStoreData::snapshot_until(&events, usize::MAX);

    let store = CsbStore::acting_as(
        state
            .csb_store_for_stream(StreamId::new(), election)
            .await?,
        user,
    );
    store
        .update(CsbAction::Import {
            hash: FIXTURE_IMPORT_HASH,
            source_stream_id: pg_stream_id,
            snapshot: Box::new(snapshot),
        })
        .await?;

    for omission in fixture_omissions(&store) {
        omission.create(&store).await?;
    }

    Ok(())
}

/// Omissions spread over every category, so the recovery phase has something
/// to assess: an irreparable one, ones decided as a whole and ones decided part
/// by part (per district, per list), and a candidate with more than one.
fn fixture_omissions(store: &CsbStream) -> Vec<Omission> {
    let lists = fixture_lists(store);
    let Some(first_list) = lists.first() else {
        return Vec::new();
    };

    let mut omissions = vec![
        preset_omission(
            b"fixture_omission_unregistered_appellation",
            OmissionType::Appellation,
            "De aanduiding is niet geregistreerd",
            OmissionCategory::Appellation,
            &[],
        ),
        preset_omission(
            b"fixture_omission_authorisation_missing",
            OmissionType::CandidateList,
            "De machtiging aanduiding ontbreekt",
            OmissionCategory::CandidateList(vec![first_list.id]),
            &[],
        ),
        preset_omission(
            b"fixture_omission_agent_unregistered",
            OmissionType::CandidateList,
            "De gemachtigde is niet geregistreerd",
            OmissionCategory::CandidateList(lists.iter().map(|list| list.id).collect()),
            &[],
        ),
    ];
    omissions.extend(declarations_of_support_omissions(&lists));
    omissions.extend(candidate_omissions(store, &lists));
    omissions
}

/// The candidate lists in district order, so the first list is the one
/// covering the first district.
fn fixture_lists(store: &CsbStream) -> Vec<CandidateList> {
    let districts = store.election.electoral_districts();
    let mut lists = store.get_candidate_lists(WithCorrections::Paper);
    lists.sort_by_key(|list| {
        list.electoral_districts
            .first()
            .and_then(|district| districts.iter().position(|d| d == district))
    });
    lists
}

/// Missing declarations of support for the last list's districts
fn declarations_of_support_omissions(lists: &[CandidateList]) -> Vec<Omission> {
    let Some(last_list) = lists.last() else {
        return Vec::new();
    };

    vec![declarations_of_support_omission(
        b"fixture_omission_declarations_other_districts",
        &last_list.electoral_districts,
    )]
}

/// Missing declarations of support for `districts`, worded for one or for
/// several of them.
fn declarations_of_support_omission(key: &[u8], districts: &[ElectoralDistrict]) -> Omission {
    preset_omission(
        key,
        OmissionType::DeclarationsOfSupport,
        "Voor meerdere kieskringen ontbreken ondersteuningsverklaringen",
        OmissionCategory::DeclarationsOfSupport(districts.to_vec()),
        &[("{districts}", district_names(districts))],
    )
}

/// One omission for a candidate on the first list only, and two for a candidate
/// on several lists: one decided per list, one on the first list only.
fn candidate_omissions(store: &CsbStream, lists: &[CandidateList]) -> Vec<Omission> {
    let Some(first_list) = lists.first() else {
        return Vec::new();
    };
    let lists_of = |person: PersonId| -> Vec<CandidateListId> {
        lists
            .iter()
            .filter(|list| list.candidates.contains(&person))
            .map(|list| list.id)
            .collect()
    };
    let candidate_where = |wanted: fn(usize) -> bool| {
        first_list
            .candidates
            .iter()
            .copied()
            .find(|person| wanted(lists_of(*person).len()))
    };
    let omission = |key: &[u8], title: &str, person: PersonId, lists: Vec<CandidateListId>| {
        preset_omission(
            key,
            OmissionType::Candidate,
            title,
            OmissionCategory::Candidate { person, lists },
            &candidate_tokens(store, first_list, person),
        )
    };

    let mut omissions = Vec::new();
    if let Some(person) = candidate_where(|list_count| list_count == 1) {
        omissions.push(omission(
            b"fixture_omission_copy_id_missing",
            "Kopie ID ontbreekt",
            person,
            vec![first_list.id],
        ));
    }
    if let Some(person) =
        candidate_where(|list_count| list_count > 1).or(first_list.candidates.first().copied())
    {
        omissions.push(omission(
            b"fixture_omission_signature_missing",
            "Handtekening ontbreekt",
            person,
            lists_of(person),
        ));
        omissions.push(omission(
            b"fixture_omission_signing_date_missing",
            "Datum ondertekening ontbreekt",
            person,
            vec![first_list.id],
        ));
    }
    omissions
}

/// The values for the `{candidate_number}` and `{candidate_name}` tokens of
/// `person` on `list`.
fn candidate_tokens(
    store: &CsbStream,
    list: &CandidateList,
    person: PersonId,
) -> [(&'static str, String); 2] {
    [
        (
            "{candidate_number}",
            list.position_of(person)
                .map(|nr| nr.to_string())
                .unwrap_or_default(),
        ),
        (
            "{candidate_name}",
            store
                .get_person(person, WithCorrections::Paper)
                .map(|person| person.name.display())
                .unwrap_or_default(),
        ),
    ]
}

/// The preset omission titled `title` under a stable id derived from `key`,
/// with its `{token}` placeholders filled in as the add-omission dialog would.
fn preset_omission(
    key: &[u8],
    omission_type: OmissionType,
    title: &str,
    category: OmissionCategory,
    tokens: &[(&str, String)],
) -> Omission {
    let preset = omission_type
        .presets()
        .iter()
        .find(|preset| preset.title == title)
        .unwrap_or_else(|| panic!("no {omission_type} preset titled {title:?}"));
    let fill = |template: &str| {
        tokens
            .iter()
            .fold(template.to_string(), |text, (token, value)| {
                text.replace(token, value)
            })
    };

    let mut omission = Omission::new(
        category,
        preset.title.parse().expect("preset title"),
        fill(&preset.description)
            .parse()
            .expect("preset description"),
        (!preset.help_text.is_empty())
            .then(|| fill(&preset.help_text).parse().expect("preset help text")),
    );
    omission.id = Uuid::new_v5(&Uuid::NAMESPACE_OID, key).into();
    omission.recoverable = preset.recoverable;
    omission
}

/// District titles listed the way the add-omission dialog does: "A, B en C".
fn district_names(districts: &[ElectoralDistrict]) -> String {
    let titles: Vec<&str> = districts.iter().map(ElectoralDistrict::title).collect();
    match titles.split_last() {
        Some((last, [])) => last.to_string(),
        Some((last, rest)) => format!("{} en {last}", rest.join(", ")),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppState, core::election::WaterCouncil};

    async fn fixture_store(election: ElectionConfig) -> CsbStream {
        let state = AppState::new_for_tests().await;
        import_csb_fixture(&state, election, CsbUser::new_test())
            .await
            .unwrap();

        let mut csb_stores = state
            .csb_store_registry
            .stores_by_scope()
            .await
            .expect("csb stores");
        assert_eq!(csb_stores.len(), 1);
        csb_stores.remove(0)
    }

    #[tokio::test]
    async fn repeated_import_is_a_no_op() {
        let state = AppState::new_for_tests().await;
        let election = ElectionConfig::EK27;

        import_csb_fixture(&state, election, CsbUser::new_test())
            .await
            .unwrap();
        import_csb_fixture(&state, election, CsbUser::new_test())
            .await
            .unwrap();

        let csb_stores = state
            .csb_store_registry
            .stores_by_scope()
            .await
            .expect("csb stores");
        assert_eq!(
            csb_stores.len(),
            1,
            "a second fixture import should be skipped"
        );
    }

    #[tokio::test]
    async fn fixture_import_adds_omissions_in_every_category() {
        let store = fixture_store(ElectionConfig::EK27).await;
        let omissions: Vec<Omission> = store.data.read().omissions.values().cloned().collect();

        assert_eq!(omissions.len(), 7);
        assert!(
            omissions
                .iter()
                .any(|o| o.category == OmissionCategory::Appellation)
        );
        assert!(
            omissions
                .iter()
                .any(|o| matches!(o.category, OmissionCategory::CandidateList(_)))
        );
        assert!(
            omissions
                .iter()
                .any(|o| matches!(o.category, OmissionCategory::DeclarationsOfSupport(_)))
        );
        assert_eq!(
            omissions
                .iter()
                .filter(|o| matches!(o.category, OmissionCategory::Candidate { .. }))
                .count(),
            3
        );

        // One is irreparable; the rest await a decision, some of them part by
        // part, so there are more decisions to take than omissions.
        assert_eq!(omissions.iter().filter(|o| !o.recoverable).count(), 1);
        let actionable: Vec<&Omission> = omissions.iter().filter(|o| o.is_actionable()).collect();
        let decisions: usize = actionable
            .iter()
            .map(|o| o.decision_count(&store.election))
            .sum();
        assert_eq!(actionable.len(), 6);
        assert!(decisions > actionable.len());
        assert_eq!(store.get_recovery_progress().pending, decisions);
        assert!(
            actionable
                .iter()
                .any(|o| o.decision_count(&store.election) == 1)
        );

        // Every token was filled in.
        for omission in &omissions {
            assert!(
                !omission.description.to_string().contains('{'),
                "{:?}",
                omission.description
            );
            assert!(
                omission
                    .help_text()
                    .is_none_or(|help_text| !help_text.to_string().contains('{')),
                "{:?}",
                omission.help_text
            );
        }
    }

    #[tokio::test]
    async fn fixture_omissions_refer_to_imported_lists_and_candidates() {
        let store = fixture_store(ElectionConfig::EK27).await;
        let omissions: Vec<Omission> = store.data.read().omissions.values().cloned().collect();

        for omission in &omissions {
            for district in omission.electoral_districts(&store.election) {
                assert!(
                    store
                        .get_candidate_lists(WithCorrections::Paper)
                        .iter()
                        .any(|list| list.electoral_districts.contains(district)),
                    "no fixture list covers {district:?}"
                );
            }
            for list_id in omission.candidate_lists() {
                let list = store
                    .get_candidate_list(*list_id, WithCorrections::Paper)
                    .expect("the omission refers to a fixture list");
                if let OmissionCategory::Candidate { person, .. } = &omission.category {
                    assert!(list.candidates.contains(person));
                    assert!(store.get_person(*person, WithCorrections::Paper).is_some());
                }
            }
        }
    }

    #[tokio::test]
    async fn single_district_election_gets_omissions_for_its_one_list() {
        let store = fixture_store(ElectionConfig::WS27(WaterCouncil::Rivierenland)).await;
        let omissions: Vec<Omission> = store.data.read().omissions.values().cloned().collect();

        // No other districts, and no candidate on more than one list.
        assert_eq!(omissions.len(), 7);
        assert!(
            omissions
                .iter()
                .all(|o| o.decision_count(&store.election) == 1)
        );
    }

    #[test]
    fn district_names_are_joined_with_en() {
        assert_eq!(district_names(&[]), "");
        assert_eq!(district_names(&[ElectoralDistrict::Utrecht]), "Utrecht");
        assert_eq!(
            district_names(&[
                ElectoralDistrict::Groningen,
                ElectoralDistrict::Fryslan,
                ElectoralDistrict::Utrecht,
            ]),
            "Groningen, Fryslân en Utrecht"
        );
    }
}
