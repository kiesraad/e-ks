use axum::{extract::FromRequestParts, http::request::Parts};

use std::collections::{BTreeSet, HashMap};

use crate::{
    AppError, AppRequestState, CsbStoreData, CsbStream, ElectionConfig, ElectoralDistrict, Session,
    StreamId,
    csb::examination::structs::BrpCheckState,
    projection::{Scrapped, WithCorrections},
    structs::{
        candidate_lists::CandidateListId,
        common::{Appellation, FullName, LegalName},
        csb::{CsbPhase, RecoveryProgress, RegisteredPoliticalGroup},
        list_designation::ListDesignation,
        political_groups::PoliticalGroup,
    },
};

pub struct CsbPoliticalGroup {
    pub political_group: PoliticalGroup,
    pub stream_id: StreamId,
    pub election: ElectionConfig,
    pub brp: BrpCheckState,
    /// The phase the group is rendered for; drives which links and actions the
    /// shared examination templates render (see the path helpers in `paths.rs`).
    pub mode: CsbPhase,
    pub is_examination_finished: bool,
    pub is_deleted: bool,
    /// What the unresolved omissions scrap; only rendered in
    /// [`CsbPhase::Recovery`].
    pub scrapped: Scrapped,
    pub restoration_count: usize,
    pub omission_count: usize,
    /// How far the group is through the recovery phase; only meaningful in
    /// [`CsbPhase::Recovery`].
    pub recovery: RecoveryProgress,
    pub first_candidate_name: Option<FullName>,
    pub first_non_scrapped_candidate_name: Option<FullName>,
    /// The statutory names on the group's name authorisations, one per party
    /// behind a combined list, which is matched to its registrations on them.
    pub legal_names: Vec<LegalName>,
    /// Whether the group obtained a seat at the previous election according to
    /// its registration; such a group needs no declarations of support
    /// (Kieswet Art. H 4). Set by [`Self::with_registrations`].
    pub previously_seated: bool,
    /// Whether the group carries an appellation that matches none of the
    /// committee's registrations, or for a combined list whether one of its
    /// parties is not registered. Set by [`Self::with_registrations`].
    pub is_unregistered: bool,
    /// The statutory names of a combined list that match no registration.
    /// Set by [`Self::with_registrations`].
    pub unregistered_legal_names: Vec<LegalName>,
    /// The electoral districts of each candidate list, which is how the
    /// shared templates name a list (see [`Self::candidate_list_districts`]).
    pub candidate_list_districts: HashMap<CandidateListId, BTreeSet<ElectoralDistrict>>,
}

impl CsbPoliticalGroup {
    pub fn new_from_csb_store(store: &CsbStream) -> Self {
        Self::new(store, &store.snapshot())
    }

    /// The group as `data`, a snapshot of `store`, shows it.
    pub fn new(store: &CsbStream, data: &CsbStoreData) -> Self {
        let election = store.election;
        let scrapped = data.get_scrapped().clone();
        let corrected = data.pg(WithCorrections::All);
        Self {
            political_group: corrected.get_political_group().clone(),
            stream_id: store.stream_id,
            election,
            brp: BrpCheckState::for_political_group(data),
            mode: CsbPhase::Examination,
            is_examination_finished: data.is_examination_finished(),
            is_deleted: data.is_deleted(),
            restoration_count: data.get_restoration_count(),
            omission_count: data.get_omission_count(),
            recovery: data.get_recovery_progress(election),
            first_candidate_name: data
                .pg(WithCorrections::All)
                .get_first_candidate_name()
                .cloned(),
            first_non_scrapped_candidate_name: data
                .get_first_candidate_name(WithCorrections::All, &scrapped)
                .cloned(),
            legal_names: corrected
                .get_name_authorisations()
                .into_iter()
                .map(|authorisation| authorisation.legal_name.clone())
                .filter(|legal_name| !legal_name.is_empty())
                .collect(),
            previously_seated: false,
            is_unregistered: false,
            unregistered_legal_names: Vec::new(),
            scrapped,
            candidate_list_districts: corrected
                .get_candidate_lists()
                .into_iter()
                .map(|list| (list.id, list.electoral_districts.clone()))
                .collect(),
        }
    }

