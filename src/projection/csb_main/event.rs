use serde::{Deserialize, Serialize};

use crate::{
    CredentialId, CsbMainStoreData, CsbUser, Event, HasCsbUser, StreamId,
    structs::{
        audit_log::{AuditEvent, AuditValue, Change, FieldKey, Subject, between, diff},
        csb::{
            HearingDetails, HearingModel, Objection, ObjectionId, RegisteredPoliticalGroup,
            RegisteredPoliticalGroupId,
        },
    },
    trans,
};

/// An event on the global CSB stream: the acting committee member plus what
/// they did. Every event records its user so the audit log can show who
/// triggered it.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CsbMainEvent {
    /// The committee member that triggered the event.
    pub user: CsbUser,
    pub action: CsbMainAction,
}

/// Actions on the global CSB stream. Variants will be added as committee-wide
/// features are implemented (process steps, audit log, etc.).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum CsbMainAction {
    /// A committee member logged in without a security key (development
    /// login); the login method is carried by the event's [`CsbUser`].
    Login,
    /// A committee member logged in with their security key; `sign_count` is
    /// the signature counter it reported, which the next login with the same
    /// key must exceed (WebAuthn §7.2 step 22).
    SecurityKeyLogin {
        credential_id: CredentialId,
        sign_count: u32,
    },
    /// A committee member signed out.
    Logout,
    /// A political group's registered appellation and previous election result
    /// were recorded for the numbering of the candidate lists.
    CreateRegisteredPoliticalGroup(RegisteredPoliticalGroup),
    UpdateRegisteredPoliticalGroup(RegisteredPoliticalGroup),
    DeleteRegisteredPoliticalGroup(RegisteredPoliticalGroupId),
    UpdateHearingDetails(HearingModel, HearingDetails),
    UpdateListOrder(Vec<StreamId>),
    AddObjection(Objection),
    UpdateObjection(Objection),
    DeleteObjection(ObjectionId),
}

impl CsbMainAction {
    /// Attach the acting committee member, producing the event to persist.
    pub fn by(self, user: CsbUser) -> CsbMainEvent {
        CsbMainEvent { user, action: self }
    }
}

impl HasCsbUser for CsbMainEvent {
    fn csb_user(&self) -> &CsbUser {
        &self.user
    }
}

impl AuditEvent for CsbMainEvent {
    type State = CsbMainStoreData;

    fn changes(&self, before: &CsbMainStoreData) -> Vec<Change> {
        let group = |id: &RegisteredPoliticalGroupId| {
            before
                .registered_political_groups
                .iter()
                .find(|group| group.id == *id)
        };
        let objection = |id: &ObjectionId| before.objections.iter().find(|o| o.id == *id);
        match &self.action {
            CsbMainAction::CreateRegisteredPoliticalGroup(g)
            | CsbMainAction::UpdateRegisteredPoliticalGroup(g) => diff(group(&g.id), Some(g)),
            CsbMainAction::DeleteRegisteredPoliticalGroup(id) => diff(group(id), None),
            CsbMainAction::UpdateHearingDetails(model, details) => {
                diff(before.hearing_details.get(model), Some(details))
            }
            CsbMainAction::UpdateListOrder(order) => Vec::from_iter(between(
                FieldKey::ListOrder,
                list_order(&before.list_order),
                list_order(order),
            )),
            CsbMainAction::AddObjection(o) | CsbMainAction::UpdateObjection(o) => {
                diff(objection(&o.id), Some(o))
            }
            CsbMainAction::DeleteObjection(id) => diff(objection(id), None),
            // The login method and the key are in the event's details.
            CsbMainAction::Login
            | CsbMainAction::SecurityKeyLogin { .. }
            | CsbMainAction::Logout => Vec::new(),
        }
    }

    fn subject(&self) -> Option<Subject> {
        match &self.action {
            CsbMainAction::Login
            | CsbMainAction::SecurityKeyLogin { .. }
            | CsbMainAction::Logout => None,
            CsbMainAction::CreateRegisteredPoliticalGroup(g)
            | CsbMainAction::UpdateRegisteredPoliticalGroup(g) => {
                Some(Subject::RegisteredPoliticalGroup(g.id))
            }
            CsbMainAction::DeleteRegisteredPoliticalGroup(id) => {
                Some(Subject::RegisteredPoliticalGroup(*id))
            }
            CsbMainAction::UpdateHearingDetails(model, _) => Some(Subject::HearingDetails(*model)),
            CsbMainAction::UpdateListOrder(_) => Some(Subject::ListOrder),
            CsbMainAction::AddObjection(o) | CsbMainAction::UpdateObjection(o) => {
                Some(Subject::Objection(o.id))
            }
            CsbMainAction::DeleteObjection(id) => Some(Subject::Objection(*id)),
        }
    }
}

