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
    CreatePerson(Person),
    CreatePersonPersonalData {
        person_id: PersonId,
        name: FullName,
        personal_data: PersonalData,
    },
    UpdatePerson(Person),
    UpdatePersonPersonalData {
        person_id: PersonId,
        name: FullName,
        personal_data: PersonalData,
    },
    UpdatePersonAddress {
        person_id: PersonId,
        address: DutchAddress,
    },
    UpdatePersonRepresentative {
        person_id: PersonId,
        representative: Option<Representative>,
    },
    DeletePerson {
        person_id: PersonId,
    },
    CreateCandidateList(CandidateList),
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

    CreateNameAuthorisation(NameAuthorisation),
    UpdateNameAuthorisation(NameAuthorisation),
    DeleteNameAuthorisation(NameAuthorisationId),

    UpdateListSubmitter(ListSubmitter),

    CreateSubstituteSubmitter(ListSubmitter),
    UpdateSubstituteSubmitter(ListSubmitter),
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

    ImportCandidates {
        list_id: CandidateListId,
        file_name: String,
        file_size: usize,
        created_persons: Vec<Person>,
        updated_persons: Vec<Person>,
        candidates: Vec<PersonId>,
    },

    /// Marks the CSB import that seeds a paper-corrections projection. Not
    /// persisted on an app stream: it is synthesised from
    /// [`CsbAction::Import`](crate::CsbAction::Import) when building the
    /// paper-corrected projection, so the import shows up as event #1 in that
    /// audit log. `hash` is the imported package's source hash.
    Import {
        hash: EventHash,
    },
}
