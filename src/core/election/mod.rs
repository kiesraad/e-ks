mod configs;
mod districts;
mod domains;
mod macros;
mod public_session;
mod types;

pub use configs::ElectionConfig;
pub use districts::ElectoralDistrict;
pub use domains::{Province, WaterCouncil};
pub use public_session::PublicSession;
pub use types::ElectionType;

use macros::define_elections;
