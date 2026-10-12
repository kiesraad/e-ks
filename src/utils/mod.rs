//! Utilities and small helpers shared across the application.
mod abbreviate_str;
mod format_hash;
mod option_string_ext;
mod overlay;
mod peer_ip_label;
mod query_param_state;
mod query_suffix;
mod redirect;
mod request_flags;
mod sha256_hex;
mod storage_url;

pub mod bag;
pub mod locality_aliases;
pub mod no_cache_headers;
pub mod pagination;
pub mod transparent_string;

#[cfg(feature = "livereload")]
pub mod livereload;

pub use abbreviate_str::abbreviate_str;
pub use format_hash::{format_hash, parse_hash_prefix};
pub use option_string_ext::{OptionAsStrExt, OptionStringExt};
pub use overlay::Overlay;
pub use peer_ip_label::peer_ip_label;
pub use query_param_state::QueryParamState;
pub use query_suffix::filter_query_suffix;
pub use redirect::{redirect_success, redirect_to_referer};
pub use request_flags::{overlay_active, success_alert_requested};
pub use sha256_hex::sha256_hex;
pub use storage_url::StorageScheme;
#[cfg(not(feature = "database"))]
pub use storage_url::database_disabled_error;
