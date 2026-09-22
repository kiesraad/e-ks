use crate::store::EventHash;
use serde::{Deserialize, Serialize};

use crate::{
    CsbStoreData, CsbUser, Event, HasCsbUser, PgEvent, PgStoreData, StreamId,
    structs::{
        audit_log::{Change, diff},
        brp::{BrpFinding, BrpStatus},
        csb::{Correction, Omission, OmissionId, OmissionPart, OmissionStatus},
        persons::PersonId,
    },
    trans,
    utils::format_hash,
};

/// An event on a CSB store: the acting committee member plus what they did.
/// Every event records its user so the audit log can show who triggered it.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CsbEvent {
    /// The committee member that triggered the event.
    pub user: CsbUser,
    pub action: CsbAction,
}

impl CsbAction {
    /// Attach the acting committee member, producing the event to persist.
    pub fn by(self, user: CsbUser) -> CsbEvent {
        CsbEvent { user, action: self }
    }
}

impl HasCsbUser for CsbEvent {
    fn csb_user(&self) -> &CsbUser {
        &self.user
    }
}

impl Event for CsbEvent {
    type State = CsbStoreData;

    fn category(&self) -> &'static str {
        self.action.category()
    }

    fn key(&self) -> &'static str {
        self.action.key()
    }

    fn description(&self, locale: crate::Locale) -> String {
        self.action.description(locale)
    }

    fn details(&self) -> String {
        self.action.details()
    }

    fn changes(&self, before: &CsbStoreData) -> Vec<Change> {
        self.action.changes(before)
    }
}

/// Domain actions that mutate the CSB (Centraal Stembureau) store.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum CsbAction {
    /// Import a submitted candidate-list package, identified by the chain hash
    /// of the event stream it was produced from.
    ///
    /// Carries a snapshot of the source [`PgStoreData`] reconstructed by
    /// replaying the source stream up to the matched event (see
    /// [`PgStoreData::snapshot_until`]). The import is persisted under a fresh
    /// CSB stream (never the source partition, which holds the PG stream's own
    /// events), so `source_stream_id` is recorded for reference. The election is
    /// not: it is copied onto the CSB stream's own `(stream_id, election)` key.
    Import {
        /// Hash of the imported event
        hash: EventHash,
        /// Stream the imported package was produced from
        source_stream_id: StreamId,
        /// Snapshot of the source projection at the matched event, with its own
        /// event log excluded. Boxed to keep the event enum small.
        snapshot: Box<PgStoreData>,
    },
    /// Create an empty political-group store without importing from a PG stream.
    CreateEmpty,
    /// Delete a political-group stream
    Delete,
    /// An app event applied to the paper-corrected projection instead of a
    /// political group's own stream. Boxed to keep the event enum small.
    PaperCorrectedUpdate(Box<PgEvent>),
    SetFinished(bool),
    CreateOmission(Omission),
    UpdateOmission(Omission),
    DeleteOmission {
        omission_id: OmissionId,
    },
    /// Record whether an omission was recovered ("hersteld") during the
    /// "Herstelde lijsten" phase.
    SetOmissionStatus {
        omission_id: OmissionId,
        status: OmissionStatus,
    },
    /// Record the decision for one part of an omission. The projection splits
    /// the part off while the omission covers other parts, and reads parts
    /// decided the same way as one omission.
    SetOmissionPartStatus {
        omission_id: OmissionId,
        part: OmissionPart,
        status: OmissionStatus,
    },
    UpdateCorrection(Correction),
    /// Empty `findings` means checked, with the BRP agreeing on every field.
    BrpPersonChecked {
        person: PersonId,
        findings: Vec<BrpFinding>,
    },
    SetBrpStatus(BrpStatus),
}

impl CsbAction {
    /// Field-level changes this action makes to `before`, for the audit log.
    fn changes(&self, before: &CsbStoreData) -> Vec<Change> {
        let omission = |id: &OmissionId| before.omissions.get(id);
        match self {
            // The snapshot is what the import brings in; it is summarised, not
            // diffed field by field.
            CsbAction::Import { snapshot, .. } => snapshot.import_summary(),
            CsbAction::PaperCorrectedUpdate(event) => event.changes(&before.paper_corrected_data),
            CsbAction::UpdateCorrection(correction) => correction_changes(before, correction),
            CsbAction::CreateOmission(o) | CsbAction::UpdateOmission(o) => {
                diff(omission(&o.id), Some(o))
            }
            CsbAction::DeleteOmission { omission_id } => diff(omission(omission_id), None),
            CsbAction::SetOmissionStatus {
                omission_id,
                status,
            } => {
                let old = omission(omission_id);
                let new = old.cloned().map(|mut o| {
                    o.status = *status;
                    o
                });
                diff(old, new.as_ref())
            }
            CsbAction::CreateEmpty
            | CsbAction::Delete
            | CsbAction::SetFinished(_)
            | CsbAction::SetOmissionPartStatus { .. }
            | CsbAction::BrpPersonChecked { .. }
            | CsbAction::SetBrpStatus(_) => Vec::new(),
        }
    }

