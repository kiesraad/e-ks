use std::collections::HashSet;

use super::scrapped_overview::districts_label;
use crate::{
    CsbStream, ElectoralDistrict,
    csb::examination::extractors::CsbPoliticalGroup,
    projection::WithCorrections,
    structs::{candidate_lists::CandidateList, persons::Person},
};

/// What paper corrections added to or removed from the group's lists, found
/// by comparing the corrected lists with the imported ones.
#[derive(Debug, Default)]
pub struct PaperCorrectionsOverview {
    /// Lists first, then districts, then candidates; each in page order.
    pub items: Vec<PaperCorrection>,
    /// Whether paper corrections also changed what the items do not show,
    /// like a candidate's initials or the submitter's address.
    pub has_other_corrections: bool,
}

#[derive(Debug)]
pub struct PaperCorrection {
    pub kind: PaperCorrectionKind,
    /// What was added or removed: a list's districts, a district or a
    /// candidate's name. `None` when there is nothing to tell apart.
    pub detail: Option<String>,
    /// The page to look at; `None` once the page is gone with the list.
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaperCorrectionKind {
    ListAdded,
    ListDeleted,
    DistrictAdded,
    DistrictRemoved,
    CandidateAdded,
    CandidateRemoved,
}

impl PaperCorrectionKind {
    /// The translation key of the pill's label.
    pub fn label_key(&self) -> &'static str {
        match self {
            Self::ListAdded => "csb.paper_corrections.list_added",
            Self::ListDeleted => "csb.paper_corrections.list_deleted",
            Self::DistrictAdded => "csb.paper_corrections.district_added",
            Self::DistrictRemoved => "csb.paper_corrections.district_removed",
            Self::CandidateAdded => "csb.paper_corrections.candidate_added",
            Self::CandidateRemoved => "csb.paper_corrections.candidate_removed",
        }
    }
}

impl PaperCorrectionsOverview {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && !self.has_other_corrections
    }
}

/// A list named by its districts: "1. Groningen, 2. Fryslân".
fn list_label(list: &CandidateList) -> String {
    districts_label(&list.electoral_districts.iter().copied().collect::<Vec<_>>())
}

fn district_label(district: &ElectoralDistrict) -> String {
    districts_label(std::slice::from_ref(district))
}

/// The imported and the corrected lists side by side.
struct ListDiff<'a> {
    store: &'a CsbStream,
    political_group: &'a CsbPoliticalGroup,
    imported: Vec<CandidateList>,
    corrected: Vec<CandidateList>,
    /// Lists are told apart by their districts, so with a single district or
    /// a single list there is no list to name.
    name_lists: bool,
    name_candidates_list: bool,
}

impl ListDiff<'_> {
    fn imported_version(&self, list: &CandidateList) -> Option<&CandidateList> {
        self.imported.iter().find(|before| before.id == list.id)
    }

    fn list_detail(&self, list: &CandidateList) -> Option<String> {
        self.name_lists.then(|| list_label(list))
    }

    /// The lists present in only one of the projections.
    fn list_items(&self) -> Vec<PaperCorrection> {
        let added = self
            .corrected
            .iter()
            .filter(|list| self.imported_version(list).is_none())
            .map(|list| PaperCorrection {
                kind: PaperCorrectionKind::ListAdded,
                detail: self.list_detail(list),
                path: Some(self.political_group.candidate_list_path(&list.id)),
            });
        let deleted = self
            .imported
            .iter()
            .filter(|before| !self.corrected.iter().any(|list| list.id == before.id))
            .map(|list| PaperCorrection {
                kind: PaperCorrectionKind::ListDeleted,
                detail: self.list_detail(list),
                path: None,
            });
        added.chain(deleted).collect()
    }

    /// The districts that moved on or off a list present in both projections.
    /// A district only ever moves within one group, so its list is found
    /// through the link.
    fn district_items(&self) -> Vec<PaperCorrection> {
        let mut items = Vec::new();
        for list in &self.corrected {
            let Some(before) = self.imported_version(list) else {
                continue;
            };
            let path = self.political_group.candidate_list_path(&list.id);
            let added = list
                .electoral_districts
                .difference(&before.electoral_districts)
                .map(|district| (PaperCorrectionKind::DistrictAdded, district));
            let removed = before
                .electoral_districts
                .difference(&list.electoral_districts)
                .map(|district| (PaperCorrectionKind::DistrictRemoved, district));
            items.extend(
                added
                    .chain(removed)
                    .map(|(kind, district)| PaperCorrection {
                        kind,
                        detail: Some(district_label(district)),
                        path: Some(path.clone()),
                    }),
            );
        }
        items
    }

    /// The candidates that joined or left a list present in both projections.
    fn candidate_items(&self) -> Vec<PaperCorrection> {
        let mut items = Vec::new();
        for list in &self.corrected {
            let Some(before) = self.imported_version(list) else {
                continue;
            };
            for person_id in &list.candidates {
                if before.candidates.contains(person_id) {
                    continue;
                }
                let Some(person) = self.store.get_person(*person_id, WithCorrections::All) else {
                    continue;
                };
                items.push(PaperCorrection {
                    kind: PaperCorrectionKind::CandidateAdded,
                    detail: Some(self.candidate_detail(&person, list)),
                    path: Some(self.political_group.candidate_path(&list.id, person_id)),
                });
            }
            // A removed candidate may be deleted as a person as well, so the
            // name comes from the import.
            for person_id in &before.candidates {
                if list.candidates.contains(person_id) {
                    continue;
                }
                let Some(person) = self.store.get_person(*person_id, WithCorrections::None) else {
                    continue;
                };
                items.push(PaperCorrection {
                    kind: PaperCorrectionKind::CandidateRemoved,
                    detail: Some(self.candidate_detail(&person, list)),
                    path: Some(self.political_group.candidate_list_path(&list.id)),
                });
            }
        }
        items
    }

    fn candidate_detail(&self, person: &Person, list: &CandidateList) -> String {
        if self.name_candidates_list {
            format!("{} ({})", person.name.display(), list_label(list))
        } else {
            person.name.display()
        }
    }
}

