//! Event-level helpers shared by `AuditLogEntry` and `AuditLogDetail`.
//!
//! Each function maps an `PgEvent` to a single piece of information needed
//! by the audit log UI (translated description, human-readable details and
//! the item the event is about).

use std::collections::BTreeSet;

use crate::{
    ElectoralDistrict, Event, Locale, PgEvent, PgStoreData,
    structs::audit_log::{AuditEvent, Change, Subject},
    trans,
    utils::format_hash,
};

/// Translated label describing what the event did.
#[expect(
    clippy::cognitive_complexity,
    reason = "A flat translation table; the `trans!` expansions inflate the metric."
)]
fn event_description(event: &PgEvent, locale: Locale) -> String {
    match event {
        PgEvent::UpdatePoliticalGroup(_) => {
            trans!("audit_log.event.update_political_group", locale)
        }
        PgEvent::CreatePerson(_) | PgEvent::CreatePersonPersonalData { .. } => {
            trans!("audit_log.event.create_person", locale)
        }
        PgEvent::UpdatePerson(_) | PgEvent::UpdatePersonPersonalData { .. } => {
            trans!("audit_log.event.update_person", locale)
        }
        PgEvent::UpdatePersonAddress { .. } => {
            trans!("audit_log.event.update_person_address", locale)
        }
        PgEvent::UpdatePersonRepresentative { .. } => {
            trans!("audit_log.event.update_person_representative", locale)
        }
        PgEvent::DeletePerson { .. } => trans!("audit_log.event.delete_person", locale),
        PgEvent::CreateCandidateList(_) => trans!("audit_log.event.create_candidate_list", locale),
        PgEvent::UpdateCandidateListDistricts { .. } => {
            trans!("audit_log.event.update_candidate_list_districts", locale)
        }
        PgEvent::UpdateCandidateListOrder { .. } => {
            trans!("audit_log.event.update_candidate_list_order", locale)
        }
        PgEvent::AddCandidateToCandidateList { .. } => {
            trans!("audit_log.event.add_candidate_to_list", locale)
        }
        PgEvent::RemoveCandidateFromCandidateList { .. } => {
            trans!("audit_log.event.remove_candidate_from_list", locale)
        }
        PgEvent::DeleteCandidateList(_) => trans!("audit_log.event.delete_candidate_list", locale),
        PgEvent::CreateNameAuthorisation(_) => {
            trans!("audit_log.event.create_name_authorisation", locale)
        }
        PgEvent::UpdateNameAuthorisation(_) => {
            trans!("audit_log.event.update_name_authorisation", locale)
        }
        PgEvent::DeleteNameAuthorisation(_) => {
            trans!("audit_log.event.delete_name_authorisation", locale)
        }
        PgEvent::UpdateListSubmitter(_) => trans!("audit_log.event.update_list_submitter", locale),
        PgEvent::CreateSubstituteSubmitter(_) => {
            trans!("audit_log.event.create_substitute_submitter", locale)
        }
        PgEvent::UpdateSubstituteSubmitter(_) => {
            trans!("audit_log.event.update_substitute_submitter", locale)
        }
        PgEvent::DeleteSubstituteSubmitter { .. } => {
            trans!("audit_log.event.delete_substitute_submitter", locale)
        }
        PgEvent::DeveloperLogin { .. } => trans!("audit_log.event.developer_login", locale),
        PgEvent::Login => trans!("audit_log.event.login", locale),
        PgEvent::Logout => trans!("audit_log.event.logout", locale),
        PgEvent::DownloadFile { .. } => trans!("audit_log.event.download_file", locale),
        PgEvent::HideDownloadWarning => trans!("audit_log.event.hide_download_warning", locale),
        PgEvent::ExportCsv { .. } => trans!("audit_log.event.export_csv", locale),
        PgEvent::ImportCandidates { .. } => trans!("audit_log.event.import_csv", locale),
        PgEvent::Import { .. } => trans!("audit_log.event.import", locale),
    }
}

