use axum::{extract::FromRequestParts, http::request::Parts};

use std::collections::HashMap;

use crate::{
    AppError, AppRequestState, CsbStream, ElectoralDistrict, Session, StreamId,
    csb::examination::structs::BrpCheckState,
    structs::{
        candidate_lists::CandidateListId, common::FullName, csb::CsbPhase,
        political_groups::PoliticalGroup,
    },
};

pub struct CsbPoliticalGroup {
    pub political_group: PoliticalGroup,
    pub stream_id: StreamId,
    pub brp: BrpCheckState,
    /// The phase the group is rendered for; drives which links and actions the
    /// shared examination templates render (see the path helpers in `paths.rs`).
    pub mode: CsbPhase,
    pub is_examination_finished: bool,
    pub is_deleted: bool,
    pub restoration_count: usize,
    pub omission_count: usize,
    pub pending_omission_count: usize,
    pub actionable_omission_count: usize,
    pub first_candidate_name: Option<FullName>,
    /// The electoral districts of each candidate list, which is how the
    /// shared templates name a list (see [`Self::candidate_list_districts`]).
    pub candidate_list_districts: HashMap<CandidateListId, Vec<ElectoralDistrict>>,
}

impl CsbPoliticalGroup {
    pub fn new_from_csb_store(store: &CsbStream) -> Self {
        Self {
            political_group: store.get_political_group(crate::projection::WithCorrections::All),
            stream_id: store.stream_id,
            brp: BrpCheckState::for_political_group(store),
            mode: CsbPhase::Examination,
            is_examination_finished: store.is_examination_finished(),
            is_deleted: store.is_deleted(),
            restoration_count: store.get_restoration_count(),
            omission_count: store.get_omission_count(),
            pending_omission_count: store.get_pending_omission_count(),
            actionable_omission_count: store.get_actionable_omission_count(),
            first_candidate_name: store
                .get_first_candidate_name(crate::projection::WithCorrections::All),
            candidate_list_districts: store
                .get_candidate_lists(crate::projection::WithCorrections::All)
                .into_iter()
                .map(|list| (list.id, list.electoral_districts))
                .collect(),
        }
    }

    pub fn with_mode(mut self, mode: CsbPhase) -> Self {
        self.mode = mode;
        self
    }

    /// Whether the group's candidate lists have to be told apart at all.
    pub fn has_multiple_candidate_lists(&self) -> bool {
        self.candidate_list_districts.len() > 1
    }

    /// The districts of one candidate list, empty when the list is unknown.
    pub fn candidate_list_districts(&self, list_id: &CandidateListId) -> &[ElectoralDistrict] {
        self.candidate_list_districts
            .get(list_id)
            .map_or(&[], Vec::as_slice)
    }

    /// The number of omissions already assessed in the recovery phase.
    pub fn decided_omission_count(&self) -> usize {
        self.actionable_omission_count - self.pending_omission_count
    }

    pub fn csb_appellation(&self) -> String {
        self.political_group
            .csb_appellation(self.first_candidate_name.as_ref())
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
    use super::*;
    use axum::{body::Body, http::Request};

    use crate::{
        AppState, CsbAction, CsbUser, ElectionConfig, Locale, PgStoreData, Province,
        structs::list_designation::ListDesignation,
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
    fn csb_appellation_returns_appellation_for_normal_list() {
        let group = CsbPoliticalGroup {
            political_group: PoliticalGroup {
                appellation: Some("Kiesraad Demo".parse().unwrap()),
                list_designation: Some(ListDesignation::Standalone),
                ..Default::default()
            },
            stream_id: StreamId::new(),
            brp: BrpCheckState::NotChecked,
            mode: CsbPhase::Examination,
            is_examination_finished: false,
            is_deleted: false,
            restoration_count: 0,
            omission_count: 0,
            pending_omission_count: 0,
            actionable_omission_count: 0,
            first_candidate_name: None,
            candidate_list_districts: HashMap::new(),
        };

        assert_eq!(group.csb_appellation(), "Kiesraad Demo");
    }

    #[test]
    fn csb_appellation_blank_list_with_candidate_uses_first_candidate_name() {
        let group = CsbPoliticalGroup {
            political_group: PoliticalGroup {
                list_designation: Some(ListDesignation::Blank),
                ..Default::default()
            },
            stream_id: StreamId::new(),
            brp: BrpCheckState::NotChecked,
            mode: CsbPhase::Examination,
            is_examination_finished: false,
            is_deleted: false,
            restoration_count: 0,
            omission_count: 0,
            pending_omission_count: 0,
            actionable_omission_count: 0,
            first_candidate_name: Some(FullName {
                last_name: "Jansen".parse().unwrap(),
                initials: "A.B.".parse().unwrap(),
                ..Default::default()
            }),
            candidate_list_districts: HashMap::new(),
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
            stream_id: StreamId::new(),
            brp: BrpCheckState::NotChecked,
            mode: CsbPhase::Examination,
            is_examination_finished: false,
            is_deleted: false,
            restoration_count: 0,
            omission_count: 0,
            pending_omission_count: 0,
            actionable_omission_count: 0,
            first_candidate_name: None,
            candidate_list_districts: HashMap::new(),
        };

        assert_eq!(group.csb_appellation(), "Blanco");
    }
}