    pub fn with_mode(mut self, mode: CsbPhase) -> Self {
        self.mode = mode;
        self
    }

    /// Look the group up among the committee's registrations, which decide
    /// whether it was previously seated.
    pub fn with_registrations(mut self, registered: &[RegisteredPoliticalGroup]) -> Self {
        let registration = self.registration(registered);
        self.previously_seated =
            registration.is_some_and(RegisteredPoliticalGroup::was_previously_seated);
        if self.registered_appellation().is_some() && self.political_group.is_combined() {
            self.unregistered_legal_names = self
                .legal_names
                .iter()
                .filter(|name| !registered.iter().any(|r| r.has_legal_name(name)))
                .cloned()
                .collect();
            self.is_unregistered =
                registration.is_none() || !self.unregistered_legal_names.is_empty();
        } else {
            self.is_unregistered =
                registration.is_none() && self.registered_appellation().is_some();
        }
        self
    }

    /// Whether the group's candidate lists have to be told apart at all.
    pub fn has_multiple_candidate_lists(&self) -> bool {
        self.candidate_list_districts.len() > 1
    }

    /// The districts of one candidate list, empty when the list is unknown.
    pub fn candidate_list_districts(
        &self,
        list_id: &CandidateListId,
    ) -> impl Iterator<Item = &ElectoralDistrict> {
        self.candidate_list_districts
            .get(list_id)
            .into_iter()
            .flatten()
    }

    /// The name shown for the group. Once its appellation is scrapped, the
    /// recovery phase names it as the blank list it continues as.
    pub fn csb_appellation(&self) -> String {
        match self.mode {
            CsbPhase::Examination => self
                .political_group
                .csb_appellation(self.first_candidate_name.as_ref()),
            CsbPhase::Recovery => self.numbering_appellation(),
        }
    }

    /// The name the group's lists are numbered under, honouring scrapping:
    /// the appellation, or the blank list the group continues as once its
    /// appellation is scrapped, named after its first non-scrapped candidate.
    pub fn numbering_appellation(&self) -> String {
        if self.is_appellation_scrapped() {
            return self.blank_appellation();
        }
        self.political_group
            .csb_appellation(self.first_non_scrapped_candidate_name.as_ref())
    }

    /// The group named as a blank list, e.g. `Blanco (Nagelhout, H.)`.
    fn blank_appellation(&self) -> String {
        PoliticalGroup {
            list_designation: Some(ListDesignation::Blank),
            ..self.political_group.clone()
        }
        .csb_appellation(self.first_non_scrapped_candidate_name.as_ref())
    }

    /// The registered appellation to match the group on: none for a blank
    /// list or once the appellation is scrapped.
    pub fn registered_appellation(&self) -> Option<&Appellation> {
        if self.is_appellation_scrapped() || self.political_group.is_blank() {
            return None;
        }
        self.political_group.appellation.as_ref()
    }

    /// The registrations behind the group. A combined list has those whose
    /// statutory name is on one of its name authorisations. Any other list has
    /// the one it is bound to, or failing that the one its appellation
    /// matches, for groups imported before the binding existed.
    pub fn registrations<'a>(
        &self,
        registered: &'a [RegisteredPoliticalGroup],
    ) -> Vec<&'a RegisteredPoliticalGroup> {
        let Some(appellation) = self.registered_appellation() else {
            return Vec::new();
        };
        if self.political_group.is_combined() {
            return registered
                .iter()
                .filter(|registration| {
                    self.legal_names
                        .iter()
                        .any(|name| registration.has_legal_name(name))
                })
                .collect();
        }
        let bound = self.political_group.registration_id();
        registered
            .iter()
            .find(|registration| Some(registration.id) == bound)
            .or_else(|| {
                registered
                    .iter()
                    .find(|registration| registration.has_appellation(appellation))
            })
            .into_iter()
            .collect()
    }

    /// The registration the group is numbered and exempted on: for a combined
    /// list the one of its parties with the most votes, as their results are
    /// not added up.
    pub fn registration<'a>(
        &self,
        registered: &'a [RegisteredPoliticalGroup],
    ) -> Option<&'a RegisteredPoliticalGroup> {
        self.registrations(registered)
            .into_iter()
            .min_by(|a, b| a.numbering_order(b))
    }

    /// The districts in which the group still has a valid list.
    pub fn valid_districts(&self) -> Vec<ElectoralDistrict> {
        let mut districts = Vec::new();
        for (list, list_districts) in &self.candidate_list_districts {
            if self.scrapped.is_list_scrapped(*list) {
                continue;
            }
            for district in list_districts {
                if !self.scrapped.is_district_scrapped(*district) && !districts.contains(district) {
                    districts.push(*district);
                }
            }
        }
        districts
    }

    pub fn is_appellation_scrapped(&self) -> bool {
        self.scrapped.is_appellation_scrapped()
    }
}

