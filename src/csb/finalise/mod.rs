//! CSB phase 4, "Vaststellen kandidatenlijsten": the public session that
//! decides on the validity and numbering of the candidate lists. Offers the
//! model I 4 downloads, records the order drawn by lot for the lists that are
//! not numbered on votes, and (later) the objections raised.
mod pages;
mod paths;

pub use pages::router;
pub use paths::CsbFinalisePath;
