//! Pages shared across the CSB section: currently the not-found page for
//! paths under `/csb` that no CSB route claims.

mod pages;
mod paths;

pub use pages::router;
pub use paths::CsbNotFoundPath;
