use axum_extra::routing::TypedPath;
use chrono::{DateTime, Utc};

use crate::{
    Event, Locale, PgEvent, QueryParamState,
    audit_log::AuditLogDetailPath,
    store::StoreEvent,
    structs::audit_log::{AuditEvent, SubjectLink, SubjectLinks},
};

/// A single entry in the audit log, representing one application event.
pub struct AuditLogEntry {
    pub event_id: usize,
    pub description: String,
    pub details: String,
    /// The item the event is about.
    pub subject: Option<SubjectLink>,
    pub created_at: DateTime<Utc>,
}

impl AuditLogEntry {
    pub fn new(event: &StoreEvent<PgEvent>, locale: Locale, links: &dyn SubjectLinks) -> Self {
        Self {
            event_id: event.event_id,
            description: event.payload.description(locale),
            details: event.payload.details(),
            subject: event.payload.subject().map(|subject| links.link(subject)),
            created_at: event.created_at,
        }
    }

    pub fn subject_id(&self) -> Option<&str> {
        self.subject.as_ref()?.id_full.as_deref()
    }

    pub fn detail_path_with_return(&self, return_to: &str) -> String {
        AuditLogDetailPath {
            event_id: self.event_id,
        }
        .with_query_params(QueryParamState::redirect_to(return_to.to_string()))
        .to_string()
    }