#[cfg(test)]
impl CsbPoliticalGroup {
    /// An unexamined, undeleted group named `appellation` with one candidate
    /// list in Groningen; override fields with struct update syntax.
    pub fn sample(appellation: &str) -> Self {
        Self {
            political_group: PoliticalGroup {
                appellation: Some(appellation.parse().unwrap()),
                ..crate::test_utils::sample_political_group()
            },
            stream_id: StreamId::new(),
            election: ElectionConfig::EK27,
            brp: BrpCheckState::NotChecked,
            mode: CsbPhase::Examination,
            is_examination_finished: false,
            is_deleted: false,
            scrapped: Default::default(),
            restoration_count: 0,
            omission_count: 0,
            recovery: Default::default(),
            first_candidate_name: None,
            first_non_scrapped_candidate_name: None,
            legal_names: Vec::new(),
            previously_seated: false,
            is_unregistered: false,
            unregistered_legal_names: Vec::new(),
            candidate_list_districts: HashMap::from([(
                CandidateListId::new(),
                BTreeSet::from([ElectoralDistrict::Groningen]),
            )]),
        }
    }
}

/// Extracts the imported political groups of the election the session works
/// on. Streams of the other elections stay out of the listing: they are
/// examined under their own election's ruleset, in a session that picked it.
pub struct CsbPoliticalGroups(pub Vec<CsbPoliticalGroup>);

impl<S: AppRequestState> FromRequestParts<S> for CsbPoliticalGroups {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let session = Session::from_request_parts(parts, state).await?;
        let election = session.require_current_election()?;
        let registry = state.csb_store_registry();

        let mut political_groups = Vec::new();
        for store in registry.stores_for_election(election).await? {
            political_groups.push(CsbPoliticalGroup::new_from_csb_store(&store));
        }

        Ok(CsbPoliticalGroups(political_groups))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use crate::structs::csb::{RegisteredPoliticalGroupId, sample_registered_political_group};
    use axum::{body::Body, http::Request};

    use crate::{
        AppState, CsbAction, CsbStore, CsbUser, ElectionConfig, Locale, PgStoreData, Province,
        structs::csb::{OmissionCategory, OmissionId, sample_omission},
    };

    /// Persist a CSB stream carrying a single import event in the (in-memory)
    /// test registry, returning its `stream_id`.
    async fn seed_csb_store(state: &AppState, election: ElectionConfig) -> StreamId {
        let stream_id = StreamId::new();
        let store = state
            .csb_store_for_stream(stream_id, election)
            .await
            .unwrap();
        store
            .update(
                CsbAction::Import {
                    hash: [0u8; 32],
                    source_stream_id: StreamId::new(),
                    snapshot: Box::new(PgStoreData::default()),
                }
                .by(CsbUser::new_test()),
            )
            .await
            .unwrap();
        stream_id
    }

    /// Request parts carrying a committee session on `election`, as the
    /// session middleware injects them for a real request.
    fn committee_parts(election: ElectionConfig) -> axum::http::request::Parts {
        let mut parts = Request::builder()
            .uri("/csb/examination")
            .body(Body::empty())
            .unwrap()
            .into_parts()
            .0;
        parts.extensions.insert(Session::for_committee(
            CsbUser::new_test(),
            election,
            Locale::default(),
        ));
        parts
    }