impl Event for CsbMainEvent {
    fn category(&self) -> &'static str {
        match self.action {
            CsbMainAction::Login
            | CsbMainAction::SecurityKeyLogin { .. }
            | CsbMainAction::Logout => "system",
            CsbMainAction::CreateRegisteredPoliticalGroup(_)
            | CsbMainAction::UpdateRegisteredPoliticalGroup(_)
            | CsbMainAction::DeleteRegisteredPoliticalGroup(_) => "registered_political_group",
            CsbMainAction::UpdateHearingDetails(..) => "hearing_details",
            CsbMainAction::UpdateListOrder(_) => "numbering",
            CsbMainAction::AddObjection(_)
            | CsbMainAction::UpdateObjection(_)
            | CsbMainAction::DeleteObjection(_) => "objection",
        }
    }

    fn key(&self) -> &'static str {
        match self.action {
            CsbMainAction::Login | CsbMainAction::SecurityKeyLogin { .. } => "login",
            CsbMainAction::Logout => "logout",
            CsbMainAction::CreateRegisteredPoliticalGroup(_) => "create_registered_political_group",
            CsbMainAction::UpdateRegisteredPoliticalGroup(_) => "update_registered_political_group",
            CsbMainAction::DeleteRegisteredPoliticalGroup(_) => "delete_registered_political_group",
            CsbMainAction::UpdateHearingDetails(..) => "update_hearing_details",
            CsbMainAction::UpdateListOrder(_) => "update_list_order",
            CsbMainAction::AddObjection(_) => "add_objection",
            CsbMainAction::UpdateObjection(_) => "update_objection",
            CsbMainAction::DeleteObjection(_) => "delete_objection",
        }
    }

    fn description(&self, locale: crate::Locale) -> String {
        match self.action {
            CsbMainAction::Login | CsbMainAction::SecurityKeyLogin { .. } => {
                trans!("audit_log.event.login", locale)
            }
            CsbMainAction::Logout => trans!("audit_log.event.logout", locale),
            CsbMainAction::CreateRegisteredPoliticalGroup(_) => {
                trans!("audit_log.event.create_registered_political_group", locale)
            }
            CsbMainAction::UpdateRegisteredPoliticalGroup(_) => {
                trans!("audit_log.event.update_registered_political_group", locale)
            }
            CsbMainAction::DeleteRegisteredPoliticalGroup(_) => {
                trans!("audit_log.event.delete_registered_political_group", locale)
            }
            CsbMainAction::UpdateHearingDetails(..) => {
                trans!("audit_log.event.update_hearing_details", locale)
            }
            CsbMainAction::UpdateListOrder(_) => {
                trans!("audit_log.event.update_list_order", locale)
            }
            CsbMainAction::AddObjection(_) => trans!("audit_log.event.add_objection", locale),
            CsbMainAction::UpdateObjection(_) => trans!("audit_log.event.update_objection", locale),
            CsbMainAction::DeleteObjection(_) => trans!("audit_log.event.delete_objection", locale),
        }
    }

    /// What the changes and the subject leave out: the security key a login
    /// used.
    fn details(&self) -> String {
        match &self.action {
            CsbMainAction::SecurityKeyLogin {
                credential_id,
                sign_count,
            } => format!("key {credential_id}, signature counter {sign_count}"),
            _ => String::new(),
        }
    }
}

