//! HTTP middleware and infrastructure endpoints that need [`AppState`](crate::AppState).
//!
//! These depend on [`AppState`](crate::AppState)/[`Config`](crate::Config), so
//! they live in their own top-level module rather than in the otherwise-leaf
//! `utils` and `core` modules (which they must not depend on).

pub mod csb_access;
pub mod eks_key;
pub mod health;
pub mod maintenance;
pub mod session;

#[cfg(not(feature = "memory-serve"))]
pub mod proxy;

#[cfg(feature = "dev-features")]
pub mod dev_login;

pub use csb_access::csb_ip_allow_list_middleware;
pub use eks_key::eks_key_middleware;
pub use health::{health_router, lb_health_router};
pub use maintenance::db_gate_middleware;
pub use session::{csb_store_middleware, session_middleware, store_middleware};

#[cfg(not(feature = "memory-serve"))]
pub use proxy::proxy_handler;