    #[tokio::test]
    async fn returns_every_political_group_of_the_session_election() {
        let state = AppState::new_for_tests().await;
        let first = seed_csb_store(&state, ElectionConfig::EK27).await;
        let second = seed_csb_store(&state, ElectionConfig::EK27).await;

        let mut parts = committee_parts(ElectionConfig::EK27);
        let CsbPoliticalGroups(groups) = CsbPoliticalGroups::from_request_parts(&mut parts, &state)
            .await
            .unwrap();

        assert_eq!(groups.len(), 2);
        let stream_ids: Vec<_> = groups.iter().map(|g| g.stream_id).collect();
        assert!(stream_ids.contains(&first));
        assert!(stream_ids.contains(&second));
    }

    #[tokio::test]
    async fn skips_political_groups_of_another_election() {
        let state = AppState::new_for_tests().await;
        let own = seed_csb_store(&state, ElectionConfig::EK27).await;
        seed_csb_store(&state, ElectionConfig::PS27(Province::Groningen)).await;

        let mut parts = committee_parts(ElectionConfig::EK27);
        let CsbPoliticalGroups(groups) = CsbPoliticalGroups::from_request_parts(&mut parts, &state)
            .await
            .unwrap();

        let stream_ids: Vec<_> = groups.iter().map(|g| g.stream_id).collect();
        assert_eq!(stream_ids, vec![own]);
    }

    #[tokio::test]
    async fn returns_empty_when_nothing_imported() {
        let state = AppState::new_for_tests().await;

        let mut parts = committee_parts(ElectionConfig::EK27);
        let CsbPoliticalGroups(groups) = CsbPoliticalGroups::from_request_parts(&mut parts, &state)
            .await
            .unwrap();

        assert!(groups.is_empty());
    }

    #[test]
    fn registration_prefers_the_bound_id_over_the_appellation() {
        let bound = sample_registered_political_group("Nieuwe Naam", 100, 1);
        let by_name = sample_registered_political_group("Kiesraad Demo", 50, 1);
        let registered = [by_name.clone(), bound.clone()];

        let group = CsbPoliticalGroup {
            political_group: PoliticalGroup {
                list_designation: Some(ListDesignation::Standalone {
                    registration: Some(bound.id),
                }),
                ..CsbPoliticalGroup::sample("Kiesraad Demo").political_group
            },
            ..CsbPoliticalGroup::sample("Kiesraad Demo")
        };
        assert_eq!(group.registration(&registered), Some(&bound));

        // Imported before the binding existed, or bound to a registration
        // the committee deleted since: matched on appellation.
        let unbound = CsbPoliticalGroup::sample("kiesraad demo");
        assert_eq!(unbound.registration(&registered), Some(&by_name));
        let dangling = CsbPoliticalGroup {
            political_group: PoliticalGroup {
                list_designation: Some(ListDesignation::Standalone {
                    registration: Some(RegisteredPoliticalGroupId::new()),
                }),
                ..CsbPoliticalGroup::sample("Kiesraad Demo").political_group
            },
            ..CsbPoliticalGroup::sample("Kiesraad Demo")
        };
        assert_eq!(dangling.registration(&registered), Some(&by_name));

        // A blank list has no registration, however it is bound.
        let blank = CsbPoliticalGroup {
            political_group: PoliticalGroup {
                list_designation: Some(ListDesignation::Blank),
                ..CsbPoliticalGroup::sample("Kiesraad Demo").political_group
            },
            ..CsbPoliticalGroup::sample("Kiesraad Demo")
        };
        assert_eq!(blank.registration(&registered), None);
    }

    /// A combined list named `A/B` whose name authorisations carry
    /// `legal_names`.
    fn combined(legal_names: &[&str]) -> CsbPoliticalGroup {
        let sample = CsbPoliticalGroup::sample("A/B");
        CsbPoliticalGroup {
            political_group: PoliticalGroup {
                list_designation: Some(ListDesignation::Combined {
                    registrations: Vec::new(),
                }),
                ..sample.political_group
            },
            legal_names: legal_names
                .iter()
                .map(|name| name.parse().unwrap())
                .collect(),
            ..CsbPoliticalGroup::sample("A/B")
        }
    }

