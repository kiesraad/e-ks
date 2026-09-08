use axum_extra::routing::TypedPath;

use crate::{
    AppError, CsbStream, QueryParamState,
    csb::examination::extractors::CsbPoliticalGroup,
    structs::{
        candidate_lists::CandidateListId,
        csb::{CsbPhase, Omission, OmissionCategory},
        persons::{Person, PersonId},
    },
};

pub struct AllOmissions {
    pub general: Vec<OmissionWithPath>,
    pub declarations_of_support: Vec<OmissionWithPath>,
    pub candidate_lists: Vec<OmissionWithPath>,
    pub candidates: Vec<CandidateOmissions>,
}

pub struct CandidateOmissions {
    pub omissions: Vec<OmissionWithPath>,
    pub person: Person,
}

pub struct OmissionWithPath {
    pub omission: Omission,
    pub path: String,
}

impl CsbStream {
    pub fn get_all_omissions(
        &self,
        political_group: &CsbPoliticalGroup,
    ) -> Result<AllOmissions, AppError> {
        let omissions = self
            .data
            .read()
            .omissions
            .values()
            .cloned()
            .collect::<Vec<_>>();

        let mut general = Vec::new();
        let mut declarations_of_support = Vec::new();
        let mut candidate_lists = Vec::new();
        let mut candidates: Vec<CandidateOmissions> = Vec::new();

        for omission in omissions {
            match omission.category {
                OmissionCategory::PoliticalGroup => general.push(OmissionWithPath {
                    path: general_path(political_group),
                    omission,
                }),
                OmissionCategory::CandidateList(ref lists) => {
                    let list_id = lists.first().ok_or(AppError::InternalServerError)?;
                    candidate_lists.push(OmissionWithPath {
                        path: candidate_list_path(political_group, list_id),
                        omission,
                    })
                }
                OmissionCategory::DeclarationsOfSupport(_) => {
                    declarations_of_support.push(OmissionWithPath {
                        path: declarations_of_support_path(political_group),
                        omission,
                    })
                }
                OmissionCategory::Candidate { person, ref lists } => {
                    let list = lists.first().ok_or(AppError::InternalServerError)?;
                    let with_path = OmissionWithPath {
                        path: candidate_path(political_group, &person, list),
                        omission: omission.clone(),
                    };
                    if let Some(candidate) = candidates.iter_mut().find(|c| c.person.id == person) {
                        candidate.omissions.push(with_path)
                    } else {
                        candidates.push(CandidateOmissions {
                            omissions: vec![with_path],
                            person: self
                                .get_person(person, crate::projection::WithCorrections::All)
                                .ok_or(AppError::InternalServerError)?,
                        });
                    }
                }
            }
        }
        let mut all = AllOmissions {
            general,
            declarations_of_support,
            candidate_lists,
            candidates,
        };
        all.sort_by_district(self);
        Ok(all)
    }
}

impl AllOmissions {
    /// Read the omissions assessed part by part in district order rather than
    /// in store order, so the parts of a split stay together and in place.
    fn sort_by_district(&mut self, store: &CsbStream) {
        self.declarations_of_support
            .sort_by_key(|view| store.district_order(&view.omission));
        self.candidate_lists
            .sort_by_key(|view| store.district_order(&view.omission));

        for candidate in &mut self.candidates {
            candidate
                .omissions
                .sort_by_key(|view| store.district_order(&view.omission));
        }
    }
}

// In examination mode an omission links to the manage-omissions overlay it can
// be edited in; in recovery mode there is nothing to edit, so it links to the
// page of the item it applies to instead.

fn general_path(political_group: &CsbPoliticalGroup) -> String {
    match political_group.mode {
        CsbPhase::Examination => political_group
            .manage_political_group_omissions_path()
            .with_query_params(QueryParamState::redirect_to(
                political_group.all_restorations_path(),
            ))
            .to_string(),
        CsbPhase::Recovery => political_group.general_information_path(),
    }
}

fn candidate_list_path(political_group: &CsbPoliticalGroup, list_id: &CandidateListId) -> String {
    match political_group.mode {
        CsbPhase::Examination => political_group
            .manage_candidate_list_omissions_path(list_id)
            .with_query_params(QueryParamState::redirect_to(
                political_group.all_restorations_path(),
            ))
            .to_string(),
        CsbPhase::Recovery => political_group.candidate_list_path(list_id),
    }
}

fn declarations_of_support_path(political_group: &CsbPoliticalGroup) -> String {
    match political_group.mode {
        CsbPhase::Examination => political_group
            .manage_declarations_of_support_omissions_path()
            .with_query_params(QueryParamState::redirect_to(
                political_group.all_restorations_path(),
            ))
            .to_string(),
        CsbPhase::Recovery => political_group.group_path(),
    }
}

