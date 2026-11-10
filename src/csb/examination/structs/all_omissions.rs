use axum_extra::routing::TypedPath;

use crate::{
    AppError, CsbStoreData, QueryParamState,
    csb::examination::{extractors::CsbPoliticalGroup, structs::OmissionRow},
    projection::WithCorrections,
    structs::{
        candidate_lists::CandidateListId,
        csb::{CsbPhase, Omission, OmissionCategory},
        persons::{Person, PersonId},
    },
};

pub struct AllOmissions {
    pub general: Vec<OmissionRow>,
    pub declarations_of_support: Vec<OmissionRow>,
    pub candidate_lists: Vec<OmissionRow>,
    pub candidates: Vec<CandidateOmissions>,
}

pub struct CandidateOmissions {
    pub omissions: Vec<OmissionRow>,
    pub person: Person,
    /// No longer on the corrected list; `person` is the imported data.
    pub removed: bool,
}

impl CsbStoreData {
    pub fn get_all_omissions(
        &self,
        political_group: &CsbPoliticalGroup,
    ) -> Result<AllOmissions, AppError> {
        let election = political_group.election;

        let mut general = Vec::new();
        let mut declarations_of_support = Vec::new();
        let mut candidate_lists = Vec::new();
        let mut candidates: Vec<(PersonId, Vec<Omission>)> = Vec::new();

        for omission in self.get_omissions() {
            match omission.category {
                OmissionCategory::Appellation => general.push(omission),
                OmissionCategory::CandidateList(ref lists) => {
                    lists.first().ok_or(AppError::InternalServerError)?;
                    candidate_lists.push(omission)
                }
                OmissionCategory::DeclarationsOfSupport(_) => {
                    declarations_of_support.push(omission)
                }
                OmissionCategory::Candidate { person, ref lists } => {
                    lists.first().ok_or(AppError::InternalServerError)?;
                    match candidates.iter_mut().find(|(id, _)| *id == person) {
                        Some((_, omissions)) => omissions.push(omission),
                        None => candidates.push((person, vec![omission])),
                    }
                }
            }
        }

        // Title order, then district order, so the parts of a split stay together.
        declarations_of_support.sort_by_cached_key(|omission| self.title_order(omission, election));
        candidate_lists.sort_by_cached_key(|omission| self.title_order(omission, election));

        let candidates = self.candidate_omissions(political_group, candidates)?;

        Ok(AllOmissions {
            general: OmissionRow::rows(general, political_group, |_| {
                appellation_path(political_group)
            }),
            declarations_of_support: OmissionRow::rows(
                declarations_of_support,
                political_group,
                |_| declarations_of_support_path(political_group),
            ),
            candidate_lists: OmissionRow::rows(candidate_lists, political_group, |omission| {
                omission
                    .candidate_lists()
                    .first()
                    .and_then(|list| candidate_list_path(political_group, list))
            }),
            candidates,
        })
    }

    /// The rows of each candidate's omissions, in title order.
    fn candidate_omissions(
        &self,
        political_group: &CsbPoliticalGroup,
        candidates: Vec<(PersonId, Vec<Omission>)>,
    ) -> Result<Vec<CandidateOmissions>, AppError> {
        let election = political_group.election;
        candidates
            .into_iter()
            .map(|(person_id, mut omissions)| {
                omissions.sort_by_cached_key(|omission| self.title_order(omission, election));
                // a candidate deleted on paper keeps their omissions, shown from the imported data
                let (person, removed) =
                    match self.pg(WithCorrections::All).get_person(person_id).ok() {
                        Some(current) => (current.clone(), false),
                        None => (
                            self.pg(WithCorrections::None)
                                .get_person(person_id)
                                .map_err(|_| AppError::InternalServerError)?
                                .clone(),
                            true,
                        ),
                    };
                Ok(CandidateOmissions {
                    omissions: OmissionRow::rows(omissions, political_group, |omission| {
                        omission
                            .candidate_lists()
                            .first()
                            .and_then(|list| candidate_path(political_group, &person_id, list))
                    }),
                    person,
                    removed,
                })
            })
            .collect::<Result<Vec<_>, AppError>>()
    }
}

