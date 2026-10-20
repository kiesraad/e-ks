//! Where the CSB audit log links its subjects to.
//!
//! The CSB shows a political group's items on its own pages, not on the
//! political group's: a person changed by a paper correction links to the
//! candidate in the examination, not to `/persons/...`.

use crate::{
    AppError, CsbMainStore, CsbStream, Locale, StreamId,
    csb::{
        examination::paths::{
            CsbAllRestorationsPath, CsbCandidateListPath, CsbCandidatePath,
            CsbGeneralInformationPath, CsbHearingDetailsPath, CsbPoliticalGroupPath,
        },
        finalise::CsbFinalisePath,
        pre_submission::paths::CsbPreSubmissionGroupPath,
    },
    projection::WithCorrections,
    structs::audit_log::{Subject, SubjectLink, SubjectLinks},
    trans,
};

/// Links from the audit log of one political group's stream. `store` is the
/// stream as it stands now: an item that has since been deleted gets no link.
pub struct CsbStreamLinks<'a> {
    pub store: &'a CsbStream,
    /// A package checked before submission; only its overview has a page.
    pub pre_submission: bool,
}

impl<'a> CsbStreamLinks<'a> {
    /// The stream `stream_id` among the imported and the pre-submission
    /// streams of the election.
    pub fn find(
        import_stores: &'a [CsbStream],
        pre_submission_stores: &'a [CsbStream],
        stream_id: StreamId,
    ) -> Result<Self, AppError> {
        streams(import_stores, pre_submission_stores)
            .find(|(store, _)| store.stream_id == stream_id)
            .map(|(store, pre_submission)| Self {
                store,
                pre_submission,
            })
            .ok_or(AppError::GenericNotFound)
    }

    /// How the stream filter and the detail page name the stream.
    pub fn stream_label(&self, locale: Locale) -> String {
        stream_label(self.store, self.pre_submission, locale)
    }

    fn href(&self, subject: Subject) -> Option<String> {
        let store = self.store;
        let stream_id = store.stream_id;
        if store.is_deleted() {
            return None;
        }
        if self.pre_submission {
            return matches!(subject, Subject::PoliticalGroupStream(_))
                .then(|| CsbPreSubmissionGroupPath { stream_id }.to_string());
        }
        match subject {
            Subject::PoliticalGroupStream(stream_id) => {
                Some(CsbPoliticalGroupPath { stream_id }.to_string())
            }
            Subject::ListSubmitter(_)
            | Subject::SubstituteSubmitter(_)
            | Subject::NameAuthorisation(_) => {
                Some(CsbGeneralInformationPath { stream_id }.to_string())
            }
            // A candidate's page is scoped to a list: the first they are on.
            Subject::Person(person_id) => store.get_first_list(person_id).map(|list| {
                CsbCandidatePath {
                    stream_id,
                    list_id: list.id,
                    person_id,
                }
                .to_string()
            }),
            Subject::CandidateList(list_id) => store
                .get_candidate_list(list_id, WithCorrections::All)
                .is_some()
                .then(|| CsbCandidateListPath { stream_id, list_id }.to_string()),
            Subject::Omission(id) => store
                .get_omission(id)
                .is_ok()
                .then(|| CsbAllRestorationsPath { stream_id }.to_string()),
            Subject::PoliticalGroup
            | Subject::RegisteredPoliticalGroup(_)
            | Subject::Objection(_)
            | Subject::HearingDetails(_)
            | Subject::ListOrder => None,
        }
    }
}

impl SubjectLinks for CsbStreamLinks<'_> {
    fn link(&self, subject: Subject) -> SubjectLink {
        let subject = subject.in_stream(self.store.stream_id);
        SubjectLink {
            id_full: subject.own_id(),
            href: self.href(subject),
        }
    }
}

/// The imported streams followed by the pre-submission streams, each marked
/// as such.
pub fn streams<'a>(
    import_stores: &'a [CsbStream],
    pre_submission_stores: &'a [CsbStream],
) -> impl Iterator<Item = (&'a CsbStream, bool)> {
    import_stores
        .iter()
        .map(|store| (store, false))
        .chain(pre_submission_stores.iter().map(|store| (store, true)))
}

/// A political group's stream by its appellation, marked when it was deleted
/// or is a pre-submission package.
pub fn stream_label(store: &CsbStream, pre_submission: bool, locale: Locale) -> String {
    let label = store.get_appellation_with_deleted_label(WithCorrections::All, locale);
    if pre_submission {
        trans!("audit_log.filter.pre_submission_stream", locale, label)
    } else {
        label
    }
}

/// Links from the audit log of the committee's main stream.
pub struct CsbMainLinks<'a> {
    pub store: &'a CsbMainStore,
    /// The political groups' streams, to name and link the groups the main
    /// stream refers to.
    pub streams: &'a [CsbStream],
    pub locale: Locale,
}