impl CsbStream {
    /// The paper corrections to the group's lists, as the group page shows
    /// them.
    pub fn get_paper_corrections_overview(
        &self,
        political_group: &CsbPoliticalGroup,
    ) -> PaperCorrectionsOverview {
        let imported = self.get_candidate_lists_in_page_order(WithCorrections::None);
        let corrected = self.get_candidate_lists_in_page_order(WithCorrections::All);
        let list_count = imported
            .iter()
            .chain(&corrected)
            .map(|list| list.id)
            .collect::<HashSet<_>>()
            .len();
        let diff = ListDiff {
            store: self,
            political_group,
            name_lists: !self.election.has_only_one_district(),
            name_candidates_list: list_count > 1,
            imported,
            corrected,
        };

        let mut items = diff.list_items();
        items.extend(diff.district_items());
        items.extend(diff.candidate_items());

        // Corrections that cancel out, like a list added and deleted again,
        // are still corrections.
        let has_other_corrections =
            self.has_paper_corrected_values() || (items.is_empty() && self.has_paper_corrections());

        PaperCorrectionsOverview {
            items,
            has_other_corrections,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::{
        CsbAction, CsbStore, PgEvent,
        structs::{candidate_lists::CandidateListId, persons::PersonId},
        test_utils::{sample_candidate_list, sample_person},
    };

    fn overview(store: &CsbStore) -> PaperCorrectionsOverview {
        store.get_paper_corrections_overview(&CsbPoliticalGroup::new_from_csb_store(store))
    }

    fn kinds(overview: &PaperCorrectionsOverview) -> Vec<PaperCorrectionKind> {
        overview.items.iter().map(|item| item.kind).collect()
    }

    /// A list in `districts` holding `candidates`, present in both projections.
    fn add_list(
        store: &CsbStore,
        districts: &[ElectoralDistrict],
        candidates: &[PersonId],
    ) -> CandidateList {
        let mut list = sample_candidate_list(CandidateListId::new());
        list.electoral_districts = districts.iter().copied().collect::<BTreeSet<_>>();
        list.candidates = candidates.to_vec();
        store.add_candidate_list(list.clone());
        list
    }

    fn new_person(store: &CsbStore, last_name: &str) -> PersonId {
        let mut person = sample_person(PersonId::new());
        person.name.last_name = last_name.parse().unwrap();
        let id = person.id;
        store.add_person(person);
        id
    }

    #[test]
    fn an_untouched_group_has_no_paper_corrections() {
        let store = CsbStore::new_for_test();
        let person = new_person(&store, "Jansen");
        add_list(&store, &[ElectoralDistrict::Utrecht], &[person]);

        let overview = overview(&store);

        assert!(overview.is_empty());
        assert!(overview.items.is_empty());
        assert!(!overview.has_other_corrections);
    }

    #[test]
    fn added_and_deleted_lists_are_named_by_their_districts() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        add_list(&store, &[ElectoralDistrict::Utrecht], &[]);
        // Deleted on paper: imported only.
        let deleted_id = CandidateListId::new();
        let mut deleted = sample_candidate_list(deleted_id);
        deleted.electoral_districts = BTreeSet::from([ElectoralDistrict::Groningen]);
        store.set_imported_candidate_list(deleted);
        // Added on paper: corrected only.
        let added_id = CandidateListId::new();
        let mut added = sample_candidate_list(added_id);
        added.electoral_districts =
            BTreeSet::from([ElectoralDistrict::Drenthe, ElectoralDistrict::Overijssel]);
        store.set_paper_corrected_candidate_list(added);

        let overview = overview(&store);

        assert_eq!(
            kinds(&overview),
            [
                PaperCorrectionKind::ListAdded,
                PaperCorrectionKind::ListDeleted
            ]
        );
        let [added, deleted] = overview.items.as_slice() else {
            panic!("two items");
        };
        assert_eq!(added.detail.as_deref(), Some("3. Drenthe, 4. Overijssel"));
        assert_eq!(
            added.path.as_deref(),
            Some(format!("/csb/examination/{stream_id}/list/{added_id}").as_str())
        );
        assert_eq!(deleted.detail.as_deref(), Some("1. Groningen"));
        assert_eq!(deleted.path, None);
        assert!(!overview.has_other_corrections);
    }

    #[test]
    fn districts_moved_on_paper_are_listed_per_district() {
        let store = CsbStore::new_for_test();
        let list = add_list(
            &store,
            &[ElectoralDistrict::Utrecht, ElectoralDistrict::Groningen],
            &[],
        );
        let mut corrected = list.clone();
        corrected.electoral_districts =
            BTreeSet::from([ElectoralDistrict::Utrecht, ElectoralDistrict::Drenthe]);
        store.set_paper_corrected_candidate_list(corrected);

        let overview = overview(&store);

        assert_eq!(
            kinds(&overview),
            [
                PaperCorrectionKind::DistrictAdded,
                PaperCorrectionKind::DistrictRemoved
            ]
        );
        assert_eq!(overview.items[0].detail.as_deref(), Some("3. Drenthe"));
        assert_eq!(overview.items[1].detail.as_deref(), Some("1. Groningen"));
        assert!(overview.items.iter().all(|item| item.path.is_some()));
    }

    #[test]
    fn added_and_removed_candidates_are_named() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;
        let stays = new_person(&store, "Blijft");
        let removed = new_person(&store, "Verwijderd");
        let added = new_person(&store, "Toegevoegd");
        let list = add_list(&store, &[ElectoralDistrict::Utrecht], &[stays, removed]);
        let mut corrected = list.clone();
        corrected.candidates = vec![stays, added];
        store.set_paper_corrected_candidate_list(corrected);
        // The removed candidate was deleted as a person too.
        store.remove_paper_corrected_person(removed);

        let overview = overview(&store);

        assert_eq!(
            kinds(&overview),
            [
                PaperCorrectionKind::CandidateAdded,
                PaperCorrectionKind::CandidateRemoved
            ]
        );
        let [added_item, removed_item] = overview.items.as_slice() else {
            panic!("two items");
        };
        // A single list is not named.
        assert_eq!(
            added_item.detail.as_deref(),
            Some("Toegevoegd, H.A.H.A. (Henk)")
        );
        assert_eq!(
            added_item.path.as_deref(),
            Some(
                format!(
                    "/csb/examination/{stream_id}/list/{}/candidate/{added}",
                    list.id
                )
                .as_str()
            )
        );
        assert_eq!(
            removed_item.detail.as_deref(),
            Some("Verwijderd, H.A.H.A. (Henk)")
        );
        assert_eq!(
            removed_item.path.as_deref(),
            Some(format!("/csb/examination/{stream_id}/list/{}", list.id).as_str())
        );
    }