impl AllOmissions {
    /// Split into the recoverable and the irrecoverable omissions, in that
    /// order. A candidate with both appears in both.
    pub fn split_on_recoverability(self) -> (Self, Self) {
        let (g_recoverable, g_irrecoverable) = split_rows(self.general);
        let (d_recoverable, d_irrecoverable) = split_rows(self.declarations_of_support);
        let (cl_recoverable, cl_irrecoverable) = split_rows(self.candidate_lists);
        let mut c_recoverable = Vec::new();
        let mut c_irrecoverable = Vec::new();
        for candidate in self.candidates {
            let (o_recoverable, o_irrecoverable) = split_rows(candidate.omissions);
            if !o_recoverable.is_empty() {
                c_recoverable.push(CandidateOmissions {
                    omissions: o_recoverable,
                    person: candidate.person.clone(),
                    removed: candidate.removed,
                });
            }
            if !o_irrecoverable.is_empty() {
                c_irrecoverable.push(CandidateOmissions {
                    omissions: o_irrecoverable,
                    person: candidate.person,
                    removed: candidate.removed,
                });
            }
        }
        (
            AllOmissions {
                general: g_recoverable,
                declarations_of_support: d_recoverable,
                candidate_lists: cl_recoverable,
                candidates: c_recoverable,
            },
            AllOmissions {
                general: g_irrecoverable,
                declarations_of_support: d_irrecoverable,
                candidate_lists: cl_irrecoverable,
                candidates: c_irrecoverable,
            },
        )
    }

    pub fn is_empty(&self) -> bool {
        self.general.is_empty()
            && self.declarations_of_support.is_empty()
            && self.candidate_lists.is_empty()
            && self.candidates.is_empty()
    }
}

/// `rows` split into the recoverable and the irrecoverable ones, in that order.
fn split_rows(rows: Vec<OmissionRow>) -> (Vec<OmissionRow>, Vec<OmissionRow>) {
    rows.into_iter().partition(|row| row.omission.recoverable)
}

// In examination mode an omission links to the manage-omissions overlay it can
// be edited in; in recovery mode there is nothing to edit, so it links to the
// page of the item it applies to instead.

fn appellation_path(political_group: &CsbPoliticalGroup) -> Option<String> {
    Some(match political_group.mode {
        CsbPhase::Examination => political_group
            .manage_appellation_omissions_path()
            .with_query_params(QueryParamState::redirect_to(
                political_group.all_restorations_path(),
            ))
            .to_string(),
        CsbPhase::Recovery => political_group.general_information_path(),
    })
}

fn candidate_list_path(
    political_group: &CsbPoliticalGroup,
    list_id: &CandidateListId,
) -> Option<String> {
    Some(match political_group.mode {
        CsbPhase::Examination => political_group
            .manage_candidate_list_omissions_path(list_id)
            .with_query_params(QueryParamState::redirect_to(
                political_group.all_restorations_path(),
            ))
            .to_string(),
        CsbPhase::Recovery => political_group.candidate_list_path(list_id),
    })
}

/// The declarations of support are decided on the recovery omissions page
/// itself, so there the title links nowhere.
fn declarations_of_support_path(political_group: &CsbPoliticalGroup) -> Option<String> {
    match political_group.mode {
        CsbPhase::Examination => Some(
            political_group
                .manage_declarations_of_support_omissions_path()
                .with_query_params(QueryParamState::redirect_to(
                    political_group.all_restorations_path(),
                ))
                .to_string(),
        ),
        CsbPhase::Recovery => None,
    }
}

