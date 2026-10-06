use crate::{
    CsbStream, ElectoralDistrict, csb::examination::extractors::CsbPoliticalGroup,
    projection::WithCorrections, structs::persons::Person,
};

/// Everything the unresolved omissions scrapped from one political group,
/// each with the recovery page of the item it was scrapped from.
#[derive(Debug, Default)]
pub struct ScrappedOverview {
    /// The appellation as the group handed it in, with the general
    /// information page it is shown on.
    pub appellation: Option<ScrappedAppellation>,
    /// In the election's district order.
    pub districts: Vec<ScrappedDistrict>,
    /// In the order the group page shows the lists.
    pub lists: Vec<ScrappedList>,
    /// In list order, then list position.
    pub candidates: Vec<ScrappedCandidate>,
}

#[derive(Debug)]
pub struct ScrappedAppellation {
    pub appellation: String,
    pub path: String,
}

#[derive(Debug)]
pub struct ScrappedDistrict {
    pub district: ElectoralDistrict,
    /// The page of the list submitted in this district; `None` when no list
    /// was submitted there.
    pub path: Option<String>,
}

#[derive(Debug)]
pub struct ScrappedList {
    pub districts: Vec<ElectoralDistrict>,
    pub path: String,
}

#[derive(Debug)]
pub struct ScrappedCandidate {
    pub person: Person,
    /// The districts of the list the candidate is scrapped from, which is how
    /// the lists are told apart.
    pub list_districts: Vec<ElectoralDistrict>,
    pub path: String,
}

/// Districts named the way the `district_name` filter does, joined with
/// commas: "1. Groningen, 2. Fryslân".
fn districts_label(districts: &[ElectoralDistrict]) -> String {
    districts
        .iter()
        .map(|district| format!("{}. {}", district.region_number(), district.title()))
        .collect::<Vec<_>>()
        .join(", ")
}

impl ScrappedList {
    pub fn districts_label(&self) -> String {
        districts_label(&self.districts)
    }
}

impl ScrappedCandidate {
    pub fn list_districts_label(&self) -> String {
        districts_label(&self.list_districts)
    }
}

impl ScrappedOverview {
    pub fn is_empty(&self) -> bool {
        // Destructured so that a new field cannot be left out here.
        let Self {
            appellation,
            districts,
            lists,
            candidates,
        } = self;
        appellation.is_none() && districts.is_empty() && lists.is_empty() && candidates.is_empty()
    }
}