    #[test]
    fn candidates_name_their_list_when_there_is_more_than_one() {
        let store = CsbStore::new_for_test();
        let added = new_person(&store, "Toegevoegd");
        add_list(&store, &[ElectoralDistrict::Groningen], &[]);
        let list = add_list(&store, &[ElectoralDistrict::Utrecht], &[]);
        let mut corrected = list;
        corrected.candidates = vec![added];
        store.set_paper_corrected_candidate_list(corrected);

        let overview = overview(&store);

        assert_eq!(
            overview.items[0].detail.as_deref(),
            Some("Toegevoegd, H.A.H.A. (Henk) (7. Utrecht)")
        );
    }

    #[tokio::test]
    async fn a_corrected_value_counts_as_another_correction() {
        let store = CsbStore::new_for_test();
        let mut person = sample_person(PersonId::new());
        store.add_person(person.clone());
        add_list(&store, &[ElectoralDistrict::Utrecht], &[person.id]);
        person.name.initials = Some("A.B.".parse().unwrap());
        store
            .update(CsbAction::PaperCorrectedUpdate(Box::new(
                PgEvent::UpdatePerson(Box::new(person)),
            )))
            .await
            .unwrap();

        let overview = overview(&store);

        assert!(overview.items.is_empty());
        assert!(overview.has_other_corrections);
        assert!(!overview.is_empty());
    }

    #[tokio::test]
    async fn a_list_added_and_deleted_again_still_counts() {
        let store = CsbStore::new_for_test();
        let list = sample_candidate_list(CandidateListId::new());
        for event in [
            PgEvent::CreateCandidateList(Box::new(list.clone())),
            PgEvent::DeleteCandidateList(list.id),
        ] {
            store
                .update(CsbAction::PaperCorrectedUpdate(Box::new(event)))
                .await
                .unwrap();
        }

        let overview = overview(&store);

        assert!(overview.items.is_empty());
        assert!(overview.has_other_corrections);
    }

    #[tokio::test]
    async fn adding_a_list_is_not_another_correction() {
        let store = CsbStore::new_for_test();
        let list = sample_candidate_list(CandidateListId::new());
        store
            .update(CsbAction::PaperCorrectedUpdate(Box::new(
                PgEvent::CreateCandidateList(Box::new(list)),
            )))
            .await
            .unwrap();

        let overview = overview(&store);

        assert_eq!(kinds(&overview), [PaperCorrectionKind::ListAdded]);
        assert!(!overview.has_other_corrections);
    }
}