/// The political groups in drawn order. Their names live on their own
/// streams, so the references carry none; the audit log resolves them.
fn list_order(order: &[StreamId]) -> AuditValue {
    AuditValue::Ordered(
        order
            .iter()
            .map(|stream_id| AuditValue::entity(Subject::PoliticalGroupStream(*stream_id), None))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Locale,
        structs::{
            audit_log::{ChangeKind, FieldPath},
            csb::{HearingDetails, sample_registered_political_group},
        },
    };

    fn state_with(actions: Vec<CsbMainAction>) -> CsbMainStoreData {
        crate::store::replay(actions.into_iter().map(|a| a.by(CsbUser::new_test())))
    }

    #[test]
    fn hearing_details_update_reports_the_changed_fields() {
        let details = HearingDetails {
            date_time: chrono::NaiveDate::from_ymd_opt(2027, 3, 1)
                .unwrap()
                .and_hms_opt(10, 0, 0)
                .unwrap(),
            chair: "Jansen".to_string(),
            members: vec!["Bos".to_string()],
        };
        let before = state_with(vec![CsbMainAction::UpdateHearingDetails(
            HearingModel::I1,
            details.clone(),
        )]);
        let updated = HearingDetails {
            chair: "De Vries".to_string(),
            ..details
        };

        let event =
            CsbMainAction::UpdateHearingDetails(HearingModel::I1, updated).by(CsbUser::new_test());

        assert_eq!(
            event.subject(),
            Some(Subject::HearingDetails(HearingModel::I1))
        );
        assert_eq!(
            event.changes(&before),
            vec![Change {
                path: FieldPath::from(FieldKey::Chair),
                kind: ChangeKind::Changed {
                    old: AuditValue::text("Jansen"),
                    new: AuditValue::text("De Vries"),
                },
            }]
        );
    }

    #[test]
    fn list_order_reports_the_moved_political_group() {
        let (a, b) = (StreamId::new(), StreamId::new());
        let before = state_with(vec![CsbMainAction::UpdateListOrder(vec![a, b])]);

        let event = CsbMainAction::UpdateListOrder(vec![b, a]).by(CsbUser::new_test());
        assert_eq!(event.category(), "numbering");
        assert_eq!(event.key(), "update_list_order");
        assert_eq!(event.description(Locale::En), "Updated list order");
        assert_eq!(event.subject(), Some(Subject::ListOrder));
        let changes = event.changes(&before);

        let [
            Change {
                kind: ChangeKind::Collection { moved, .. },
                ..
            },
        ] = changes.as_slice()
        else {
            panic!("expected one collection change: {changes:#?}");
        };
        assert_eq!(moved.len(), 1);
    }

    #[test]
    fn objection_events_diff_the_objection_text() {
        let objection = Objection {
            id: ObjectionId::new(),
            objection_text: "Bezwaar".parse().unwrap(),
        };
        let added = CsbMainAction::AddObjection(objection.clone()).by(CsbUser::new_test());
        assert_eq!(added.subject(), Some(Subject::Objection(objection.id)));
        assert_eq!(
            added.changes(&CsbMainStoreData::default()),
            vec![Change::added(
                FieldKey::ObjectionText,
                AuditValue::text("Bezwaar")
            )]
        );

        let before = state_with(vec![CsbMainAction::AddObjection(objection.clone())]);
        let deleted = CsbMainAction::DeleteObjection(objection.id).by(CsbUser::new_test());
        assert_eq!(
            deleted.changes(&before),
            vec![Change::removed(
                FieldKey::ObjectionText,
                AuditValue::text("Bezwaar")
            )]
        );
    }

    #[test]
    fn registered_political_group_events_share_a_category() {
        let group = sample_registered_political_group("Test Partij", 1234, 2);
        let create =
            CsbMainAction::CreateRegisteredPoliticalGroup(group.clone()).by(CsbUser::new_test());
        let update =
            CsbMainAction::UpdateRegisteredPoliticalGroup(group.clone()).by(CsbUser::new_test());
        let delete =
            CsbMainAction::DeleteRegisteredPoliticalGroup(group.id).by(CsbUser::new_test());

        for event in [&create, &update, &delete] {
            assert_eq!(event.category(), "registered_political_group");
        }
        assert_eq!(create.key(), "create_registered_political_group");
        assert_eq!(update.key(), "update_registered_political_group");
        assert_eq!(delete.key(), "delete_registered_political_group");
        assert_eq!(create.description(Locale::En), "Registered political group");
    }

    #[test]
    fn security_key_login_is_a_login_recording_the_counter() {
        let event = CsbMainAction::SecurityKeyLogin {
            credential_id: vec![0xfb, 0xff, 0xfe].into(),
            sign_count: 42,
        }
        .by(CsbUser::SecurityKey {
            username: "alice".parse().expect("valid username"),
        });

        assert_eq!(event.category(), "system");
        assert_eq!(event.key(), "login");
        assert_eq!(
            event.description(Locale::En),
            CsbMainAction::Login
                .by(CsbUser::new_test())
                .description(Locale::En)
        );
        assert_eq!(event.details(), "key -__-, signature counter 42");
    }
}
