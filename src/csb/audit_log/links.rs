//! Where the CSB audit log links its subjects to.
//!
//! The CSB shows a political group's items on its own pages, not on the
//! political group's: a person changed by a paper correction links to the
//! candidate in the examination, not to `/persons/...`.

use std::sync::Arc;

use crate::{
    AppError, CsbMainStore, CsbStoreData, CsbStream, Locale, StreamId,
    csb::{
        audit_log::CsbAuditLogPath,
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

/// Links from the audit log of one political group's stream. `data` is the
/// stream as it stands now: an item that has since been deleted gets no link.
pub struct CsbStreamLinks {
    pub stream_id: StreamId,
    pub data: Arc<CsbStoreData>,
    /// A package checked before submission; only its overview has a page.
    pub pre_submission: bool,
}

impl CsbStreamLinks {
    pub fn new(store: &CsbStream, pre_submission: bool) -> Self {
        Self {
            stream_id: store.stream_id,
            data: store.snapshot(),
            pre_submission,
        }
    }

    /// The stream `stream_id` among the imported and the pre-submission
    /// streams of the election.
    pub fn find(
        import_stores: &[CsbStream],
        pre_submission_stores: &[CsbStream],
        stream_id: StreamId,
    ) -> Result<Self, AppError> {
        streams(import_stores, pre_submission_stores)
            .find(|(store, _)| store.stream_id == stream_id)
            .map(|(store, pre_submission)| Self::new(store, pre_submission))
            .ok_or(AppError::GenericNotFound)
    }

    /// How the stream filter and the detail page name the stream.
    pub fn stream_label(&self, locale: Locale) -> String {
        stream_label(&self.data, self.pre_submission, locale)
    }

    fn href(&self, subject: Subject) -> Option<String> {
        let data = &self.data;
        let stream_id = self.stream_id;
        if data.is_deleted() {
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
            Subject::Person(person_id) => data
                .pg(WithCorrections::All)
                .get_first_list(person_id)
                .map(|list| {
                    CsbCandidatePath {
                        stream_id,
                        list_id: list.id,
                        person_id,
                    }
                    .to_string()
                }),
            Subject::CandidateList(list_id) => data
                .pg(WithCorrections::All)
                .get_candidate_list(list_id)
                .is_ok()
                .then(|| CsbCandidateListPath { stream_id, list_id }.to_string()),
            Subject::Omission(id) => data
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

impl SubjectLinks for CsbStreamLinks {
    fn link(&self, subject: Subject) -> SubjectLink {
        let stream_id = self.stream_id;
        let subject = subject.in_stream(stream_id);
        let id_full = subject.own_id();
        SubjectLink {
            href: self.href(subject),
            events_href: id_full
                .as_ref()
                .map(|id| format!("{CsbAuditLogPath}?stream={stream_id}&search={id}")),
            id_full,
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
pub fn stream_label(data: &CsbStoreData, pre_submission: bool, locale: Locale) -> String {
    let label = data.get_appellation_with_deleted_label(WithCorrections::All, locale);
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
                .filter(|stream| !stream.snapshot().is_deleted())
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
        let id_full = subject.own_id();
        // A political group's events are on its own stream.
        let events_href = match subject {
            Subject::PoliticalGroupStream(stream_id) => {
                Some(format!("{CsbAuditLogPath}?stream={stream_id}"))
            }
            _ => id_full
                .as_ref()
                .map(|id| format!("{CsbAuditLogPath}?search={id}")),
        };
        SubjectLink {
            href: self.href(subject),
            events_href,
            id_full,
        }
    }

    fn describe(&self, subject: Subject) -> Option<String> {
        match subject {
            Subject::PoliticalGroupStream(stream_id) => self
                .stream(stream_id)
                .map(|stream| stream_label(&stream.snapshot(), false, self.locale)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CsbAction, CsbUser,
        store::{StoreData, StoreEvent},
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
        let links = CsbStreamLinks::new(&store, false);

        let link = links.link(Subject::Person(person_id));
        assert_eq!(link.id_full, Some(person_id.to_string()));
        assert_eq!(
            link.href,
            Some(format!(
                "/csb/examination/{stream_id}/list/{list_id}/candidate/{person_id}"
            ))
        );

        assert_eq!(
            link.events_href,
            Some(format!(
                "/csb/audit-log?stream={stream_id}&search={person_id}"
            ))
        );

        let group = links.link(Subject::PoliticalGroup);
        assert_eq!(group.id_full, Some(stream_id.to_string()));
        assert_eq!(group.href, Some(format!("/csb/examination/{stream_id}")));
    }

    /// The main stream refers to political groups by stream; their events
    /// are on that stream, not among the main stream's.
    #[test]
    fn main_stream_links_a_political_group_to_its_own_events() {
        let main_store = CsbMainStore::new_for_test();
        let streams = [CsbStream::new_for_test()];
        let stream_id = streams[0].stream_id;
        let links = CsbMainLinks {
            store: &main_store,
            streams: &streams,
            locale: Locale::En,
        };

        let link = links.link(Subject::PoliticalGroupStream(stream_id));
        assert_eq!(
            link.events_href,
            Some(format!("/csb/audit-log?stream={stream_id}"))
        );

        assert_eq!(links.link(Subject::ListOrder).events_href, None);
    }

    #[test]
    fn pre_submission_only_links_the_package() {
        let (person_id, list_id) = (PersonId::new(), CandidateListId::new());
        let store = stream_with_candidate(person_id, list_id);
        let links = CsbStreamLinks::new(&store, true);

        assert_eq!(links.link(Subject::Person(person_id)).href, None);
        assert_eq!(
            links.link(Subject::PoliticalGroup).href,
            Some(format!("/csb/pre-submission/{}", store.stream_id))
        );
    }

    #[test]
    fn a_deleted_stream_links_nowhere() {
        let store = CsbStream::new_for_test();
        store.write().apply(StoreEvent::new(
            1,
            CsbAction::Delete.by(CsbUser::new_test()),
        ));
        let links = CsbStreamLinks::new(&store, false);

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
