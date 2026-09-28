use chrono::NaiveDate;
use uuid::Uuid;

use crate::{
    AppError, AppRequestState, CsbAction, CsbMainAction, CsbMainStore, CsbStore, CsbStoreData,
    CsbStream, CsbUser, ElectionConfig, ElectoralDistrict, PgStore, PgStoreData, StreamId,
    projection::WithCorrections,
    store::StoreRegistry,
    structs::{
        brp::{BrpFinding, BrpFindingKind, BrpStatus, BrpValue},
        candidate_lists::{CandidateList, CandidateListId},
        common::{Address, PreviousElectionResults},
        csb::{Omission, OmissionCategory, OmissionType, RegisteredPoliticalGroup},
        list_designation::ListDesignation,
        persons::PersonId,
        political_groups::PoliticalGroup,
    },
};

/// Marks CSB imports that were created from fixtures
pub const FIXTURE_IMPORT_HASH: [u8; 32] = [0; 32];

/// What the committee did with a fixture group's candidate lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Handling {
    /// The group handed nothing in; it only exists as a registration.
    NotHandedIn,
    /// Imported as handed in, with nothing to remark on.
    Imported,
    /// Imported with omissions in every category, for the recovery phase.
    WithOmissions,
    /// Imported with the paper documents differing from the package, and no
    /// omissions.
    WithPaperCorrections,
}

/// A political group of the CSB fixture: its registration with the committee
/// and what happened to its lists.
struct FixtureGroup {
    appellation: &'static str,
    /// Votes and seats at the previous election, when the group is registered
    /// with the committee.
    registration: Option<(u64, u32)>,
    handling: Handling,
}

/// The fixture groups. Together they exercise the numbering (Kieswet Art.
/// I 14): registered groups with a seat are numbered on votes, the others by
/// lot; one registration handed nothing in and one import is not registered.
/// Of the seated groups, the one with the most votes has the fewest seats, so
/// the numbering on votes is told apart from a numbering on seats.
const FIXTURE_GROUPS: [FixtureGroup; 6] = [
    FixtureGroup {
        appellation: "Beweging Losse Eindjes",
        registration: Some((1_234_567, 12)),
        handling: Handling::WithOmissions,
    },
    FixtureGroup {
        appellation: "De Stille Meerderheid",
        registration: Some((612_340, 3)),
        handling: Handling::Imported,
    },
    FixtureGroup {
        appellation: "Partij Puntkomma",
        registration: Some((456_789, 4)),
        handling: Handling::WithPaperCorrections,
    },
    FixtureGroup {
        appellation: "Uitgeslapen Nederland",
        registration: Some((210_543, 2)),
        handling: Handling::NotHandedIn,
    },
    FixtureGroup {
        appellation: "Kiesvereniging Bijna Gekozen",
        registration: Some((98_765, 0)),
        handling: Handling::Imported,
    },
    FixtureGroup {
        appellation: "Actiegroep Laatste Moment",
        registration: None,
        handling: Handling::Imported,
    },
];

/// The fixture group that also hands its package in ahead of nomination day
/// for the pre-submission check (*voorinlevering*).
const PRE_SUBMISSION_GROUP: &str = "De Stille Meerderheid";

impl FixtureGroup {
    /// The group's registration with the committee, under a stable id.
    fn registered_political_group(&self) -> Option<RegisteredPoliticalGroup> {
        let (votes, seats) = self.registration?;
        Some(RegisteredPoliticalGroup {
            id: Uuid::new_v5(&Uuid::NAMESPACE_OID, self.appellation.as_bytes()).into(),
            appellation: self.appellation.parse().expect("fixture appellation"),
            previous_votes: votes.into(),
            previous_seats: seats.into(),
        })
    }