    #[test]
    fn a_combined_list_is_matched_on_the_legal_names_of_its_parties() {
        let a = sample_registered_political_group("A", 100, 0);
        let b = sample_registered_political_group("B", 300, 2);
        let other = sample_registered_political_group("A/B", 500, 3);
        let registered = [a.clone(), b.clone(), other];

        // Matched on statutory name, not on the combined appellation; the
        // party with the most votes stands for the combination.
        let group = combined(&["vereniging a", "Vereniging B"]);
        assert_eq!(group.registrations(&registered), [&a, &b]);
        assert_eq!(group.registration(&registered), Some(&b));
        let group = group.with_registrations(&registered);
        assert!(group.previously_seated);
        assert!(!group.is_unregistered);

        // One party unregistered: still matched on the other, with a warning
        // naming the unregistered one.
        let group = combined(&["Vereniging A", "Vereniging C"]).with_registrations(&registered);
        assert!(!group.previously_seated);
        assert!(group.is_unregistered);
        assert_eq!(
            group.unregistered_legal_names,
            ["Vereniging C".parse::<LegalName>().unwrap()]
        );

        // Without name authorisations nothing matches.
        let group = combined(&[]).with_registrations(&registered);
        assert_eq!(group.registration(&registered), None);
        assert!(group.is_unregistered);
        assert!(group.unregistered_legal_names.is_empty());
    }

    #[test]
    fn csb_appellation_returns_appellation_for_normal_list() {
        let group = CsbPoliticalGroup::sample("Kiesraad Demo");

        assert_eq!(group.csb_appellation(), "Kiesraad Demo");
    }

    #[test]
    fn csb_appellation_names_a_scrapped_appellation_as_blank_in_recovery_only() {
        let store = CsbStore::new_for_test();
        store.set_political_group(PoliticalGroup {
            appellation: Some("Kiesraad Demo".parse().unwrap()),
            list_designation: Some(ListDesignation::Standalone { registration: None }),
            ..Default::default()
        });
        let mut omission = sample_omission(OmissionCategory::Appellation);
        omission.recoverable = false;
        store.insert_omission(omission);

        let group = CsbPoliticalGroup::new_from_csb_store(&store);
        assert_eq!(group.csb_appellation(), "Kiesraad Demo");
        assert_eq!(
            group.with_mode(CsbPhase::Recovery).csb_appellation(),
            "Blanco"
        );
    }

    #[test]
    fn csb_appellation_blank_list_with_candidate_uses_first_candidate_name() {
        let group = CsbPoliticalGroup {
            political_group: PoliticalGroup {
                list_designation: Some(ListDesignation::Blank),
                ..Default::default()
            },
            first_candidate_name: Some(FullName {
                last_name: "Jansen".parse().unwrap(),
                initials: Some("A.B.".parse().unwrap()),
                ..Default::default()
            }),
            ..CsbPoliticalGroup::sample("Kiesraad Demo")
        };

        assert_eq!(group.csb_appellation(), "Blanco (Jansen, A.B.)");
    }

    #[test]
    fn csb_appellation_blank_list_without_candidates_uses_blanco_fallback() {
        let group = CsbPoliticalGroup {
            political_group: PoliticalGroup {
                list_designation: Some(ListDesignation::Blank),
                ..Default::default()
            },
            ..CsbPoliticalGroup::sample("Kiesraad Demo")
        };

        assert_eq!(group.csb_appellation(), "Blanco");
    }

    #[test]
    fn recovery_mode_appellation_honours_scrapping() {
        let group = CsbPoliticalGroup {
            political_group: PoliticalGroup {
                // should convert to Blank because of appellation scrapping
                list_designation: Some(ListDesignation::Standalone { registration: None }),
                ..Default::default()
            },
            mode: CsbPhase::Recovery,
            first_candidate_name: Some(FullName {
                first_name: None,
                last_name: "Scrapped".parse().unwrap(),
                last_name_prefix: None,
                initials: Some("S.".parse().unwrap()),
            }),
            // should be used as first candidate
            first_non_scrapped_candidate_name: Some(FullName {
                first_name: None,
                last_name: "Present".parse().unwrap(),
                last_name_prefix: None,
                initials: Some("P.".parse().unwrap()),
            }),
            scrapped: Scrapped::new_for_test(
                BTreeSet::from([OmissionId::new()]),
                BTreeSet::new(),
                BTreeMap::new(),
            ),
            ..CsbPoliticalGroup::sample("Kiesraad Demo")
        };

        assert_eq!("Blanco (Present, P.)", group.csb_appellation());
    }
}