    fn category(&self) -> &'static str {
        match self {
            CsbAction::Import { .. } => "import",
            CsbAction::CreateEmpty => "import",
            CsbAction::Delete => "delete",
            CsbAction::PaperCorrectedUpdate(_) => "paper_correction",
            CsbAction::SetFinished(_) => "set_finished",
            CsbAction::CreateOmission(_)
            | CsbAction::UpdateOmission(_)
            | CsbAction::DeleteOmission { .. }
            | CsbAction::SetOmissionStatus { .. }
            | CsbAction::SetOmissionPartStatus { .. } => "omission",
            CsbAction::UpdateCorrection(_) => "correction",
            CsbAction::BrpPersonChecked { .. } | CsbAction::SetBrpStatus(_) => "brp_validation",
        }
    }

    fn key(&self) -> &'static str {
        match self {
            CsbAction::Import { .. } => "import",
            CsbAction::CreateEmpty => "create_empty",
            CsbAction::Delete => "delete",
            CsbAction::PaperCorrectedUpdate(event) => event.key(),
            CsbAction::SetFinished(_) => "set_finished",
            CsbAction::CreateOmission(_) => "create_omission",
            CsbAction::UpdateOmission(_) => "update_omission",
            CsbAction::DeleteOmission { .. } => "delete_omission",
            CsbAction::SetOmissionStatus { .. } => "set_omission_status",
            CsbAction::SetOmissionPartStatus { .. } => "set_omission_part_status",
            CsbAction::UpdateCorrection(_) => "update_correction",
            CsbAction::BrpPersonChecked { .. } => "brp_person_checked",
            CsbAction::SetBrpStatus(_) => "brp_validation",
        }
    }

    fn description(&self, locale: crate::Locale) -> String {
        match self {
            CsbAction::Import { .. } => trans!("audit_log.event.import", locale),
            CsbAction::Delete => trans!("audit_log.event.delete", locale),
            CsbAction::CreateEmpty => trans!("audit_log.event.create_empty", locale),
            CsbAction::PaperCorrectedUpdate(event) => event.description(locale),
            CsbAction::SetFinished(_) => trans!("audit_log.event.set_finished", locale),
            CsbAction::CreateOmission(_) => trans!("audit_log.event.create_omission", locale),
            CsbAction::UpdateOmission(_) => trans!("audit_log.event.update_omission", locale),
            CsbAction::DeleteOmission { .. } => trans!("audit_log.event.delete_omission", locale),
            CsbAction::SetOmissionStatus { .. } => {
                trans!("audit_log.event.set_omission_status", locale)
            }
            CsbAction::SetOmissionPartStatus { .. } => {
                trans!("audit_log.event.set_omission_part_status", locale)
            }
            CsbAction::UpdateCorrection { .. } => {
                trans!("audit_log.event.update_correction", locale)
            }
            CsbAction::BrpPersonChecked { .. } => {
                trans!("audit_log.event.brp_validation", locale)
            }
            CsbAction::SetBrpStatus(_) => {
                trans!("audit_log.event.set_brp_validation_state", locale)
            }
        }
    }

    fn details(&self) -> String {
        match self {
            CsbAction::Import {
                hash,
                source_stream_id,
                ..
            } => {
                format!(
                    "Hash: {}\nSource stream: {source_stream_id}",
                    format_hash(hash, true)
                )
            }
            CsbAction::Delete => String::new(),
            CsbAction::CreateEmpty => String::new(),
            CsbAction::PaperCorrectedUpdate(event) => event.details(),
            CsbAction::SetFinished(value) => value.to_string(),
            CsbAction::CreateOmission(o) | CsbAction::UpdateOmission(o) => {
                o.description.to_string()
            }
            CsbAction::DeleteOmission { omission_id } => omission_id.to_string(),
            CsbAction::SetOmissionStatus {
                omission_id,
                status,
            } => {
                format!("{omission_id}: {status:?}")
            }
            CsbAction::SetOmissionPartStatus {
                omission_id,
                part,
                status,
            } => {
                format!("{omission_id}: {part:?} {status:?}")
            }
            CsbAction::UpdateCorrection(_) => String::new(),
            CsbAction::BrpPersonChecked { person, .. } => person.to_string(),
            CsbAction::SetBrpStatus(value) => value.to_string(),
        }
    }
}

