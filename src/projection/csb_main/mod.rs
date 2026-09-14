mod event;
mod extractor;
mod getters;

pub use event::{CsbMainAction, CsbMainEvent};

use serde::{Deserialize, Serialize};

use crate::{
    Scope, StreamId,
    store::{StoreData, StoreEvent},
    structs::csb::RegisteredPoliticalGroup,
};

/// Fixed stream ID shared by all CSB members for the global committee stream.
pub const CSB_MAIN_STREAM_ID: StreamId = StreamId(uuid::Uuid::from_u128(
    0xC5B0_0000_0000_8000_8000_0000_0000_0001,
));

/// Global CSB state shared across all committee members: process step tracking,
/// audit log entries (logins, imports, etc.), the registered political groups
/// with their previous election results, and other committee-wide events.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CsbMainStoreData {
    pub(crate) events: Vec<StoreEvent<CsbMainEvent>>,
    pub(crate) registered_political_groups: Vec<RegisteredPoliticalGroup>,
    pub(crate) list_order: Vec<StreamId>,
}

impl StoreData for CsbMainStoreData {
    type Event = CsbMainEvent;

    fn apply(&mut self, event: StoreEvent<CsbMainEvent>) {
        self.events.push(event.clone());
        match event.payload.action {
            CsbMainAction::Login | CsbMainAction::Logout => {}
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
            CsbMainAction::UpdateListOrder(order) => {
                self.list_order = order;
            }
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
        use crate::ElectionConfig;

        crate::store::Store {
            stream_id: CSB_MAIN_STREAM_ID,
            election: ElectionConfig::EK27,
            backend: crate::store::StoreBackend::Memory {
                store: crate::store::memory::MemoryStore::default(),
            },
            data: std::sync::Arc::new(parking_lot::RwLock::new(CsbMainStoreData::default())),
        }
    }
}
