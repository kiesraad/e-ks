//! Administration of the registered political groups and their result at the
//! previous election, recorded per election on the CSB main stream.
//!
//! The central electoral committee numbers the candidate lists (Kieswet Art.
//! I 12 to I 15, Hoofdstuk S for the Eerste Kamer): the lists of political
//! groups that obtained one or more seats at the previous election of the same
//! body come first, in the order of the number of votes cast on them; the
//! remaining lists are numbered by lot. The appellations, votes and seats
//! recorded here are what that first numbering on model I 4 is based on.
mod forms;
mod pages;
pub(in crate::csb) mod paths;

pub use forms::RegisteredPoliticalGroupForm;
pub use pages::router;
pub use paths::{CsbAddRegisteredPoliticalGroupPath, CsbRegisteredPoliticalGroupsPath};