/// Short human-readable details for a listing row (name, file, districts, ...).
fn event_details(event: &PgEvent) -> String {
    fn district_titles(districts: &BTreeSet<ElectoralDistrict>) -> String {
        districts
            .iter()
            .map(ElectoralDistrict::title)
            .collect::<Vec<_>>()
            .join(", ")
    }

    match event {
        PgEvent::UpdatePoliticalGroup(pg) => pg
            .appellation
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        PgEvent::CreatePerson(p) | PgEvent::UpdatePerson(p) => p.name.display(),
        PgEvent::CreatePersonPersonalData { name, .. }
        | PgEvent::UpdatePersonPersonalData { name, .. } => name.display(),
        PgEvent::CreateCandidateList(cl) => district_titles(&cl.electoral_districts),
        PgEvent::UpdateCandidateListDistricts {
            electoral_districts,
            ..
        } => district_titles(electoral_districts),
        PgEvent::CreateNameAuthorisation(aa) | PgEvent::UpdateNameAuthorisation(aa) => {
            format!("{} ({})", aa.legal_name, aa.name.display())
        }
        PgEvent::UpdateListSubmitter(ls) => ls.name.display(),
        PgEvent::CreateSubstituteSubmitter(ss) | PgEvent::UpdateSubstituteSubmitter(ss) => {
            ss.name.display()
        }
        PgEvent::DownloadFile { file_name, .. } | PgEvent::ExportCsv { file_name, .. } => {
            file_name.clone()
        }
        PgEvent::ImportCandidates(import) => import.file_name.clone(),
        PgEvent::Import { hash } => format_hash(hash, true),
        PgEvent::UpdatePersonAddress { .. }
        | PgEvent::UpdatePersonRepresentative { .. }
        | PgEvent::DeletePerson { .. }
        | PgEvent::UpdateCandidateListOrder { .. }
        | PgEvent::AddCandidateToCandidateList { .. }
        | PgEvent::RemoveCandidateFromCandidateList { .. }
        | PgEvent::DeleteCandidateList(..)
        | PgEvent::DeleteNameAuthorisation(..)
        | PgEvent::DeleteSubstituteSubmitter { .. }
        | PgEvent::DeveloperLogin { .. }
        | PgEvent::Login
        | PgEvent::Logout
        | PgEvent::HideDownloadWarning => PgEvent::DEFAULT_DETAILS.to_string(),
    }
}

impl PgEvent {
    pub const DEFAULT_DETAILS: &str = "-";
}

/// The item an event is about.
fn event_subject(event: &PgEvent) -> Option<Subject> {
    match event {
        PgEvent::UpdatePoliticalGroup(_) | PgEvent::Import { .. } => Some(Subject::PoliticalGroup),
        PgEvent::CreatePerson(p) | PgEvent::UpdatePerson(p) => Some(Subject::Person(p.id)),
        PgEvent::CreatePersonPersonalData { person_id, .. }
        | PgEvent::UpdatePersonPersonalData { person_id, .. }
        | PgEvent::UpdatePersonAddress { person_id, .. }
        | PgEvent::UpdatePersonRepresentative { person_id, .. }
        | PgEvent::DeletePerson { person_id } => Some(Subject::Person(*person_id)),
        PgEvent::CreateCandidateList(cl) => Some(Subject::CandidateList(cl.id)),
        PgEvent::UpdateCandidateListDistricts { list_id, .. }
        | PgEvent::UpdateCandidateListOrder { list_id, .. }
        | PgEvent::AddCandidateToCandidateList { list_id, .. }
        | PgEvent::RemoveCandidateFromCandidateList { list_id, .. }
        | PgEvent::DeleteCandidateList(list_id)
        | PgEvent::ExportCsv { list_id, .. } => Some(Subject::CandidateList(*list_id)),
        PgEvent::ImportCandidates(import) => Some(Subject::CandidateList(import.list_id)),
        PgEvent::CreateNameAuthorisation(na) | PgEvent::UpdateNameAuthorisation(na) => {
            Some(Subject::NameAuthorisation(na.id))
        }
        PgEvent::DeleteNameAuthorisation(id) => Some(Subject::NameAuthorisation(*id)),
        PgEvent::UpdateListSubmitter(ls) => Some(Subject::ListSubmitter(ls.id)),
        PgEvent::CreateSubstituteSubmitter(ss) | PgEvent::UpdateSubstituteSubmitter(ss) => {
            Some(Subject::SubstituteSubmitter(ss.id))
        }
        PgEvent::DeleteSubstituteSubmitter {
            substitute_submitter_id,
        } => Some(Subject::SubstituteSubmitter(*substitute_submitter_id)),
        // The stream id is one of the event's changes.
        PgEvent::DeveloperLogin { .. }
        | PgEvent::Login
        | PgEvent::Logout
        | PgEvent::DownloadFile { .. }
        | PgEvent::HideDownloadWarning => None,
    }
}

