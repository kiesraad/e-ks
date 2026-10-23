//! Where the political group's audit log links its subjects to.

use crate::{
    PgStore,
    audit_log::AuditLogPath,
    candidate_lists::ViewCandidateListPath,
    persons::UpdatePersonPath,
    structs::{
        audit_log::{Subject, SubjectLink, SubjectLinks},
        list_submitters::ListSubmitter,
        political_groups::PoliticalGroup,
    },
};

/// Links into the political group's part of the app. `store` is the stream as
/// it stands now: an item that has since been deleted gets no link.
pub struct PgLinks<'a> {
    pub store: &'a PgStore,
}

impl PgLinks<'_> {
    fn href(&self, subject: Subject) -> Option<String> {
        let store = self.store;
        match subject {
            Subject::PoliticalGroupStream(_) => Some(PoliticalGroup::update_path().to_string()),
            Subject::ListSubmitter(_) => Some(ListSubmitter::update_path().to_string()),
            Subject::SubstituteSubmitter(id) => store
                .get_substitute_submitter(id)
                .ok()
                .map(|s| s.substitute_update_path().to_string()),
            Subject::Person(person_id) => store
                .get_person(person_id)
                .is_ok()
                .then(|| UpdatePersonPath { person_id }.to_string()),
            Subject::CandidateList(list_id) => store
                .get_candidate_list(list_id)
                .is_ok()
                .then(|| ViewCandidateListPath { list_id }.to_string()),
            Subject::NameAuthorisation(id) => store
                .get_name_authorisation(id)
                .ok()
                .map(|na| na.update_path().to_string()),
            // Committee items have no page in this part of the app.
            Subject::PoliticalGroup
            | Subject::Omission(_)
            | Subject::RegisteredPoliticalGroup(_)
            | Subject::Objection(_)
            | Subject::HearingDetails(_)
            | Subject::ListOrder => None,
        }
    }
}

impl SubjectLinks for PgLinks<'_> {
    fn link(&self, subject: Subject) -> SubjectLink {
        let subject = subject.in_stream(self.store.stream_id);
        let id_full = subject.own_id();
        SubjectLink {
            href: self.href(subject),
            events_href: id_full
                .as_ref()
                .map(|id| format!("{AuditLogPath}?search={id}")),
            id_full,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{structs::persons::PersonId, test_utils::sample_person};

    #[tokio::test]
    async fn links_existing_items_to_their_page_and_deleted_ones_nowhere() {
        let store = PgStore::new_for_test();
        let person = sample_person(PersonId::new());
        person.create(&store).await.unwrap();
        let deleted = PersonId::new();
        let links = PgLinks { store: &store };

        let link = links.link(Subject::Person(person.id));
        assert_eq!(link.id_full, Some(person.id.to_string()));
        assert_eq!(link.href, Some(format!("/persons/{}/update", person.id)));
        assert_eq!(
            link.events_href,
            Some(format!("/audit-log?search={}", person.id))
        );

        let link = links.link(Subject::Person(deleted));
        assert_eq!(link.id_full, Some(deleted.to_string()));
        assert_eq!(link.href, None);
    }

    #[test]
    fn the_political_group_is_named_by_the_stream_id() {
        let store = PgStore::new_for_test();
        let link = PgLinks { store: &store }.link(Subject::PoliticalGroup);

        assert_eq!(link.id_full, Some(store.stream_id.to_string()));
        assert_eq!(link.href.as_deref(), Some("/political-group/information"));
    }
}
