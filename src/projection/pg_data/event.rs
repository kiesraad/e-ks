use crate::store::EventHash;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::{
    ElectoralDistrict, StreamId,
    structs::{
        candidate_lists::{CandidateList, CandidateListId},
        common::{DutchAddress, FullName},
        list_submitters::{ListSubmitter, ListSubmitterId},
        name_authorisations::{NameAuthorisation, NameAuthorisationId},
        persons::{Person, PersonId, PersonalData, Representative},
        political_groups::PoliticalGroup,
    },
};

/// Domain events that mutate the application store.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum PgEvent {
    UpdatePoliticalGroup(PoliticalGroup),
    CreatePerson(Box<Person>),
    CreatePersonPersonalData {
        person_id: PersonId,
        name: Box<FullName>,
        personal_data: Box<PersonalData>,
    },
    UpdatePerson(Box<Person>),
    UpdatePersonPersonalData {
        person_id: PersonId,
        name: Box<FullName>,
        personal_data: Box<PersonalData>,
    },
    UpdatePersonAddress {
        person_id: PersonId,
        address: Box<DutchAddress>,
    },
    UpdatePersonRepresentative {
        person_id: PersonId,
        representative: Option<Box<Representative>>,
    },
    DeletePerson {
        person_id: PersonId,
    },
    CreateCandidateList(Box<CandidateList>),
    UpdateCandidateListDistricts {
        list_id: CandidateListId,
        electoral_districts: BTreeSet<ElectoralDistrict>,
    },
    UpdateCandidateListOrder {
        list_id: CandidateListId,
        candidates: Vec<PersonId>,
    },
    AddCandidateToCandidateList {
        list_id: CandidateListId,
        person_id: PersonId,
    },
    RemoveCandidateFromCandidateList {
        list_id: CandidateListId,
        person_id: PersonId,
    },
    DeleteCandidateList(CandidateListId),

    CreateNameAuthorisation(Box<NameAuthorisation>),
    UpdateNameAuthorisation(Box<NameAuthorisation>),
    DeleteNameAuthorisation(NameAuthorisationId),

    UpdateListSubmitter(Box<ListSubmitter>),

    CreateSubstituteSubmitter(Box<ListSubmitter>),
    UpdateSubstituteSubmitter(Box<ListSubmitter>),
    DeleteSubstituteSubmitter {
        substitute_submitter_id: ListSubmitterId,
    },

    DeveloperLogin {
        stream_id: StreamId,
    },

    /// The user authenticated via TVS / DigiD. Recorded only once the session
    /// has an election, since that completes the stream's partition key.
    Login,
    /// The user signed out.
    Logout,

    DownloadFile {
        file_name: String,
        download_path: String,
    },
    HideDownloadWarning,

    ExportCsv {
        file_name: String,
        file_size: usize,
        list_id: CandidateListId,
    },

    ImportCandidates(Box<CandidateImport>),

    /// Marks the CSB import that seeds a paper-corrections projection. Not
    /// persisted on an app stream: it is synthesised from
    /// [`CsbAction::Import`](crate::CsbAction::Import) when building the
    /// paper-corrected projection, so the import shows up as event #1 in that
    /// audit log. `hash` is the imported package's source hash.
    Import {
        hash: EventHash,
    },
}

// Large payloads are boxed so each in-memory event stays small.
const _: () = assert!(std::mem::size_of::<PgEvent>() <= 56);

/// Payload of [`PgEvent::ImportCandidates`]; encodes like the struct variant it replaced.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CandidateImport {
    pub list_id: CandidateListId,
    pub file_name: String,
    pub file_size: usize,
    pub created_persons: Vec<Person>,
    pub updated_persons: Vec<Person>,
    pub candidates: Vec<PersonId>,
}

impl PgEvent {
    /// How many persons applying this event inserts (see
    /// `PgStoreData::apply_person_create_event` and
    /// `apply_import_candidates_event`); updates insert none.
    pub fn created_person_count(&self) -> usize {
        match self {
            PgEvent::CreatePerson(_) | PgEvent::CreatePersonPersonalData { .. } => 1,
            PgEvent::ImportCandidates(import) => import.created_persons.len(),
            _ => 0,
        }
    }
}
