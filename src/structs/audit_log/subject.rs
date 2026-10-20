//! What an audit-log event, or a value in its diff, is about.
//!
//! A [`Subject`] names an item without saying where it lives: the political
//! group's part of the app and the CSB's show the same person on different
//! pages. Each part implements [`SubjectLinks`] to resolve a subject to the id
//! it prints and the page it links to.

use std::fmt::Display;

use crate::{
    StreamId,
    structs::{
        candidate_lists::CandidateListId,
        csb::{HearingModel, ObjectionId, OmissionId, RegisteredPoliticalGroupId},
        list_submitters::ListSubmitterId,
        name_authorisations::NameAuthorisationId,
        persons::PersonId,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    /// The political group of the stream the event is on. An event does not
    /// know its stream; [`Subject::in_stream`] fills it in.
    PoliticalGroup,
    /// The political group of a stream, named by its stream id.
    PoliticalGroupStream(StreamId),
    ListSubmitter(ListSubmitterId),
    SubstituteSubmitter(ListSubmitterId),
    Person(PersonId),
    CandidateList(CandidateListId),
    NameAuthorisation(NameAuthorisationId),
    Omission(OmissionId),
    RegisteredPoliticalGroup(RegisteredPoliticalGroupId),
    Objection(ObjectionId),
    HearingDetails(HearingModel),
    ListOrder,
}

impl Subject {
    /// This subject as seen from the stream `stream_id`, which is the stream
    /// of [`Subject::PoliticalGroup`].
    pub fn in_stream(self, stream_id: StreamId) -> Self {
        match self {
            Subject::PoliticalGroup => Subject::PoliticalGroupStream(stream_id),
            other => other,
        }
    }

    /// The id the subject carries; `None` for the singletons.
    pub fn own_id(&self) -> Option<String> {
        let id: &dyn Display = match self {
            Subject::PoliticalGroupStream(id) => id,
            Subject::ListSubmitter(id) | Subject::SubstituteSubmitter(id) => id,
            Subject::Person(id) => id,
            Subject::CandidateList(id) => id,
            Subject::NameAuthorisation(id) => id,
            Subject::Omission(id) => id,
            Subject::RegisteredPoliticalGroup(id) => id,
            Subject::Objection(id) => id,
            Subject::PoliticalGroup | Subject::HearingDetails(_) | Subject::ListOrder => {
                return None;
            }
        };
        Some(id.to_string())
    }
}

/// A subject as one part of the app shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct SubjectLink {
    pub id_full: Option<String>,
    /// The item's page; `None` when it has none here or no longer exists.
    pub href: Option<String>,
}

/// Resolves subjects for the audit log of one part of the app.
pub trait SubjectLinks {
    fn link(&self, subject: Subject) -> SubjectLink;

    /// How the item is known now, for a reference recorded without a name.
    fn describe(&self, _subject: Subject) -> Option<String> {
        None
    }
}

/// Prints a subject's own id and links nowhere.
#[cfg(test)]
pub struct NoLinks;

#[cfg(test)]
impl SubjectLinks for NoLinks {
    fn link(&self, subject: Subject) -> SubjectLink {
        SubjectLink {
            id_full: subject.own_id(),
            href: None,
        }
    }
}