impl CsbStream {
    /// What is scrapped from this group, over the corrected lists: those are
    /// the lists the committee examines.
    pub fn get_scrapped_overview(&self, political_group: &CsbPoliticalGroup) -> ScrappedOverview {
        let scrapped = &political_group.scrapped;
        let lists = self.get_candidate_lists_in_page_order(WithCorrections::All);

        let appellation = scrapped
            .is_appellation_scrapped()
            .then(|| ScrappedAppellation {
                appellation: political_group
                    .political_group
                    .appellation
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                path: political_group.general_information_path(),
            });

        let districts = scrapped
            .districts(&self.election)
            .into_iter()
            .map(|district| ScrappedDistrict {
                district,
                path: lists
                    .iter()
                    .find(|list| list.electoral_districts.contains(&district))
                    .map(|list| political_group.candidate_list_path(&list.id)),
            })
            .collect();

        let scrapped_lists = lists
            .iter()
            .filter(|list| scrapped.is_list_scrapped(list.id))
            .map(|list| ScrappedList {
                districts: list.electoral_districts.iter().copied().collect(),
                path: political_group.candidate_list_path(&list.id),
            })
            .collect();

        let candidates = lists
            .iter()
            .flat_map(|list| {
                list.candidates
                    .iter()
                    .filter(|person| scrapped.is_candidate_scrapped(list.id, **person))
                    .filter_map(|person| self.get_person(*person, WithCorrections::All))
                    .map(|person| ScrappedCandidate {
                        path: political_group.candidate_path(&list.id, &person.id),
                        list_districts: list.electoral_districts.iter().copied().collect(),
                        person,
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        ScrappedOverview {
            appellation,
            districts,
            lists: scrapped_lists,
            candidates,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::{
        CsbStore,
        structs::{
            candidate_lists::CandidateListId,
            csb::{CsbPhase, OmissionCategory, OmissionStatus, sample_omission},
            persons::PersonId,
        },
        test_utils::{sample_candidate_list, sample_person, sample_political_group},
    };

    /// A list in `districts` holding one candidate.
    fn add_list(store: &CsbStore, districts: &[ElectoralDistrict]) -> (CandidateListId, PersonId) {
        let person = sample_person(PersonId::new());
        let person_id = person.id;
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.electoral_districts = districts.iter().copied().collect::<BTreeSet<_>>();
        list.candidates = vec![person_id];
        store.add_person(person);
        store.add_candidate_list(list);
        (list_id, person_id)
    }

    async fn irreparable(store: &CsbStore, category: OmissionCategory) {
        let mut omission = sample_omission(category);
        omission.recoverable = false;
        omission.create(store).await.unwrap();
    }

    async fn not_recovered(store: &CsbStore, category: OmissionCategory) {
        let omission = sample_omission(category);
        omission.create(store).await.unwrap();
        omission
            .set_status(store, OmissionStatus::NotRecovered)
            .await
            .unwrap();
    }

    fn overview(store: &CsbStore) -> ScrappedOverview {
        store.get_scrapped_overview(
            &CsbPoliticalGroup::new_from_csb_store(store).with_mode(CsbPhase::Recovery),
        )
    }

    #[tokio::test]
    async fn nothing_unresolved_scraps_nothing() {
        let store = CsbStore::new_for_test();
        store.set_political_group(sample_political_group());
        let (list, _) = add_list(&store, &[ElectoralDistrict::Utrecht]);
        // A pending omission is not decided yet.
        sample_omission(OmissionCategory::CandidateList(vec![list]))
            .create(&store)
            .await
            .unwrap();

        assert!(overview(&store).is_empty());
    }

    #[tokio::test]
    async fn every_kind_of_scrapped_item_links_to_its_own_recovery_page() {
        let store = CsbStore::new_for_test();
        store.set_political_group(sample_political_group());
        let stream_id = store.stream_id;
        let (utrecht, candidate) = add_list(&store, &[ElectoralDistrict::Utrecht]);
        let (groningen, _) = add_list(
            &store,
            &[ElectoralDistrict::Groningen, ElectoralDistrict::Fryslan],
        );

        irreparable(&store, OmissionCategory::Appellation).await;
        not_recovered(
            &store,
            OmissionCategory::DeclarationsOfSupport(vec![ElectoralDistrict::Fryslan]),
        )
        .await;
        not_recovered(&store, OmissionCategory::CandidateList(vec![utrecht])).await;
        not_recovered(
            &store,
            OmissionCategory::Candidate {
                person: candidate,
                lists: vec![utrecht],
            },
        )
        .await;

        let overview = overview(&store);

        let appellation = overview.appellation.expect("the appellation is scrapped");
        assert_eq!(appellation.appellation, "Kiesraad Demo");
        assert_eq!(
            appellation.path,
            format!("/csb/recovery/{stream_id}/general-information")
        );

        assert_eq!(overview.districts.len(), 1);
        assert_eq!(overview.districts[0].district, ElectoralDistrict::Fryslan);
        assert_eq!(
            overview.districts[0].path.as_deref(),
            Some(format!("/csb/recovery/{stream_id}/list/{groningen}").as_str())
        );

        assert_eq!(overview.lists.len(), 1);
        assert_eq!(
            overview.lists[0].districts,
            vec![ElectoralDistrict::Utrecht]
        );
        assert_eq!(
            overview.lists[0].path,
            format!("/csb/recovery/{stream_id}/list/{utrecht}")
        );

        assert_eq!(overview.candidates.len(), 1);
        assert_eq!(overview.candidates[0].person.id, candidate);
        assert_eq!(
            overview.candidates[0].list_districts,
            vec![ElectoralDistrict::Utrecht]
        );
        assert_eq!(
            overview.candidates[0].path,
            format!("/csb/recovery/{stream_id}/list/{utrecht}/candidate/{candidate}")
        );
    }

    #[tokio::test]
    async fn a_district_without_a_list_has_nowhere_to_link_to() {
        let store = CsbStore::new_for_test();
        store.set_political_group(sample_political_group());
        add_list(&store, &[ElectoralDistrict::Utrecht]);
        not_recovered(
            &store,
            OmissionCategory::DeclarationsOfSupport(vec![ElectoralDistrict::Groningen]),
        )
        .await;

        let overview = overview(&store);

        assert_eq!(overview.districts.len(), 1);
        assert!(overview.districts[0].path.is_none());
    }
}
