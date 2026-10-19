mod event;
mod extractor;
mod getters;

pub use event::{CsbMainAction, CsbMainEvent};

use serde::{Deserialize, Serialize};

use std::collections::HashMap;

use crate::{
    AppError, CredentialId, CsbMainStore, CsbUser, Scope, StreamId,
    auth::csb_webauthn::sign_count_advanced,
    store::{StoreData, StoreEvent},
    structs::csb::{HearingDetails, HearingModel, Objection, RegisteredPoliticalGroup},
};

/// Fixed stream ID shared by all CSB members for the global committee stream.
pub const CSB_MAIN_STREAM_ID: StreamId = StreamId(uuid::Uuid::from_u128(
    0xC5B0_0000_0000_8000_8000_0000_0000_0001,
));

/// Global CSB state shared across all committee members: process step tracking,
/// audit log entries (logins, imports, etc.), the registered political groups
/// with their previous election results, and other committee-wide events.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CsbMainStoreData {
    pub(crate) events: Vec<StoreEvent<CsbMainEvent>>,
    pub(crate) registered_political_groups: Vec<RegisteredPoliticalGroup>,
    /// The hearings the committee entered details for, at most one per model.
    pub(crate) hearing_details: HashMap<HearingModel, HearingDetails>,
    pub(crate) list_order: Vec<StreamId>,
    pub(crate) objections: Vec<Objection>,
    /// The signature counter each security key reported at its last login.
    pub(crate) sign_counts: HashMap<CredentialId, u32>,
}

/// Attempts to record a login once the stream moved under it; beyond this
/// the login fails closed rather than spin.
const RECORD_LOGIN_ATTEMPTS: usize = 3;

impl CsbMainStore {
    /// Records a security-key login with its signature counter (WebAuthn
    /// §7.2 step 24) as one step with the step 22 counter check: counter and
    /// stream position come from one snapshot, and the append is refused when
    /// the stream moved since. `false` means the counter did not advance past
    /// what is recorded, or the stream kept moving: the login fails.
    pub async fn record_security_key_login(
        &self,
        user: CsbUser,
        credential_id: CredentialId,
        sign_count: u32,
    ) -> Result<bool, AppError> {
        for _ in 0..RECORD_LOGIN_ATTEMPTS {
            let data = self.snapshot();
            let stored = data.sign_counts.get(&credential_id).copied().unwrap_or(0);
            if !sign_count_advanced(stored, sign_count) {
                return Ok(false);
            }
            let action = CsbMainAction::SecurityKeyLogin {
                credential_id: credential_id.clone(),
                sign_count,
            };
            match self
                .update_if_unchanged(action.by(user.clone()), data.last_event_id())
                .await
            {
                Ok(()) => return Ok(true),
                Err(AppError::Conflict) => self.load().await?,
                Err(err) => return Err(err),
            }
        }
        Ok(false)
    }
}

impl StoreData for CsbMainStoreData {
    type Event = CsbMainEvent;

    fn apply(&mut self, event: StoreEvent<CsbMainEvent>) {
        self.events.push(event.clone());
        match event.payload.action {
            CsbMainAction::Login | CsbMainAction::Logout => {}
            CsbMainAction::SecurityKeyLogin {
                credential_id,
                sign_count,
            } => {
                // Never backwards: two logins racing on two instances must
                // not reopen the clone check.
                let recorded = self.sign_counts.entry(credential_id).or_default();
                *recorded = (*recorded).max(sign_count);
            }
            CsbMainAction::CreateRegisteredPoliticalGroup(group) => {
                self.registered_political_groups.push(group);
            }
            CsbMainAction::UpdateRegisteredPoliticalGroup(group) => {
                if let Some(existing) = self
                    .registered_political_groups
                    .iter_mut()
                    .find(|existing| existing.id == group.id)
                {
                    *existing = group;
                }
            }
            CsbMainAction::DeleteRegisteredPoliticalGroup(id) => {
                self.registered_political_groups
                    .retain(|group| group.id != id);
            }
            CsbMainAction::UpdateHearingDetails(model, hearing_details) => {
                self.hearing_details.insert(model, hearing_details);
            }
            CsbMainAction::UpdateListOrder(order) => {
                self.list_order = order;
            }
            CsbMainAction::AddObjection(objection) => self.objections.push(objection),
            CsbMainAction::UpdateObjection(objection) => {
                if let Some(existing) = self.objections.iter_mut().find(|o| o.id == objection.id) {
                    *existing = objection;
                }
            }
            CsbMainAction::DeleteObjection(id) => self.objections.retain(|o| o.id != id),
        }
    }

    fn events(&self) -> &[StoreEvent<Self::Event>] {
        &self.events
    }

    fn scope() -> Scope {
        Scope::CentralElectoralCommittee
    }
}

#[cfg(test)]
impl crate::CsbMainStore {
    pub fn new_for_test() -> Self {
        Self::new_for_test_with_election(crate::ElectionConfig::EK27)
    }

    pub fn new_for_test_with_election(election: crate::ElectionConfig) -> Self {
        crate::store::Store::new_temp(CSB_MAIN_STREAM_ID, election, std::sync::Arc::default())
    }

    pub fn list_order_events(&self) -> usize {
        self.snapshot()
            .events
            .iter()
            .filter(|event| matches!(event.payload.action, CsbMainAction::UpdateListOrder(_)))
            .count()
    }
}