fn candidate_path(
    political_group: &CsbPoliticalGroup,
    person: &PersonId,
    list: &CandidateListId,
) -> Option<String> {
    Some(match political_group.mode {
        CsbPhase::Examination => political_group
            .manage_candidate_omissions_path(person, list)
            .with_query_params(QueryParamState::redirect_to(
                political_group.all_restorations_path(),
            ))
            .to_string(),
        CsbPhase::Recovery => political_group.candidate_path(list, person),
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        CsbStore, StreamId,
        structs::csb::{OmissionType, sample_omission},
        test_utils::{sample_candidate_list, sample_person},
    };

    use super::*;

    fn redirect_param(stream_id: StreamId) -> String {
        format!("&redirect_to=%2Fcsb%2Fexamination%2F{stream_id}%2Fomissions")
    }

    #[test]
    fn general_path_test() {
        let store = CsbStore::new_for_test();

        let path = general_path(&CsbPoliticalGroup::new_from_csb_store(&store)).unwrap();

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
        )
        .unwrap();

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

        let data = store.snapshot();
        let all_omissions = data
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

        let data = store.snapshot();
        let all_omissions = data
            .get_all_omissions(&CsbPoliticalGroup::new_from_csb_store(&store))
            .expect("Couldn't retrieve all omissions");

        assert_eq!(all_omissions.candidates.len(), 1);
        assert_eq!(all_omissions.candidates[0].omissions.len(), 1)
    }

    /// A candidate deleted on paper keeps their omissions in the overview,
    /// shown from the imported data, instead of failing the page.
    #[tokio::test]
    async fn omissions_of_a_candidate_deleted_on_paper_are_kept() {
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
            None,
        )
        .create(&store)
        .await
        .expect("Couldn't create omission");

        store
            .update(crate::CsbAction::PaperCorrectedUpdate(Box::new(
                crate::PgEvent::DeletePerson { person_id },
            )))
            .await
            .expect("Couldn't delete the candidate");

        let data = store.snapshot();
        let all_omissions = data
            .get_all_omissions(&CsbPoliticalGroup::new_from_csb_store(&store))
            .expect("Couldn't retrieve all omissions");

        assert_eq!(all_omissions.candidates.len(), 1);
        assert!(all_omissions.candidates[0].removed);
        assert_eq!(all_omissions.candidates[0].person.id, person_id);
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

        let data = store.snapshot();
        let all_omissions = data
            .get_all_omissions(&CsbPoliticalGroup::new_from_csb_store(&store))
            .expect("Couldn't retrieve all omissions");

        // creates one candidate with 10 omissions
        assert_eq!(all_omissions.candidates.len(), 1);
        assert_eq!(all_omissions.candidates[0].omissions.len(), omission_count)
    }

    fn sample_omissions_with_paths(data: Vec<(&str, bool)>) -> Vec<OmissionRow> {
        let mut omissions = Vec::new();
        for (path, recoverable) in data {
            let mut omission = sample_omission(OmissionCategory::DeclarationsOfSupport(Vec::new()));
            omission.recoverable = recoverable;
            omissions.push(OmissionRow {
                omission,
                path: Some(path.to_string()),
                parts: Vec::new(),
            });
        }
        omissions
    }

    #[test]
    // splitting up the test in subroutines impedes readability
    #[allow(clippy::cognitive_complexity)]
    #[allow(clippy::too_many_lines)]
    fn split_on_recoverability_splits_correctly() {
        let sample_people = [
            sample_person(PersonId::new()), // 1 recoverable, 1 irrecoverable
            sample_person(PersonId::new()), // 1 recoverable, 0 irrecoverable
            sample_person(PersonId::new()), // 0 recoverable, 1 irrecoverable
        ];
        let omissions = AllOmissions {
            general: sample_omissions_with_paths(vec![("/rec/gen", true), ("/irr/gen", false)]),
            declarations_of_support: sample_omissions_with_paths(vec![
                ("/irr/dec", false),
                ("/rec/dec", true),
            ]),
            candidate_lists: sample_omissions_with_paths(vec![
                ("/rec/cl", true),
                ("/irr/cl", false),
            ]),
            candidates: vec![
                CandidateOmissions {
                    omissions: sample_omissions_with_paths(vec![
                        ("/irr/c0", false),
                        ("/rec/c0", true),
                    ]),
                    person: sample_people[0].clone(),
                    removed: false,
                },
                CandidateOmissions {
                    omissions: sample_omissions_with_paths(vec![("/rec/c1", true)]),
                    person: sample_people[1].clone(),
                    removed: false,
                },
                CandidateOmissions {
                    omissions: sample_omissions_with_paths(vec![("/irr/c2", false)]),
                    person: sample_people[2].clone(),
                    removed: false,
                },
            ],
        };

        let (recoverable, irrecoverable) = omissions.split_on_recoverability();

        assert_eq!(recoverable.general.len(), 1);
        assert_eq!(recoverable.general[0].path.as_deref(), Some("/rec/gen"));
        assert_eq!(irrecoverable.general.len(), 1);
        assert_eq!(irrecoverable.general[0].path.as_deref(), Some("/irr/gen"));

        assert_eq!(recoverable.declarations_of_support.len(), 1);
        assert_eq!(
            recoverable.declarations_of_support[0].path.as_deref(),
            Some("/rec/dec")
        );
        assert_eq!(irrecoverable.declarations_of_support.len(), 1);
        assert_eq!(
            irrecoverable.declarations_of_support[0].path.as_deref(),
            Some("/irr/dec")
        );

        assert_eq!(recoverable.candidate_lists.len(), 1);
        assert_eq!(
            recoverable.candidate_lists[0].path.as_deref(),
            Some("/rec/cl")
        );
        assert_eq!(irrecoverable.candidate_lists.len(), 1);
        assert_eq!(
            irrecoverable.candidate_lists[0].path.as_deref(),
            Some("/irr/cl")
        );

        assert_eq!(recoverable.candidates.len(), 2);
        let c0 = recoverable
            .candidates
            .iter()
            .find(|c| c.person.id == sample_people[0].id)
            .expect("person 0 should be in the recoverable set");
        assert_eq!(c0.omissions.len(), 1);
        assert_eq!(c0.omissions[0].path.as_deref(), Some("/rec/c0"));
        let c1 = recoverable
            .candidates
            .iter()
            .find(|c| c.person.id == sample_people[1].id)
            .expect("person 1 should be in the recoverable set");
        assert_eq!(c1.omissions.len(), 1);
        assert_eq!(c1.omissions[0].path.as_deref(), Some("/rec/c1"));

        assert_eq!(irrecoverable.candidates.len(), 2);
        let c0 = irrecoverable
            .candidates
            .iter()
            .find(|c| c.person.id == sample_people[0].id)
            .expect("person 0 should be in the irrecoverable set");
        assert_eq!(c0.omissions.len(), 1);
        assert_eq!(c0.omissions[0].path.as_deref(), Some("/irr/c0"));
        let c2 = irrecoverable
            .candidates
            .iter()
            .find(|c| c.person.id == sample_people[2].id)
            .expect("person 2 should be in the recoverable set");
        assert_eq!(c2.omissions.len(), 1);
        assert_eq!(c2.omissions[0].path.as_deref(), Some("/irr/c2"));
    }

    #[test]
    /// test for regression on the [AllOmission] fields such that [AllOmissions::is_empty] keeps
    /// checking all fields. Fixing this tests means [AllOmissions::is_empty] needs fixing too.
    fn empty_all_omissions_yields_true_on_is_empty() {
        let omissions = AllOmissions {
            general: Vec::new(),
            declarations_of_support: Vec::new(),
            candidate_lists: Vec::new(),
            candidates: Vec::new(),
        };

        assert!(omissions.is_empty());
    }
}