impl Event for PgEvent {
    /// Return a stable category key for filtering in the audit log.
    fn category(&self) -> &'static str {
        match self {
            PgEvent::UpdatePoliticalGroup(_) => "political_group",
            PgEvent::CreatePerson(_)
            | PgEvent::CreatePersonPersonalData { .. }
            | PgEvent::UpdatePerson(_)
            | PgEvent::UpdatePersonPersonalData { .. }
            | PgEvent::UpdatePersonAddress { .. }
            | PgEvent::UpdatePersonRepresentative { .. }
            | PgEvent::DeletePerson { .. } => "person",
            PgEvent::CreateCandidateList(_)
            | PgEvent::UpdateCandidateListDistricts { .. }
            | PgEvent::UpdateCandidateListOrder { .. }
            | PgEvent::AddCandidateToCandidateList { .. }
            | PgEvent::RemoveCandidateFromCandidateList { .. }
            | PgEvent::DeleteCandidateList(_) => "candidate_list",
            PgEvent::CreateNameAuthorisation(_)
            | PgEvent::UpdateNameAuthorisation(_)
            | PgEvent::DeleteNameAuthorisation(_) => "name_authorisation",
            PgEvent::UpdateListSubmitter(_) => "list_submitter",
            PgEvent::CreateSubstituteSubmitter(_)
            | PgEvent::UpdateSubstituteSubmitter(_)
            | PgEvent::DeleteSubstituteSubmitter { .. } => "substitute_submitter",
            PgEvent::DeveloperLogin { .. }
            | PgEvent::Login
            | PgEvent::Logout
            | PgEvent::DownloadFile { .. }
            | PgEvent::HideDownloadWarning
            | PgEvent::ExportCsv { .. }
            | PgEvent::ImportCandidates { .. } => "system",
            PgEvent::Import { .. } => "import",
        }
    }

    /// Return a stable snake_case key identifying the event variant.
    /// Variants that share a user-facing description share a key (e.g. both
    /// `CreatePerson` and `CreatePersonPersonalData` map to `create_person`).
    fn key(&self) -> &'static str {
        match self {
            PgEvent::UpdatePoliticalGroup(_) => "update_political_group",
            PgEvent::CreatePerson(_) | PgEvent::CreatePersonPersonalData { .. } => "create_person",
            PgEvent::UpdatePerson(_) | PgEvent::UpdatePersonPersonalData { .. } => "update_person",
            PgEvent::UpdatePersonAddress { .. } => "update_person_address",
            PgEvent::UpdatePersonRepresentative { .. } => "update_person_representative",
            PgEvent::DeletePerson { .. } => "delete_person",
            PgEvent::CreateCandidateList(_) => "create_candidate_list",
            PgEvent::UpdateCandidateListDistricts { .. } => "update_candidate_list_districts",
            PgEvent::UpdateCandidateListOrder { .. } => "update_candidate_list_order",
            PgEvent::AddCandidateToCandidateList { .. } => "add_candidate_to_list",
            PgEvent::RemoveCandidateFromCandidateList { .. } => "remove_candidate_from_list",
            PgEvent::DeleteCandidateList(_) => "delete_candidate_list",
            PgEvent::CreateNameAuthorisation(_) => "create_name_authorisation",
            PgEvent::UpdateNameAuthorisation(_) => "update_name_authorisation",
            PgEvent::DeleteNameAuthorisation(_) => "delete_name_authorisation",
            PgEvent::UpdateListSubmitter(_) => "update_list_submitter",
            PgEvent::CreateSubstituteSubmitter(_) => "create_substitute_submitter",
            PgEvent::UpdateSubstituteSubmitter(_) => "update_substitute_submitter",
            PgEvent::DeleteSubstituteSubmitter { .. } => "delete_substitute_submitter",
            PgEvent::DeveloperLogin { .. } => "developer_login",
            PgEvent::Login => "login",
            PgEvent::Logout => "logout",
            PgEvent::DownloadFile { .. } => "download_file",
            PgEvent::HideDownloadWarning => "hide_download_warning",
            PgEvent::ExportCsv { .. } => "export_csv",
            PgEvent::ImportCandidates { .. } => "import_csv",
            PgEvent::Import { .. } => "import",
        }
    }

    fn description(&self, locale: crate::Locale) -> String {
        event_description(self, locale)
    }

    fn details(&self) -> String {
        event_details(self)
    }
}

impl AuditEvent for PgEvent {
    type State = PgStoreData;

    fn changes(&self, before: &PgStoreData) -> Vec<Change> {
        self.audit_changes(before)
    }

    fn subject(&self) -> Option<Subject> {
        event_subject(self)
    }
}