    /// Check whether this entry matches a search query (case-insensitive).
    /// Searches across details, subject ID, and description.
    pub fn matches_search(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.details.to_lowercase().contains(&query)
            || self
                .subject_id()
                .is_some_and(|id| id.to_lowercase().contains(&query))
            || self.description.to_lowercase().contains(&query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Locale, StreamId,
        structs::{
            audit_log::{NoLinks, Subject},
            candidate_lists::CandidateListId,
            list_submitters::ListSubmitterId,
            name_authorisations::NameAuthorisationId,
            persons::PersonId,
        },
        test_utils::{
            sample_candidate_list, sample_list_submitter, sample_name_authorisation, sample_person,
            sample_political_group,
        },
    };

    const EN: Locale = Locale::En;

    /// The description and details of each kind of event, as the list
    /// shows them.
    #[test]
    #[expect(clippy::too_many_lines, reason = "One case per kind of event.")]
    fn describes_each_kind_of_event() {
        let person = sample_person(PersonId::new());
        let group = sample_political_group();
        let name_auth = sample_name_authorisation(NameAuthorisationId::new());
        let submitter = sample_list_submitter(ListSubmitterId::new());
        let list_id = CandidateListId::new();
        let none = PgEvent::DEFAULT_DETAILS.to_string();
        let cases = [
            (
                PgEvent::CreatePerson(Box::new(person.clone())),
                "Created person",
                person.name.display(),
            ),
            (
                PgEvent::UpdatePoliticalGroup(group.clone()),
                "Updated political group",
                group.appellation.as_ref().unwrap().to_string(),
            ),
            (
                PgEvent::DeletePerson {
                    person_id: person.id,
                },
                "Deleted person",
                none.clone(),
            ),
            (
                PgEvent::CreateNameAuthorisation(Box::new(name_auth.clone())),
                "Created statutory name and authorised agent",
                format!("{} ({})", name_auth.legal_name, name_auth.name.display()),
            ),
            (
                PgEvent::UpdateListSubmitter(Box::new(submitter.clone())),
                "Updated submitter of the list",
                submitter.name.display(),
            ),
            (
                PgEvent::CreateSubstituteSubmitter(Box::new(submitter.clone())),
                "Created substitute submitter of the list",
                submitter.name.display(),
            ),
            (
                PgEvent::DownloadFile {
                    file_name: "model-h1.pdf".to_string(),
                    download_path: "/download/h1".to_string(),
                },
                "Downloaded file",
                "model-h1.pdf".to_string(),
            ),
            (
                PgEvent::CreateCandidateList(Box::new(sample_candidate_list(list_id))),
                "Created list of candidates",
                "Utrecht".to_string(),
            ),
            (
                PgEvent::DeleteCandidateList(list_id),
                "Deleted list of candidates",
                none.clone(),
            ),
            (
                PgEvent::AddCandidateToCandidateList {
                    list_id,
                    person_id: person.id,
                },
                "Added candidate to list",
                none.clone(),
            ),
            (
                PgEvent::RemoveCandidateFromCandidateList {
                    list_id,
                    person_id: person.id,
                },
                "Removed candidate from list",
                none.clone(),
            ),
            (
                PgEvent::UpdatePersonAddress {
                    person_id: person.id,
                    address: Box::new(crate::structs::common::DutchAddress::default()),
                },
                "Updated person address",
                none.clone(),
            ),
            (
                PgEvent::ExportCsv {
                    file_name: "candidates.csv".to_string(),
                    file_size: 1024,
                    list_id,
                },
                "Exported CSV",
                "candidates.csv".to_string(),
            ),
            (
                PgEvent::DeveloperLogin {
                    stream_id: StreamId::new(),
                },
                "Developer login",
                none.clone(),
            ),
        ];

        for (event, description, details) in cases {
            let entry = AuditLogEntry::new(&StoreEvent::new(1, event), EN, &NoLinks);
            assert_eq!(entry.description, description);
            assert_eq!(entry.details, details, "details of {description}");
        }
    }

    #[test]
    fn subjects_name_the_item_the_event_is_about() {
        let (list_id, person_id) = (CandidateListId::new(), PersonId::new());

        assert_eq!(
            PgEvent::ExportCsv {
                file_name: "candidates.csv".to_string(),
                file_size: 1024,
                list_id,
            }
            .subject(),
            Some(Subject::CandidateList(list_id))
        );
        assert_eq!(
            PgEvent::UpdatePersonAddress {
                person_id,
                address: Box::new(crate::structs::common::DutchAddress::default()),
            }
            .subject(),
            Some(Subject::Person(person_id))
        );
        let deleted = StoreEvent::new(1, PgEvent::DeleteCandidateList(list_id));
        let entry = AuditLogEntry::new(&deleted, EN, &NoLinks);
        assert_eq!(entry.subject_id(), Some(list_id.to_string().as_str()));
    }

    #[test]
    fn translates_to_dutch() {
        let person_id = PersonId::new();
        let event = StoreEvent::new(1, PgEvent::DeletePerson { person_id });

        let entry = AuditLogEntry::new(&event, Locale::Nl, &NoLinks);

        assert_eq!(entry.description, "Persoon verwijderd");
    }

    #[test]
    fn preserves_event_timestamp() {
        let timestamp = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let event = StoreEvent::new_at(
            1,
            PgEvent::DeletePerson {
                person_id: PersonId::new(),
            },
            timestamp,
        );

        let entry = AuditLogEntry::new(&event, EN, &NoLinks);

        assert_eq!(entry.created_at, timestamp);
    }

    #[test]
    fn matches_search_by_description() {
        let person = sample_person(PersonId::new());
        let event = StoreEvent::new(1, PgEvent::CreatePerson(Box::new(person)));
        let entry = AuditLogEntry::new(&event, EN, &NoLinks);

        assert!(entry.matches_search("Created"));
        assert!(entry.matches_search("created"));
        assert!(!entry.matches_search("nonexistent"));
    }

    #[test]
    fn matches_search_by_details() {
        let event = StoreEvent::new(
            1,
            PgEvent::DownloadFile {
                file_name: "export-report.pdf".to_string(),
                download_path: "/download/report".to_string(),
            },
        );
        let entry = AuditLogEntry::new(&event, EN, &NoLinks);

        assert!(entry.matches_search("export-report"));
        assert!(entry.matches_search("EXPORT-REPORT"));
    }

    #[test]
    fn matches_search_by_subject_id() {
        let person_id = PersonId::new();
        let person = sample_person(person_id);
        let event = StoreEvent::new(1, PgEvent::CreatePerson(Box::new(person)));
        let entry = AuditLogEntry::new(&event, EN, &NoLinks);

        let id = person_id.to_string();
        assert_eq!(entry.subject_id(), Some(id.as_str()));
        assert!(entry.matches_search(&crate::abbreviate_str(&id)));
        assert!(entry.matches_search(&id));
    }

    #[test]
    fn event_category_and_key_are_set_correctly() {
        let person = sample_person(PersonId::new());
        let event = PgEvent::CreatePerson(Box::new(person));
        assert_eq!(event.category(), "person");
        assert_eq!(event.key(), "create_person");

        let pg = sample_political_group();
        let event = PgEvent::UpdatePoliticalGroup(pg);
        assert_eq!(event.category(), "political_group");
        assert_eq!(event.key(), "update_political_group");

        let event = PgEvent::DeveloperLogin {
            stream_id: StreamId::new(),
        };
        assert_eq!(event.category(), "system");
        assert_eq!(event.key(), "developer_login");
    }
}