    /// The group as its political group would have entered it, its previous
    /// result matching the registration.
    fn political_group(&self) -> PoliticalGroup {
        let previous_election_results = match self.registration {
            Some((_, 0)) | None => PreviousElectionResults::ZeroSeats,
            Some((_, 1..=15)) => PreviousElectionResults::OneToFifteenSeats,
            Some((_, _)) => PreviousElectionResults::SixteenOrMoreSeats,
        };
        PoliticalGroup {
            appellation: Some(self.appellation.parse().expect("fixture appellation")),
            list_designation: Some(ListDesignation::Standalone),
            previous_election_results: Some(previous_election_results),
        }
    }
}

/// Register the fixture political groups with the committee, import their
/// candidate lists as CSB streams and import one of them for the pre-submission
/// check, each unless that fixture was imported already.
pub async fn import_csb_fixture<S: AppRequestState>(
    state: &S,
    election: ElectionConfig,
    user: CsbUser,
) -> Result<(), AppError> {
    if !has_fixture_import(state.csb_store_registry(), election).await? {
        import_examination_fixture(state, election, &user).await?;
    }
    if !has_fixture_import(state.pre_submission_store_registry(), election).await? {
        import_pre_submission_fixture(state, election, &user).await?;
    }

    Ok(())
}

/// Whether `registry` holds a fixture import for this election.
async fn has_fixture_import(
    registry: &StoreRegistry<CsbStoreData>,
    election: ElectionConfig,
) -> Result<bool, AppError> {
    for store in registry.stores_for_election(election).await? {
        let comes_from_fixtures = store.data.read().events.first().is_some_and(
            |e| matches!(&e.payload.action, CsbAction::Import { hash, .. } if *hash == FIXTURE_IMPORT_HASH),
        );
        if comes_from_fixtures {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Register the fixture political groups with the committee and import their
/// candidate lists as CSB streams for the examination.
async fn import_examination_fixture<S: AppRequestState>(
    state: &S,
    election: ElectionConfig,
    user: &CsbUser,
) -> Result<(), AppError> {
    let main_store = state.csb_main_store(election).await?;

    for group in &FIXTURE_GROUPS {
        register(&main_store, user, group).await?;

        let store = match group.handling {
            Handling::NotHandedIn => continue,
            Handling::Imported | Handling::WithOmissions | Handling::WithPaperCorrections => {
                import_fixture_group(
                    state,
                    state.csb_store_registry(),
                    election,
                    user.clone(),
                    group,
                )
                .await?
            }
        };

        match group.handling {
            Handling::WithOmissions => {
                for omission in fixture_omissions(&store) {
                    omission.create(&store).await?;
                }
            }
            Handling::WithPaperCorrections => fixture_paper_corrections(&store).await?,
            Handling::NotHandedIn | Handling::Imported => {}
        }
    }

    Ok(())
}

/// Import the [`PRE_SUBMISSION_GROUP`]'s package for the pre-submission check,
/// with the BRP check already done and some candidates having BRP
/// discrepancies.
async fn import_pre_submission_fixture<S: AppRequestState>(
    state: &S,
    election: ElectionConfig,
    user: &CsbUser,
) -> Result<(), AppError> {
    let group = FIXTURE_GROUPS
        .iter()
        .find(|group| group.appellation == PRE_SUBMISSION_GROUP)
        .expect("the pre-submission group is a fixture group");
    let store = import_fixture_group(
        state,
        state.pre_submission_store_registry(),
        election,
        user.clone(),
        group,
    )
    .await?;

    fixture_brp_check(&store).await
}

/// Record the group's registration on the main stream, unless the group is
/// unregistered or its appellation was registered by hand already.
async fn register(
    main_store: &CsbMainStore,
    user: &CsbUser,
    group: &FixtureGroup,
) -> Result<(), AppError> {
    let Some(registration) = group.registered_political_group() else {
        return Ok(());
    };
    if main_store.has_registered_appellation(&registration.appellation, None) {
        return Ok(());
    }
    main_store
        .update(CsbMainAction::CreateRegisteredPoliticalGroup(registration).by(user.clone()))
        .await
}

/// Load the fixtures into a fresh political group stream as `group` and
/// import it as a fresh stream of `registry`.
async fn import_fixture_group<S: AppRequestState>(
    state: &S,
    registry: &StoreRegistry<CsbStoreData>,
    election: ElectionConfig,
    user: CsbUser,
    group: &FixtureGroup,
) -> Result<CsbStore, AppError> {
    let pg_stream_id = StreamId::new();
    let app_store = state
        .store_for_stream(pg_stream_id, election, false)
        .await?;
    crate::fixtures::load_for_group(&PgStore::own(app_store.clone()), group.political_group())
        .await?;
    let events = app_store.data.read().events.clone();
    let snapshot = PgStoreData::snapshot_until(&events, usize::MAX);

    let store = CsbStore::acting_as(
        registry.get_or_create(StreamId::new(), election).await?,
        user,
    );
    store
        .update(CsbAction::Import {
            hash: FIXTURE_IMPORT_HASH,
            source_stream_id: pg_stream_id,
            snapshot: Box::new(snapshot),
        })
        .await?;

    Ok(store)
}

/// The outcome of a BRP check over the whole package, recorded rather than
/// looked up so the fixture needs no BRP to be reachable: the first four
/// candidates on the first list have discrepancies, one of them on two fields,
/// and the BRP agrees with everyone else.
async fn fixture_brp_check(store: &CsbStore) -> Result<(), AppError> {
    let flagged: Vec<PersonId> = fixture_lists(store)
        .first()
        .map(|list| list.candidates.iter().copied().take(4).collect())
        .unwrap_or_default();
    let findings_at = |position: usize| -> Vec<BrpFinding> {
        match position {
            0 => vec![
                BrpFindingKind::Mismatch {
                    brp_value: BrpValue::PlaceOfResidence("Utrecht".parse().expect("locality")),
                }
                .into(),
                BrpFindingKind::Mismatch {
                    brp_value: BrpValue::Initials("A.B.C.".parse().expect("initials")),
                }
                .into(),
            ],
            1 => vec![BrpFindingKind::BsnUnknown.into()],
            2 => vec![
                BrpFindingKind::Deceased {
                    date_of_death: NaiveDate::from_ymd_opt(2025, 11, 3),
                }
                .into(),
            ],
            3 => vec![BrpFindingKind::NotDutch.into()],
            _ => Vec::new(),
        }
    };

    for person in store.get_persons(WithCorrections::All) {
        let findings = flagged
            .iter()
            .position(|flagged| *flagged == person.id)
            .map(findings_at)
            .unwrap_or_default();
        store
            .update(CsbAction::BrpPersonChecked {
                person: person.id,
                findings,
            })
            .await?;
    }
    store
        .update(CsbAction::SetBrpStatus(BrpStatus::Finished))
        .await
}

/// Where the paper documents differ from the imported package: the list
/// submitter's house number, and the place of residence and initials of two
/// candidates on the first list.
async fn fixture_paper_corrections(store: &CsbStore) -> Result<(), AppError> {
    let corrections = store.paper_corrections();

    let mut submitter = corrections.get_list_submitter();
    if let Address::Dutch(address) = &mut submitter.address {
        address.house_number = Some("7".parse().expect("house number"));
        address.house_number_addition = None;
    }
    submitter.update(&corrections).await?;

    let Some(first_list) = fixture_lists(store).into_iter().next() else {
        return Ok(());
    };
    let mut candidates = first_list.candidates.iter().skip(1);

    if let Some(person_id) = candidates.next() {
        let mut person = corrections.get_person(*person_id)?;
        person.personal_data.place_of_residence = Some("Utrecht".parse().expect("locality"));
        person.update(&corrections).await?;
    }
    if let Some(person_id) = candidates.next() {
        let mut person = corrections.get_person(*person_id)?;
        person.name.initials = Some("A.B.C.".parse().expect("initials"));
        person.update(&corrections).await?;
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

    let districts: Vec<ElectoralDistrict> = last_list.electoral_districts.iter().copied().collect();
    vec![declarations_of_support_omission(
        b"fixture_omission_declarations_other_districts",
        &districts,
    )]
}

/// Missing declarations of support for `districts`, named in the omission
/// letter note.
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
    use crate::{
        AppState,
        core::election::WaterCouncil,
        csb::examination::{numbering::list_numbering, structs::BrpCheckState},
    };

    /// The fixture groups that were imported, as many as have a handed-in list.
    const IMPORTED_GROUP_COUNT: usize = 5;

    /// The fixture groups registered with the committee.
    const REGISTERED_GROUP_COUNT: usize = 5;

    async fn fixture_state(election: ElectionConfig) -> AppState {
        let state = AppState::new_for_tests().await;
        import_csb_fixture(&state, election, CsbUser::new_test())
            .await
            .unwrap();
        state
    }

    /// Every imported fixture stream, by appellation.
    async fn fixture_stores(state: &AppState) -> Vec<CsbStream> {
        let csb_stores = state
            .csb_store_registry
            .stores_by_scope()
            .await
            .expect("csb stores");
        assert_eq!(csb_stores.len(), IMPORTED_GROUP_COUNT);
        csb_stores
    }

    /// The imported fixture stream named `appellation`.
    async fn fixture_store_named(state: &AppState, appellation: &str) -> CsbStream {
        fixture_stores(state)
            .await
            .into_iter()
            .find(|store| store.get_appellation(WithCorrections::None) == appellation)
            .unwrap_or_else(|| panic!("no fixture import named {appellation:?}"))
    }

    /// The imported fixture stream carrying the omissions.
    async fn fixture_store(election: ElectionConfig) -> CsbStream {
        let state = fixture_state(election).await;
        fixture_store_named(&state, "Beweging Losse Eindjes").await
    }

    /// The streams imported for the pre-submission check.
    async fn pre_submission_stores(state: &AppState) -> Vec<CsbStream> {
        state
            .pre_submission_store_registry
            .stores_by_scope()
            .await
            .expect("pre-submission stores")
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
            IMPORTED_GROUP_COUNT,
            "a second fixture import should be skipped"
        );
        assert_eq!(
            pre_submission_stores(&state).await.len(),
            1,
            "a second pre-submission fixture import should be skipped"
        );
        let main_store = state.csb_main_store(election).await.unwrap();
        assert_eq!(
            main_store.registered_political_groups().len(),
            REGISTERED_GROUP_COUNT,
            "a second fixture import should register nothing"
        );
    }

    #[tokio::test]
    async fn pre_submission_fixture_is_one_group_with_its_brp_check_done() -> Result<(), AppError> {
        let state = fixture_state(ElectionConfig::EK27).await;

        let stores = pre_submission_stores(&state).await;
        assert_eq!(stores.len(), 1);
        let store = &stores[0];
        assert_eq!(
            store.get_appellation(WithCorrections::None),
            PRE_SUBMISSION_GROUP
        );
        // The same package the examination got, without anything the
        // examination adds to it.
        assert_eq!(
            store.get_candidate_lists(WithCorrections::None).len(),
            fixture_store_named(&state, PRE_SUBMISSION_GROUP)
                .await
                .get_candidate_lists(WithCorrections::None)
                .len()
        );
        assert_eq!(store.get_omission_count(), 0);
        assert!(!store.has_paper_corrections());

        // Every candidate was checked; the first four on the first list have
        // five findings between them.
        assert!(matches!(store.get_brp_status(), BrpStatus::Finished));
        assert_eq!(
            BrpCheckState::for_political_group(store),
            BrpCheckState::Errors {
                errors: 5,
                handled: 0
            }
        );
        let first_list = fixture_lists(store).remove(0);
        let findings = store.get_brp_findings();
        let flagged: Vec<usize> = first_list
            .candidates
            .iter()
            .map(|person| findings[person].len())
            .collect();
        assert_eq!(&flagged[..5], &[2, 1, 1, 1, 0]);
        assert!(flagged[4..].iter().all(|count| *count == 0));

        Ok(())
    }

    #[tokio::test]
    async fn pre_submission_fixture_is_added_to_an_existing_examination_fixture()
    -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let election = ElectionConfig::EK27;
        let user = CsbUser::new_test();
        import_examination_fixture(&state, election, &user).await?;
        assert!(pre_submission_stores(&state).await.is_empty());

        import_csb_fixture(&state, election, user).await?;

        assert_eq!(fixture_stores(&state).await.len(), IMPORTED_GROUP_COUNT);
        assert_eq!(pre_submission_stores(&state).await.len(), 1);

        Ok(())
    }

    #[tokio::test]
    async fn fixture_registers_groups_with_their_previous_result() -> Result<(), AppError> {
        let election = ElectionConfig::EK27;
        let state = fixture_state(election).await;
        let main_store = state.csb_main_store(election).await?;

        let registered = main_store.registered_political_groups();
        let summary: Vec<(String, u64, u32)> = registered
            .iter()
            .map(|group| {
                (
                    group.appellation.to_string(),
                    group.previous_votes.value(),
                    group.previous_seats.value(),
                )
            })
            .collect();
        // Most votes first, the way the lists are numbered.
        assert_eq!(
            summary,
            [
                ("Beweging Losse Eindjes".to_string(), 1_234_567, 12),
                ("De Stille Meerderheid".to_string(), 612_340, 3),
                ("Partij Puntkomma".to_string(), 456_789, 4),
                ("Uitgeslapen Nederland".to_string(), 210_543, 2),
                ("Kiesvereniging Bijna Gekozen".to_string(), 98_765, 0),
            ]
        );

        // Registrations have stable ids, so a group can be linked to.
        let again = FIXTURE_GROUPS[0].registered_political_group().unwrap();
        assert_eq!(registered[0].id, again.id);

        // Every imported group entered the previous result its registration
        // gives it; the unregistered one has no seats.
        for store in fixture_stores(&state).await {
            let group = store.get_political_group(WithCorrections::None);
            let expected = match registered
                .iter()
                .find(|r| r.has_appellation(group.appellation.as_ref().unwrap()))
            {
                Some(registration) if registration.previous_seats.value() > 0 => {
                    PreviousElectionResults::OneToFifteenSeats
                }
                _ => PreviousElectionResults::ZeroSeats,
            };
            assert_eq!(group.previous_election_results, Some(expected));
        }

        Ok(())
    }

    #[tokio::test]
    async fn a_registration_made_by_hand_is_kept() -> Result<(), AppError> {
        let state = AppState::new_for_tests().await;
        let election = ElectionConfig::EK27;
        let main_store = state.csb_main_store(election).await?;
        let by_hand = RegisteredPoliticalGroup {
            id: crate::structs::csb::RegisteredPoliticalGroupId::new(),
            appellation: "beweging losse eindjes".parse().unwrap(),
            previous_votes: 1.into(),
            previous_seats: 1.into(),
        };
        main_store
            .update(
                CsbMainAction::CreateRegisteredPoliticalGroup(by_hand.clone())
                    .by(CsbUser::new_test()),
            )
            .await?;

        import_csb_fixture(&state, election, CsbUser::new_test()).await?;

        let registered = main_store.registered_political_groups();
        assert_eq!(registered.len(), REGISTERED_GROUP_COUNT);
        assert_eq!(
            main_store.get_registered_political_group(by_hand.id)?,
            by_hand
        );

        Ok(())
    }

    #[tokio::test]
    async fn fixture_numbers_seated_groups_on_votes_and_the_rest_by_lot() -> Result<(), AppError> {
        let election = ElectionConfig::EK27;
        let state = fixture_state(election).await;
        let main_store = state.csb_main_store(election).await?;

        let numbering = list_numbering(state.csb_store_registry(), &main_store).await?;

        // The demo group's unregistered appellation is scrapped by its
        // irreparable omission, so it continues as a blank list, by lot.
        let on_votes: Vec<(&str, Option<usize>)> = numbering
            .on_votes()
            .map(|group| (group.appellation.as_str(), group.position))
            .collect();
        // Most votes first, not most seats first.
        assert_eq!(
            on_votes,
            [
                ("De Stille Meerderheid", Some(1)),
                ("Partij Puntkomma", Some(2))
            ]
        );

        // By lot: every fixture list covers the same districts, so the
        // registered group without a seat sorts alphabetically with the rest.
        let by_lot: Vec<&str> = numbering
            .by_lot()
            .map(|group| group.appellation.as_str())
            .collect();
        assert_eq!(by_lot.len(), 3, "{by_lot:?}");
        assert_eq!(by_lot[0], "Actiegroep Laatste Moment");
        assert!(by_lot[1].starts_with("Blanco ("), "{by_lot:?}");
        assert_eq!(by_lot[2], "Kiesvereniging Bijna Gekozen");
        assert!(numbering.by_lot().all(|group| group.position.is_none()));

        // The registration that handed nothing in is not numbered.
        assert!(
            numbering
                .groups
                .iter()
                .all(|group| group.appellation != "Uitgeslapen Nederland")
        );

        Ok(())
    }

    #[tokio::test]
    async fn paper_corrected_group_has_corrections_and_no_omissions() -> Result<(), AppError> {
        let state = fixture_state(ElectionConfig::EK27).await;
        let store = fixture_store_named(&state, "Partij Puntkomma").await;

        assert!(store.has_paper_corrections());
        assert_eq!(store.get_omission_count(), 0);
        assert_eq!(store.get_correction_count(), 0, "no CSB corrections");

        // The submitter's house number differs from the package.
        let imported = store.get_list_submitter(WithCorrections::None);
        let corrected = store.get_list_submitter(WithCorrections::Paper);
        assert_eq!(imported.id, corrected.id);
        assert_ne!(
            imported.address.house_number(),
            corrected.address.house_number()
        );
        assert_eq!(corrected.address.house_number().as_deref(), Some("7"));

        // Two candidates on the first list differ from the package.
        let list = fixture_lists(&store).remove(0);
        let changed: Vec<PersonId> = list
            .candidates
            .iter()
            .copied()
            .filter(|person| {
                store.get_person(*person, WithCorrections::None)
                    != store.get_person(*person, WithCorrections::Paper)
            })
            .collect();
        assert_eq!(changed, list.candidates[1..3].to_vec());
        let residence = store
            .get_person(changed[0], WithCorrections::Paper)
            .and_then(|person| person.personal_data.place_of_residence);
        assert_eq!(
            residence.map(|p| p.to_string()),
            Some("Utrecht".to_string())
        );
        let initials = store
            .get_person(changed[1], WithCorrections::Paper)
            .and_then(|person| person.name.initials)
            .map(|initials| initials.to_string());
        assert_eq!(initials, Some("A.B.C.".to_string()));

        Ok(())
    }

    #[tokio::test]
    async fn plain_imports_have_neither_omissions_nor_corrections() {
        let state = fixture_state(ElectionConfig::EK27).await;

        for appellation in [
            "De Stille Meerderheid",
            "Kiesvereniging Bijna Gekozen",
            "Actiegroep Laatste Moment",
        ] {
            let store = fixture_store_named(&state, appellation).await;
            assert_eq!(store.get_omission_count(), 0, "{appellation}");
            assert!(!store.has_paper_corrections(), "{appellation}");
            assert!(
                !store.get_candidate_lists(WithCorrections::None).is_empty(),
                "{appellation}"
            );
        }
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