impl CsbMainLinks<'_> {
    fn href(&self, subject: Subject) -> Option<String> {
        let store = self.store;
        match subject {
            Subject::RegisteredPoliticalGroup(id) => store
                .get_registered_political_group(id)
                .ok()
                .map(|group| group.edit_path().to_string()),
            Subject::Objection(id) => store
                .get_objection(id)
                .ok()
                .map(|objection| objection.update_path().to_string()),
            Subject::HearingDetails(model) => Some(CsbHearingDetailsPath { model }.to_string()),
            Subject::ListOrder => Some(CsbFinalisePath.to_string()),
            Subject::PoliticalGroupStream(stream_id) => self
                .stream(stream_id)
                .filter(|stream| !stream.is_deleted())
                .map(|_| CsbPoliticalGroupPath { stream_id }.to_string()),
            Subject::PoliticalGroup
            | Subject::ListSubmitter(_)
            | Subject::SubstituteSubmitter(_)
            | Subject::Person(_)
            | Subject::CandidateList(_)
            | Subject::NameAuthorisation(_)
            | Subject::Omission(_) => None,
        }
    }

    fn stream(&self, stream_id: StreamId) -> Option<&CsbStream> {
        self.streams
            .iter()
            .find(|stream| stream.stream_id == stream_id)
    }
}

impl SubjectLinks for CsbMainLinks<'_> {
    fn link(&self, subject: Subject) -> SubjectLink {
        SubjectLink {
            id_full: subject.own_id(),
            href: self.href(subject),
        }
    }

    fn describe(&self, subject: Subject) -> Option<String> {
        match subject {
            Subject::PoliticalGroupStream(stream_id) => self
                .stream(stream_id)
                .map(|stream| stream_label(stream, false, self.locale)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CsbAction, CsbUser,
        store::StoreEvent,
        structs::{candidate_lists::CandidateListId, persons::PersonId},
        test_utils::{sample_candidate_list, sample_person},
    };

    fn stream_with_candidate(person_id: PersonId, list_id: CandidateListId) -> CsbStream {
        let store = CsbStream::new_for_test();
        store.add_person(sample_person(person_id));
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id];
        store.add_candidate_list(list);
        store
    }

    /// The same person links to the CSB's candidate page, not the political
    /// group's person page.
    #[test]
    fn person_links_to_the_candidate_in_the_examination() {
        let (person_id, list_id) = (PersonId::new(), CandidateListId::new());
        let store = stream_with_candidate(person_id, list_id);
        let stream_id = store.stream_id;
        let links = CsbStreamLinks {
            store: &store,
            pre_submission: false,
        };

        let link = links.link(Subject::Person(person_id));
        assert_eq!(link.id_full, Some(person_id.to_string()));
        assert_eq!(
            link.href,
            Some(format!(
                "/csb/examination/{stream_id}/list/{list_id}/candidate/{person_id}"
            ))
        );

        let group = links.link(Subject::PoliticalGroup);
        assert_eq!(group.id_full, Some(stream_id.to_string()));
        assert_eq!(group.href, Some(format!("/csb/examination/{stream_id}")));
    }

    #[test]
    fn pre_submission_only_links_the_package() {
        let (person_id, list_id) = (PersonId::new(), CandidateListId::new());
        let store = stream_with_candidate(person_id, list_id);
        let links = CsbStreamLinks {
            store: &store,
            pre_submission: true,
        };

        assert_eq!(links.link(Subject::Person(person_id)).href, None);
        assert_eq!(
            links.link(Subject::PoliticalGroup).href,
            Some(format!("/csb/pre-submission/{}", store.stream_id))
        );
    }

    #[test]
    fn a_deleted_stream_links_nowhere() {
        let store = CsbStream::new_for_test();
        store.apply_event(StoreEvent::new(
            1,
            CsbAction::Delete.by(CsbUser::new_test()),
        ));
        let links = CsbStreamLinks {
            store: &store,
            pre_submission: false,
        };

        assert_eq!(links.link(Subject::PoliticalGroup).href, None);
    }

    #[test]
    fn find_tells_a_pre_submission_stream_from_an_import() {
        let imports = [CsbStream::new_for_test()];
        let pre_submissions = [CsbStream::new_for_test()];

        let found =
            CsbStreamLinks::find(&imports, &pre_submissions, pre_submissions[0].stream_id).unwrap();
        assert!(found.pre_submission);
        let found = CsbStreamLinks::find(&imports, &pre_submissions, imports[0].stream_id).unwrap();
        assert!(!found.pre_submission);
        assert!(CsbStreamLinks::find(&imports, &pre_submissions, StreamId::new()).is_err());
    }
}
