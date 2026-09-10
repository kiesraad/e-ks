use serde::{Deserialize, Serialize};

use crate::{
    CsbUser, Event, HasCsbUser, StreamId,
    structs::csb::{RegisteredPoliticalGroup, RegisteredPoliticalGroupId},
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
    /// A committee member logged in; the login method is carried by the
    /// event's [`CsbUser`].
    Login,
    /// A committee member signed out.
    Logout,
    /// A political group's registered appellation and previous election result
    /// were recorded for the numbering of the candidate lists.
    CreateRegisteredPoliticalGroup(RegisteredPoliticalGroup),
    UpdateRegisteredPoliticalGroup(RegisteredPoliticalGroup),
    DeleteRegisteredPoliticalGroup(RegisteredPoliticalGroupId),
    UpdateListOrder(Vec<StreamId>),
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

impl Event for CsbMainEvent {
    fn category(&self) -> &'static str {
        match self.action {
            CsbMainAction::Login | CsbMainAction::Logout => "system",
            CsbMainAction::CreateRegisteredPoliticalGroup(_)
            | CsbMainAction::UpdateRegisteredPoliticalGroup(_)
            | CsbMainAction::DeleteRegisteredPoliticalGroup(_) => "registered_political_group",
            CsbMainAction::UpdateListOrder(_) => "numbering",
        }
    }

    fn key(&self) -> &'static str {
        match self.action {
            CsbMainAction::Login => "login",
            CsbMainAction::Logout => "logout",
            CsbMainAction::CreateRegisteredPoliticalGroup(_) => "create_registered_political_group",
            CsbMainAction::UpdateRegisteredPoliticalGroup(_) => "update_registered_political_group",
            CsbMainAction::DeleteRegisteredPoliticalGroup(_) => "delete_registered_political_group",
            CsbMainAction::UpdateListOrder(_) => "update_list_order",
        }
    }

    fn description(&self, locale: crate::Locale) -> String {
        match self.action {
            CsbMainAction::Login => trans!("audit_log.event.login", locale),
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
            CsbMainAction::UpdateListOrder(_) => {
                trans!("audit_log.event.update_list_order", locale)
            }
        }
    }

    fn details(&self) -> String {
        match &self.action {
            CsbMainAction::Login | CsbMainAction::Logout => String::new(),
            CsbMainAction::CreateRegisteredPoliticalGroup(group)
            | CsbMainAction::UpdateRegisteredPoliticalGroup(group) => format!(
                "{}: {} votes, {} seats",
                group.appellation, group.previous_votes, group.previous_seats
            ),
            CsbMainAction::DeleteRegisteredPoliticalGroup(id) => id.to_string(),
            CsbMainAction::UpdateListOrder(order) => order
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Locale, structs::csb::sample_registered_political_group};

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
        assert_eq!(create.details(), "Test Partij: 1234 votes, 2 seats");
        assert_eq!(delete.details(), group.id.to_string());
    }

    #[test]
    fn list_order_event_lists_the_streams_in_order() {
        let first = StreamId::new();
        let second = StreamId::new();
        let event = CsbMainAction::UpdateListOrder(vec![first, second]).by(CsbUser::new_test());

        assert_eq!(event.category(), "numbering");
        assert_eq!(event.key(), "update_list_order");
        assert_eq!(event.description(Locale::En), "Updated list order");
        assert_eq!(event.details(), format!("{first}, {second}"));
    }
}
