//! Drops omissions and committee corrections about data a paper correction
//! deleted or changed.

use super::CsbStoreData;
use crate::{
    PgStoreData,
    structs::{
        common::UtcDateTime,
        csb::{Omission, OmissionCategory},
    },
};

impl CsbStoreData {
    /// Narrow every omission to what the paper corrected data still holds.
    pub(super) fn drop_dangling_omissions(&mut self, event_time: UtcDateTime) {
        let data = &self.paper_corrected_data;
        self.omissions
            .retain(|_, omission| narrow(omission, data, event_time));
    }

    /// Drop the committee's corrections that no longer change anything.
    pub(super) fn drop_moot_corrections(&mut self) {
        let data = &self.paper_corrected_data;
        let group = &data.political_group;

        if group.is_blank() || group.appellation == self.csb_corrected_appellation {
            self.csb_corrected_appellation = None;
        }
        self.csb_corrected_persons.retain(|person_id, delta| {
            data.persons
                .get(person_id)
                .is_some_and(|person| delta.retain_changes(person))
        });
    }
}

/// `false` when nothing the omission is about is left.
fn narrow(omission: &mut Omission, data: &PgStoreData, event_time: UtcDateTime) -> bool {
    let parts = |category: &OmissionCategory| match category {
        OmissionCategory::Appellation => 1,
        OmissionCategory::CandidateList(lists) | OmissionCategory::Candidate { lists, .. } => {
            lists.len()
        }
        OmissionCategory::DeclarationsOfSupport(districts) => districts.len(),
    };
    let before = parts(&omission.category);

    let kept = match &mut omission.category {
        // A blank list has no appellation.
        OmissionCategory::Appellation => !data.political_group.is_blank(),
        OmissionCategory::CandidateList(lists) => {
            lists.retain(|list_id| data.candidate_lists.contains_key(list_id));
            !lists.is_empty()
        }
        OmissionCategory::Candidate { person, lists } => {
            lists.retain(|list_id| {
                data.candidate_lists
                    .get(list_id)
                    .is_some_and(|list| list.candidates.contains(person))
            });
            !lists.is_empty() && data.persons.contains_key(person)
        }
        // Legacy: no districts means all of them.
        OmissionCategory::DeclarationsOfSupport(districts) if districts.is_empty() => true,
        OmissionCategory::DeclarationsOfSupport(districts) => {
            districts.retain(|district| {
                data.candidate_lists
                    .values()
                    .any(|list| list.electoral_districts.contains(district))
            });
            !districts.is_empty()
        }
    };

    if kept && parts(&omission.category) != before {
        omission.updated_at = event_time;
    }
    kept
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::{
        CsbAction, CsbUser, ElectoralDistrict, PgEvent, StreamId,
        projection::CandidateImport,
        structs::{
            candidate_lists::{CandidateList, CandidateListId},
            common::{Appellation, LastName},
            csb::{Correction, OmissionStatus, PersonCorrection, sample_omission},
            list_designation::ListDesignation,
            persons::PersonId,
        },
        test_utils::{sample_person, sample_political_group},
    };

    /// List `a` holds `first` and `second`, list `b` only `first`.
    struct Imported {
        a: CandidateListId,
        b: CandidateListId,
        first: PersonId,
        second: PersonId,
    }

    impl Imported {
        fn new() -> Self {
            Self {
                a: CandidateListId::new(),
                b: CandidateListId::new(),
                first: PersonId::new(),
                second: PersonId::new(),
            }
        }

        fn import(&self) -> CsbAction {
            let list = |id, electoral_districts, candidates| {
                PgEvent::CreateCandidateList(Box::new(CandidateList {
                    id,
                    electoral_districts,
                    candidates,
                    ..Default::default()
                }))
            };
            CsbAction::Import {
                hash: [42; 32],
                source_stream_id: StreamId::new(),
                snapshot: Box::new(PgStoreData::from_events([
                    PgEvent::UpdatePoliticalGroup(sample_political_group()),
                    PgEvent::CreatePerson(Box::new(sample_person(self.first))),
                    PgEvent::CreatePerson(Box::new(sample_person(self.second))),
                    list(
                        self.a,
                        [ElectoralDistrict::Groningen, ElectoralDistrict::Fryslan].into(),
                        vec![self.first, self.second],
                    ),
                    list(
                        self.b,
                        [ElectoralDistrict::Utrecht].into(),
                        vec![self.first],
                    ),
                ])),
            }
        }

        fn candidate(&self, person: PersonId, lists: Vec<CandidateListId>) -> Omission {
            sample_omission(OmissionCategory::Candidate { person, lists })
        }

        fn replay(&self, actions: Vec<CsbAction>, on_paper: Vec<PgEvent>) -> CsbStoreData {
            let on_paper = on_paper
                .into_iter()
                .map(|event| CsbAction::PaperCorrectedUpdate(Box::new(event)));
            let data: CsbStoreData = crate::store::replay(
                std::iter::once(self.import())
                    .chain(actions)
                    .chain(on_paper)
                    .map(|action| action.by(CsbUser::new_test())),
            );
            assert_no_dangling(&data);
            data
        }
    }

    fn assert_no_dangling(data: &CsbStoreData) {
        let pg = &data.paper_corrected_data;
        for omission in data.omissions.values() {
            let mut narrowed = omission.clone();
            assert!(
                narrow(&mut narrowed, pg, omission.updated_at),
                "{omission:?} is about nothing"
            );
            assert_eq!(&narrowed, omission, "{omission:?} points at something gone");
        }
        for person_id in data.csb_corrected_persons.keys() {
            assert!(pg.persons.contains_key(person_id));
        }
        assert_eq!(
            data.scrapped,
            super::super::Scrapped::derive(pg, &data.omissions)
        );
    }

    #[test]
    fn deleting_a_candidate_drops_their_omissions() {
        let imported = Imported::new();
        let theirs = imported.candidate(imported.first, vec![imported.a, imported.b]);
        let other = imported.candidate(imported.second, vec![imported.a]);
        let data = imported.replay(
            vec![
                CsbAction::CreateOmission(theirs.clone()),
                CsbAction::CreateOmission(other.clone()),
            ],
            vec![PgEvent::DeletePerson {
                person_id: imported.first,
            }],
        );

        assert!(!data.omissions.contains_key(&theirs.id));
        assert_eq!(data.omissions[&other.id], {
            let mut other = other.clone();
            other.updated_at = data.omissions[&other.id].updated_at;
            other
        });
    }

    #[test]
    fn taking_a_candidate_off_a_list_narrows_their_omission_to_the_other_lists() {
        let imported = Imported::new();
        let omission = imported.candidate(imported.first, vec![imported.a, imported.b]);
        let created = vec![CsbAction::CreateOmission(omission.clone())];

        let data = imported.replay(
            created.clone(),
            vec![PgEvent::RemoveCandidateFromCandidateList {
                list_id: imported.a,
                person_id: imported.first,
            }],
        );
        assert_eq!(
            data.omissions[&omission.id].category,
            OmissionCategory::Candidate {
                person: imported.first,
                lists: vec![imported.b],
            }
        );
        assert_eq!(
            data.omissions[&omission.id].updated_at,
            data.events.last().unwrap().created_at.into()
        );

        // Reordering and a CSV import replace the candidates too.
        let data = imported.replay(
            created.clone(),
            vec![
                PgEvent::UpdateCandidateListOrder {
                    list_id: imported.a,
                    candidates: vec![imported.second],
                },
                PgEvent::ImportCandidates(Box::new(CandidateImport {
                    list_id: imported.b,
                    file_name: "kandidaten.csv".to_string(),
                    file_size: 1,
                    created_persons: Vec::new(),
                    updated_persons: Vec::new(),
                    candidates: Vec::new(),
                })),
            ],
        );
        assert!(data.omissions.is_empty());
    }

    #[test]
    fn deleting_a_list_drops_it_from_the_omissions_about_it() {
        let imported = Imported::new();
        let both = sample_omission(OmissionCategory::CandidateList(vec![
            imported.a, imported.b,
        ]));
        let only_b = sample_omission(OmissionCategory::CandidateList(vec![imported.b]));
        let candidate = imported.candidate(imported.first, vec![imported.b]);
        let data = imported.replay(
            vec![
                CsbAction::CreateOmission(both.clone()),
                CsbAction::CreateOmission(only_b.clone()),
                CsbAction::CreateOmission(candidate.clone()),
            ],
            vec![PgEvent::DeleteCandidateList(imported.b)],
        );

        assert_eq!(data.omissions.len(), 1);
        assert_eq!(
            data.omissions[&both.id].category,
            OmissionCategory::CandidateList(vec![imported.a])
        );
    }

    #[test]
    fn a_district_no_list_is_submitted_in_drops_out_of_the_declarations_of_support() {
        let imported = Imported::new();
        let all = sample_omission(OmissionCategory::DeclarationsOfSupport(vec![
            ElectoralDistrict::Groningen,
            ElectoralDistrict::Fryslan,
            ElectoralDistrict::Utrecht,
        ]));
        let utrecht = sample_omission(OmissionCategory::DeclarationsOfSupport(vec![
            ElectoralDistrict::Utrecht,
        ]));
        // Legacy: no districts means all of them.
        let legacy = sample_omission(OmissionCategory::DeclarationsOfSupport(Vec::new()));
        let data = imported.replay(
            vec![
                CsbAction::CreateOmission(all.clone()),
                CsbAction::CreateOmission(utrecht.clone()),
                CsbAction::CreateOmission(legacy.clone()),
            ],
            vec![
                PgEvent::UpdateCandidateListDistricts {
                    list_id: imported.a,
                    electoral_districts: [ElectoralDistrict::Groningen].into(),
                },
                PgEvent::DeleteCandidateList(imported.b),
            ],
        );

        assert_eq!(
            data.omissions[&all.id].category,
            OmissionCategory::DeclarationsOfSupport(vec![ElectoralDistrict::Groningen])
        );
        assert!(!data.omissions.contains_key(&utrecht.id));
        assert_eq!(data.omissions[&legacy.id].category, legacy.category);
    }

    #[test]
    fn blanking_the_list_drops_what_was_recorded_about_the_appellation() {
        let imported = Imported::new();
        let omission = sample_omission(OmissionCategory::Appellation);
        let mut blank = sample_political_group();
        blank.list_designation = Some(ListDesignation::Blank);
        let data = imported.replay(
            vec![
                CsbAction::CreateOmission(omission),
                CsbAction::UpdateCorrection(Correction::Appellation(
                    Appellation::from_str("Gecorrigeerde Naam").unwrap(),
                )),
            ],
            vec![PgEvent::UpdatePoliticalGroup(blank)],
        );

        assert!(data.omissions.is_empty());
        assert_eq!(data.csb_corrected_appellation, None);
        assert_eq!(data.get_restoration_count(), 0);
    }

    #[test]
    fn a_decision_is_kept_on_the_part_that_is_left() {
        let imported = Imported::new();
        let mut omission = sample_omission(OmissionCategory::CandidateList(vec![
            imported.a, imported.b,
        ]));
        omission.recoverable = false;
        let data = imported.replay(
            vec![CsbAction::CreateOmission(omission.clone())],
            vec![PgEvent::DeleteCandidateList(imported.a)],
        );

        assert!(data.scrapped.is_list_scrapped(imported.b));
        assert!(!data.scrapped.is_list_scrapped(imported.a));
        assert_eq!(data.omissions[&omission.id].status, OmissionStatus::Pending);
    }

    #[test]
    fn an_omission_about_something_already_gone_is_not_recorded() {
        let imported = Imported::new();
        let gone = imported.candidate(PersonId::new(), vec![imported.a]);
        let not_on_b = imported.candidate(imported.second, vec![imported.a, imported.b]);
        let no_list = sample_omission(OmissionCategory::CandidateList(
            vec![CandidateListId::new()],
        ));
        let data = imported.replay(
            vec![
                CsbAction::CreateOmission(gone),
                CsbAction::CreateOmission(not_on_b.clone()),
                CsbAction::CreateOmission(no_list),
            ],
            Vec::new(),
        );

        assert_eq!(data.omissions.len(), 1);
        assert_eq!(
            data.omissions[&not_on_b.id].category,
            OmissionCategory::Candidate {
                person: imported.second,
                lists: vec![imported.a],
            }
        );
    }

    #[test]
    fn deleting_a_candidate_drops_the_committees_corrections_on_them() {
        let imported = Imported::new();
        let correct = |person| {
            CsbAction::UpdateCorrection(Correction::Person(
                person,
                PersonCorrection::LastName(LastName::from_str("Gecorrigeerd").unwrap()),
            ))
        };
        let data = imported.replay(
            vec![correct(imported.first), correct(imported.second)],
            vec![PgEvent::DeletePerson {
                person_id: imported.first,
            }],
        );

        assert_eq!(data.get_all_csb_corrected_persons(), vec![imported.second]);
        assert_eq!(data.get_correction_count(), 1);
    }

    #[test]
    fn a_paper_correction_to_the_corrected_value_drops_the_committees_correction() {
        let imported = Imported::new();
        let corrected = LastName::from_str("Gecorrigeerd").unwrap();
        let mut on_paper = sample_person(imported.first);
        on_paper.name.last_name = corrected.clone();
        let mut renamed = sample_political_group();
        renamed.appellation = Some(Appellation::from_str("Gecorrigeerde Naam").unwrap());
        let data = imported.replay(
            vec![
                CsbAction::UpdateCorrection(Correction::Person(
                    imported.first,
                    PersonCorrection::LastName(corrected),
                )),
                CsbAction::UpdateCorrection(Correction::Person(
                    imported.first,
                    PersonCorrection::Initials(Some("X.Y.".parse().unwrap())),
                )),
                CsbAction::UpdateCorrection(Correction::Appellation(
                    renamed.appellation.clone().unwrap(),
                )),
            ],
            vec![
                PgEvent::UpdatePerson(Box::new(on_paper)),
                PgEvent::UpdatePoliticalGroup(renamed),
            ],
        );

        // The initials still differ from the paper.
        assert_eq!(
            data.get_person_corrections(imported.first),
            [PersonCorrection::Initials(Some("X.Y.".parse().unwrap()))].into()
        );
        assert_eq!(data.csb_corrected_appellation, None);
        assert_eq!(data.get_correction_count(), 1);
    }
}