/// What a committee correction changes: the corrected value against the
/// entity as corrected so far (paper corrections and earlier committee
/// corrections included).
fn correction_changes(before: &CsbStoreData, correction: &Correction) -> Vec<Change> {
    match correction {
        Correction::Appellation(appellation) => {
            let old = before.corrected_political_group();
            let mut new = old.clone();
            new.appellation = Some(appellation.clone());
            diff(Some(&old), Some(&new))
        }
        Correction::Person(person_id, correction) => {
            let old = before.corrected_person(*person_id);
            let new = old.clone().map(|mut person| {
                correction.clone().apply(&mut person);
                person
            });
            diff(old.as_ref(), new.as_ref())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        store::{StoreData, StoreEvent},
        structs::{
            audit_log::{AuditValue, ChangeKind, FieldKey, FieldPath},
            csb::{OmissionCategory, PersonCorrection, sample_omission},
        },
        test_utils::sample_person,
    };

    fn state_with(actions: Vec<CsbAction>) -> CsbStoreData {
        let mut data = CsbStoreData::default();
        for (index, action) in actions.into_iter().enumerate() {
            data.apply(StoreEvent::new(index + 1, action.by(CsbUser::new_test())));
        }
        data
    }

    fn import_of(person: &crate::structs::persons::Person) -> CsbAction {
        let mut snapshot = PgStoreData::default();
        snapshot.persons.insert(person.id, person.clone());
        CsbAction::Import {
            hash: [1; 32],
            source_stream_id: StreamId::new(),
            snapshot: Box::new(snapshot),
        }
    }

    #[test]
    fn correction_diffs_against_the_person_as_corrected_so_far() {
        let person = sample_person(PersonId::new());
        let before = state_with(vec![
            import_of(&person),
            CsbAction::UpdateCorrection(Correction::Person(
                person.id,
                PersonCorrection::LastName("Eerste".parse().unwrap()),
            )),
        ]);

        let changes = CsbAction::UpdateCorrection(Correction::Person(
            person.id,
            PersonCorrection::LastName("Tweede".parse().unwrap()),
        ))
        .changes(&before);

        assert_eq!(
            changes,
            vec![Change {
                path: FieldPath::from(FieldKey::LastName),
                kind: ChangeKind::Changed {
                    old: AuditValue::text("Eerste"),
                    new: AuditValue::text("Tweede"),
                },
            }]
        );
    }

    #[test]
    fn appellation_correction_diffs_the_political_group() {
        let before = state_with(vec![import_of(&sample_person(PersonId::new()))]);

        let changes =
            CsbAction::UpdateCorrection(Correction::Appellation("Nieuwe Naam".parse().unwrap()))
                .changes(&before);

        assert_eq!(
            changes,
            vec![Change::added(
                FieldKey::Appellation,
                AuditValue::text("Nieuwe Naam")
            )]
        );
    }

    #[test]
    fn paper_corrected_update_diffs_the_paper_corrected_projection() {
        let person = sample_person(PersonId::new());
        let before = state_with(vec![import_of(&person)]);
        let mut updated = person.clone();
        updated.name.first_name = Some("Gecorrigeerd".parse().unwrap());

        let changes = CsbAction::PaperCorrectedUpdate(Box::new(PgEvent::UpdatePerson(updated)))
            .changes(&before);

        assert_eq!(
            changes,
            vec![Change {
                path: FieldPath::from(FieldKey::FirstName),
                kind: ChangeKind::Changed {
                    old: AuditValue::text("Henk"),
                    new: AuditValue::text("Gecorrigeerd"),
                },
            }]
        );
    }

    #[test]
    fn import_is_summarised_and_omissions_are_diffed() {
        let import = import_of(&sample_person(PersonId::new()));
        let summary = import.changes(&CsbStoreData::default());
        assert_eq!(
            summary[0],
            Change::added(FieldKey::Persons, AuditValue::text(1))
        );

        let omission = sample_omission(OmissionCategory::PoliticalGroup);
        let before = state_with(vec![import, CsbAction::CreateOmission(omission.clone())]);

        let changes = CsbAction::SetOmissionStatus {
            omission_id: omission.id,
            status: OmissionStatus::Recovered,
        }
        .changes(&before);

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, FieldPath::from(FieldKey::OmissionStatus));
        assert!(matches!(changes[0].kind, ChangeKind::Changed { .. }));

        assert!(CsbAction::SetFinished(true).changes(&before).is_empty());
    }

    fn import_event() -> CsbEvent {
        CsbAction::Import {
            hash: [42; 32],
            source_stream_id: StreamId::default(),
            snapshot: Box::new(PgStoreData::default()),
        }
        .by(CsbUser::new_test())
    }

    #[test]
    fn import_event_category() {
        assert_eq!(import_event().category(), "import");
    }

    #[test]
    fn import_event_key() {
        assert_eq!(import_event().key(), "import");
    }

    /// The audit-log metadata of a paper correction delegates to the wrapped
    /// app event, under its own category.
    #[test]
    fn paper_corrected_update_delegates_to_inner_event() {
        let event = CsbAction::PaperCorrectedUpdate(Box::new(PgEvent::UpdatePoliticalGroup(
            crate::structs::political_groups::PoliticalGroup::default(),
        )));

        assert_eq!(event.category(), "paper_correction");
        assert_eq!(event.key(), "update_political_group");
        assert_eq!(
            event.description(crate::Locale::En),
            "Updated political group"
        );
    }
}