fn candidate_path(
    political_group: &CsbPoliticalGroup,
    person: &PersonId,
    list: &CandidateListId,
) -> String {
    match political_group.mode {
        CsbPhase::Examination => political_group
            .manage_candidate_omissions_path(person, list)
            .with_query_params(QueryParamState::redirect_to(
                political_group.all_restorations_path(),
            ))
            .to_string(),
        CsbPhase::Recovery => political_group.candidate_path(list, person),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        CsbStore, StreamId,
        structs::csb::OmissionType,
        test_utils::{sample_candidate_list, sample_person},
    };

    use super::*;

    fn redirect_param(stream_id: StreamId) -> String {
        format!("&redirect_to=%2Fcsb%2Fexamination%2F{stream_id}%2Fomissions")
    }

    #[test]
    fn general_path_test() {
        let store = CsbStore::new_for_test();

        let path = general_path(&CsbPoliticalGroup::new_from_csb_store(&store));

        let pg_type = OmissionType::PoliticalGroup.to_string();
        let stream_id = store.stream_id;
        let redirect_param = redirect_param(stream_id);
        assert!(
            path.contains(
                format!("/csb/examination/{stream_id}/omission/{pg_type}/{stream_id}/overview?")
                    .as_str()
            )
        );
        assert!(path.contains(redirect_param.as_str()));
    }

    #[test]
    fn candidate_list_omission_path_links_to_the_referenced_list() {
        let store = CsbStore::new_for_test();
        let list_id = CandidateListId::new();
        store.add_candidate_list(sample_candidate_list(list_id));

        let political_group = CsbPoliticalGroup::new_from_csb_store(&store);
        let path = political_group
            .manage_candidate_list_omissions_path(&list_id)
            .with_query_params(QueryParamState::redirect_to(
                political_group.all_restorations_path().to_string(),
            ))
            .to_string();

        let list_type = OmissionType::CandidateList.to_string();
        let stream_id = store.stream_id;
        let redirect_param = redirect_param(stream_id);
        assert!(
            path.contains(
                format!("/csb/examination/{stream_id}/omission/{list_type}/{list_id}/overview?")
                    .as_str()
            )
        );
        assert!(path.contains(redirect_param.as_str()));
    }

    #[test]
    fn candidate_path_test() {
        let store = CsbStore::new_for_test();

        let list_id = CandidateListId::new();
        let list = sample_candidate_list(list_id);
        store.add_candidate_list(list);

        let person_id = PersonId::new();
        store.add_person(sample_person(person_id));

        let path = candidate_path(
            &CsbPoliticalGroup::new_from_csb_store(&store),
            &person_id,
            &list_id,
        );

        let candidate_type = OmissionType::Candidate.to_string();
        let stream_id = store.stream_id;
        let redirect_param = redirect_param(stream_id);
        let list_param = format!("&list={list_id}");
        assert!(
            path.contains(
                format!(
                    "/csb/examination/{stream_id}/omission/{candidate_type}/{person_id}/overview?"
                )
                .as_str()
            )
        );
        assert!(path.contains(list_param.as_str()));
        assert!(path.contains(redirect_param.as_str()));
    }

    #[test]
    fn person_without_omissions() {
        let store = CsbStore::new_for_test();

        store.add_person(sample_person(PersonId::new()));

        let all_omissions = store
            .get_all_omissions(&CsbPoliticalGroup::new_from_csb_store(&store))
            .expect("Couldn't retrieve all omissions");

        assert!(all_omissions.candidates.is_empty());
    }

    #[tokio::test]
    async fn person_with_omission() {
        let store = CsbStore::new_for_test();

        let person_id = PersonId::new();
        store.add_person(sample_person(person_id));

        let list_id = CandidateListId::new();
        store.add_candidate_list(sample_candidate_list(list_id));

        Omission::new(
            OmissionCategory::Candidate {
                person: person_id,
                lists: vec![list_id],
            },
            "title".parse().unwrap(),
            "description".parse().unwrap(),
            "help_text".parse().ok(),
        )
        .create(&store)
        .await
        .expect("Couldn't create omission");

        let all_omissions = store
            .get_all_omissions(&CsbPoliticalGroup::new_from_csb_store(&store))
            .expect("Couldn't retrieve all omissions");

        assert_eq!(all_omissions.candidates.len(), 1);
        assert_eq!(all_omissions.candidates[0].omissions.len(), 1)
    }

    #[tokio::test]
    async fn person_with_multiple_omissions() {
        let omission_count = 10;
        let store = CsbStore::new_for_test();

        let person_id = PersonId::new();
        store.add_person(sample_person(person_id));

        let list_id = CandidateListId::new();
        store.add_candidate_list(sample_candidate_list(list_id));
        for _ in 0..omission_count {
            Omission::new(
                OmissionCategory::Candidate {
                    person: person_id,
                    lists: vec![list_id],
                },
                "title".parse().unwrap(),
                "description".parse().unwrap(),
                "help_text".parse().ok(),
            )
            .create(&store)
            .await
            .expect("Couldn't create omission");
        }

        let all_omissions = store
            .get_all_omissions(&CsbPoliticalGroup::new_from_csb_store(&store))
            .expect("Couldn't retrieve all omissions");

        // creates one candidate with 10 omissions
        assert_eq!(all_omissions.candidates.len(), 1);
        assert_eq!(all_omissions.candidates[0].omissions.len(), omission_count)
    }
}
