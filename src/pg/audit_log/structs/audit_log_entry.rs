use axum_extra::routing::TypedPath;
use chrono::{DateTime, Utc};

use crate::{Event, Locale, PgEvent, audit_log::AuditLogDetailPath, store::StoreEvent};

/// A single entry in the audit log, representing one application event.
pub struct AuditLogEntry {
    pub event_id: usize,
    pub description: String,
    pub details: String,
    pub subject_id_full: String,
    pub subject_path: String,
    pub created_at: DateTime<Utc>,
}

impl AuditLogEntry {
    pub fn new(event: StoreEvent<PgEvent>, locale: Locale) -> Self {
        let full_id = event.payload.subject_id_full();
        Self {
            event_id: event.event_id,
            description: event.payload.description(locale),
            details: event.payload.details(),
            subject_id_full: full_id,
            subject_path: event.payload.subject_path(),
            created_at: event.created_at,
        }
    }

    pub fn detail_path(&self) -> impl TypedPath {
        AuditLogDetailPath {
            event_id: self.event_id,
        }
    }

    /// Check whether this entry matches a search query (case-insensitive).
    /// Searches across details, subject ID, and description.
    pub fn matches_search(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.details.to_lowercase().contains(&query)
            || self.subject_id_full.to_lowercase().contains(&query)
            || self.description.to_lowercase().contains(&query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Locale, StreamId,
        structs::{
            candidate_lists::CandidateListId, list_submitters::ListSubmitterId,
            name_authorisations::NameAuthorisationId, persons::PersonId,
        },
        test_utils::{
            sample_candidate_list, sample_list_submitter, sample_name_authorisation, sample_person,
            sample_political_group,
        },
    };

    const EN: Locale = Locale::En;

    #[test]
    fn from_create_person_event() {
        let person = sample_person(PersonId::new());
        let expected_name = person.name.display();
        let event = StoreEvent::new(1, PgEvent::CreatePerson(person));

        let entry = AuditLogEntry::new(event, EN);

        assert_eq!(entry.event_id, 1);
        assert_eq!(entry.description, "Created person");
        assert_eq!(entry.details, expected_name);
    }

    #[test]
    fn from_update_political_group_event() {
        let pg = sample_political_group();
        let expected_name = pg.appellation.as_ref().unwrap().to_string();
        let event = StoreEvent::new(2, PgEvent::UpdatePoliticalGroup(pg));

        let entry = AuditLogEntry::new(event, EN);

        assert_eq!(entry.event_id, 2);
        assert_eq!(entry.description, "Updated political group");
        assert_eq!(entry.details, expected_name);
    }

    #[test]
    fn from_delete_person_event_has_no_details() {
        let person_id = PersonId::new();
        let event = StoreEvent::new(3, PgEvent::DeletePerson { person_id });

        let entry = AuditLogEntry::new(event, EN);

        assert_eq!(entry.description, "Deleted person");
        assert_eq!(entry.details, PgEvent::DEFAULT_DETAILS);
    }

    #[test]
    fn from_create_name_authorisation_event() {
        let name_auth = sample_name_authorisation(NameAuthorisationId::new());
        let expected_name = format!("{} ({})", name_auth.legal_name, name_auth.name.display());
        let event = StoreEvent::new(4, PgEvent::CreateNameAuthorisation(name_auth));

        let entry = AuditLogEntry::new(event, EN);

        assert_eq!(
            entry.description,
            "Created statutory name and authorised agent"
        );
        assert_eq!(entry.details, expected_name);
    }

    #[test]
    fn from_update_list_submitter_event() {
        let submitter = sample_list_submitter(ListSubmitterId::new());
        let expected_name = submitter.name.display();
        let event = StoreEvent::new(5, PgEvent::UpdateListSubmitter(submitter));

        let entry = AuditLogEntry::new(event, EN);

        assert_eq!(entry.description, "Updated submitter of the list");
        assert_eq!(entry.details, expected_name);
    }

    #[test]
    fn from_download_file_event() {
        let event = StoreEvent::new(
            6,
            PgEvent::DownloadFile {
                file_name: "model-h1.pdf".to_string(),
                download_path: "/download/h1".to_string(),
            },
        );

        let entry = AuditLogEntry::new(event, EN);

        assert_eq!(entry.description, "Downloaded file");
        assert_eq!(entry.details, "model-h1.pdf");
    }

    #[test]
    fn from_create_candidate_list_event_shows_districts() {
        let list = sample_candidate_list(CandidateListId::new());
        let event = StoreEvent::new(7, PgEvent::CreateCandidateList(list));

        let entry = AuditLogEntry::new(event, EN);

        assert_eq!(entry.description, "Created list of candidates");
        assert_eq!(entry.details, "Utrecht");
    }

    #[test]
    fn from_developer_login_event_shows_default_details() {
        let event = StoreEvent::new(
            8,
            PgEvent::DeveloperLogin {
                stream_id: StreamId::new(),
            },
        );

        let entry = AuditLogEntry::new(event, EN);

        assert_eq!(entry.description, "Developer login");
        assert_eq!(entry.details, PgEvent::DEFAULT_DETAILS);
    }

    #[test]
    fn translates_to_dutch() {
        let person_id = PersonId::new();
        let event = StoreEvent::new(1, PgEvent::DeletePerson { person_id });

        let entry = AuditLogEntry::new(event, Locale::Nl);

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

        let entry = AuditLogEntry::new(event, EN);

        assert_eq!(entry.created_at, timestamp);
    }

    #[test]
    fn matches_search_by_description() {
        let person = sample_person(PersonId::new());
        let event = StoreEvent::new(1, PgEvent::CreatePerson(person));
        let entry = AuditLogEntry::new(event, EN);

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
        let entry = AuditLogEntry::new(event, EN);

        assert!(entry.matches_search("export-report"));
        assert!(entry.matches_search("EXPORT-REPORT"));
    }

    #[test]
    fn matches_search_by_subject_id() {
        let person_id = PersonId::new();
        let person = sample_person(person_id);
        let event = StoreEvent::new(1, PgEvent::CreatePerson(person));
        let entry = AuditLogEntry::new(event, EN);

        assert!(entry.matches_search(&crate::abbreviate_str(&entry.subject_id_full)));
        assert!(entry.matches_search(&entry.subject_id_full));
    }

    #[test]
    fn from_export_csv_event() {
        let list_id = CandidateListId::new();
        let event = StoreEvent::new(
            1,
            PgEvent::ExportCsv {
                file_name: "candidates.csv".to_string(),
                file_size: 1024,
                list_id,
            },
        );

        let entry = AuditLogEntry::new(event, EN);

        assert_eq!(entry.description, "Exported CSV");
        assert_eq!(entry.details, "candidates.csv");
        assert_eq!(entry.subject_path, format!("/candidate-lists/{list_id}"));
    }

    #[test]
    fn from_delete_candidate_list_event() {
        let cl_id = CandidateListId::new();
        let event = StoreEvent::new(1, PgEvent::DeleteCandidateList(cl_id));

        let entry = AuditLogEntry::new(event, EN);

        assert_eq!(entry.description, "Deleted list of candidates");
        assert!(entry.subject_path.is_empty());
    }

    #[test]
    fn from_update_person_address_event() {
        let person_id = PersonId::new();
        let event = StoreEvent::new(
            1,
            PgEvent::UpdatePersonAddress {
                person_id,
                address: crate::structs::common::DutchAddress::default(),
            },
        );

        let entry = AuditLogEntry::new(event, EN);

        assert_eq!(entry.description, "Updated person address");
        assert_eq!(entry.subject_path, format!("/persons/{person_id}/update"));
    }

    #[test]
    fn from_create_substitute_submitter_event() {
        let submitter = sample_list_submitter(ListSubmitterId::new());
        let expected_name = submitter.name.display();
        let event = StoreEvent::new(1, PgEvent::CreateSubstituteSubmitter(submitter));

        let entry = AuditLogEntry::new(event, EN);

        assert_eq!(
            entry.description,
            "Created substitute submitter of the list"
        );
        assert_eq!(entry.details, expected_name);
    }

    #[test]
    fn from_add_candidate_to_list_event() {
        let list_id = CandidateListId::new();
        let person_id = PersonId::new();
        let event = StoreEvent::new(
            1,
            PgEvent::AddCandidateToCandidateList { list_id, person_id },
        );

        let entry = AuditLogEntry::new(event, EN);

        assert_eq!(entry.description, "Added candidate to list");
        assert_eq!(entry.subject_path, format!("/candidate-lists/{list_id}"));
    }

    #[test]
    fn from_remove_candidate_from_list_event() {
        let list_id = CandidateListId::new();
        let person_id = PersonId::new();
        let event = StoreEvent::new(
            1,
            PgEvent::RemoveCandidateFromCandidateList { list_id, person_id },
        );

        let entry = AuditLogEntry::new(event, EN);

        assert_eq!(entry.description, "Removed candidate from list");
    }

    #[test]
    fn event_category_and_key_are_set_correctly() {
        let person = sample_person(PersonId::new());
        let event = PgEvent::CreatePerson(person);
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
